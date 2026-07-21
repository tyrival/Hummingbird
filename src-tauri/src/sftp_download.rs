use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
    io::Read,
    net::TcpStream,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

use crate::error::{AppError, ErrorCode};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshServerConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default, alias = "private_key")]
    pub private_key: Option<String>,
    #[serde(default, alias = "app_root")]
    pub app_root: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteFile {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub modified: u64,
}

pub fn parse_servers_from_config(encrypted: &str) -> Result<Vec<SshServerConfig>, AppError> {
    let json_str = crate::crypto::decrypt_ssh_config(encrypted)?;
    if json_str.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&json_str).map_err(|_| AppError::new(ErrorCode::InvalidSettings))
}

pub fn serialize_servers_to_config(servers: &[SshServerConfig]) -> Result<String, AppError> {
    let json_str =
        serde_json::to_string(servers).map_err(|_| AppError::new(ErrorCode::InvalidSettings))?;
    crate::crypto::encrypt_ssh_config(&json_str)
}

fn connect_session(server: &SshServerConfig) -> Result<Session, AppError> {
    let addr = format!("{}:{}", server.host, server.port);
    let tcp = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?,
        Duration::from_secs(10),
    )
    .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;

    let mut session = Session::new().map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;
    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;

    // Try key first if available, then fall back to password.
    // Both can be present — key takes precedence.
    let mut authenticated = false;
    if let Some(key) = &server.private_key {
        eprintln!(
            "[connect_session] trying pubkey auth for user={}",
            server.user
        );
        if let Ok(()) = session.userauth_pubkey_memory(&server.user, None, key, None) {
            eprintln!("[connect_session] pubkey auth succeeded");
            authenticated = true;
        } else {
            eprintln!("[connect_session] pubkey auth failed, falling back");
        }
    }
    if !authenticated {
        if let Some(_password) = &server.password {
            eprintln!(
                "[connect_session] trying password auth for user={}",
                server.user
            );
            session
                .userauth_password(&server.user, _password)
                .map_err(|_| AppError::new(ErrorCode::AuthenticationFailed))?;
            eprintln!("[connect_session] password auth succeeded");
        } else if !authenticated {
            return Err(AppError::new(ErrorCode::AuthenticationFailed));
        }
    }

    if !session.authenticated() {
        return Err(AppError::new(ErrorCode::AuthenticationFailed));
    }
    Ok(session)
}

/// List service-exchange.log* files in the remote log directory.
/// Uses SFTP only — no shell/PTY commands are executed.
pub fn list_remote_logs(
    server: &SshServerConfig,
    relative_path: &str,
) -> Result<Vec<RemoteFile>, AppError> {
    let session = connect_session(server)?;
    let sftp = session
        .sftp()
        .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;

    let full_path = Path::new(&server.app_root).join(relative_path);
    eprintln!(
        "[list_remote_logs] app_root=\"{}\" relative=\"{}\" full_path=\"{}\"",
        server.app_root,
        relative_path,
        full_path.display(),
    );
    let entries = match sftp.readdir(&full_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[list_remote_logs] readdir failed: {e}");
            return Err(AppError::new(ErrorCode::FileNotFound));
        }
    };

    let mut files = Vec::new();
    for (path, stat) in entries {
        if stat.is_file() {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name.starts_with("service-exchange.log") {
                files.push(RemoteFile {
                    name,
                    path: path.to_string_lossy().into_owned(),
                    size_bytes: stat.size.unwrap_or(0),
                    modified: stat.mtime.unwrap_or(0),
                });
            }
        }
    }
    Ok(files)
}

/// Download specified log files to local_dir. Decompresses .gz files.
/// Uses SFTP only — no shell/PTY commands are executed.
pub fn download_logs(
    server: &SshServerConfig,
    relative_path: &str,
    filenames: &[String],
    local_dir: &Path,
    cancellation: &CancellationToken,
) -> Result<Vec<PathBuf>, AppError> {
    std::fs::create_dir_all(local_dir).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;

    let session = connect_session(server)?;
    let sftp = session
        .sftp()
        .map_err(|_| AppError::new(ErrorCode::NetworkFailed))?;

    let full_path = Path::new(&server.app_root).join(relative_path);
    let mut downloaded = Vec::new();

    for filename in filenames {
        if cancellation.is_cancelled() {
            return Err(AppError::new(ErrorCode::Cancelled));
        }

        let remote_path = full_path.join(filename);
        let local_path = local_dir.join(filename);

        let mut remote_file = sftp
            .open(&remote_path)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        let mut buffer = Vec::new();
        remote_file
            .read_to_end(&mut buffer)
            .map_err(|_| AppError::new(ErrorCode::ParseFailed))?;
        drop(remote_file);

        std::fs::write(&local_path, &buffer).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;

        // Decompress .gz files
        if filename.ends_with(".gz") {
            let decompressed_name = filename.trim_end_matches(".gz").to_string();
            let decompressed_path = local_dir.join(&decompressed_name);
            let compressed =
                std::fs::read(&local_path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
            let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|_| AppError::new(ErrorCode::ParseFailed))?;
            std::fs::write(&decompressed_path, &decompressed)
                .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
            let _ = std::fs::remove_file(&local_path);
            downloaded.push(decompressed_path);
        } else {
            downloaded.push(local_path);
        }
    }
    Ok(downloaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_server_config() {
        let servers = vec![SshServerConfig {
            name: "test-server".into(),
            host: "192.168.1.1".into(),
            port: 22,
            user: "root".into(),
            password: Some("test123".into()),
            private_key: None,
            app_root: "/home/acrel-iot-linux".into(),
        }];
        let encrypted = serialize_servers_to_config(&servers).unwrap();
        let decrypted = parse_servers_from_config(&encrypted).unwrap();
        assert_eq!(decrypted.len(), 1);
        assert_eq!(decrypted[0].name, "test-server");
        assert_eq!(decrypted[0].host, "192.168.1.1");
    }

    #[test]
    fn empty_list_round_trips() {
        let servers: Vec<SshServerConfig> = Vec::new();
        let encrypted = serialize_servers_to_config(&servers).unwrap();
        let decrypted = parse_servers_from_config(&encrypted).unwrap();
        assert!(decrypted.is_empty());
    }
}
