use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

use tokio_util::sync::CancellationToken;

use crate::error::{AppError, ErrorCode};
use crate::sftp_download::{RemoteFile, SshServerConfig};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyseConfig {
    pub log_analyse_dir: String,
    pub ssh_servers: Vec<SshServerConfig>,
}

impl AnalyseConfig {
    pub fn load(app_config_dir: &Path) -> Self {
        let mut config = Self {
            log_analyse_dir: default_analyse_dir().to_string_lossy().into_owned(),
            ssh_servers: Vec::new(),
        };
        let config_path = find_config_txt();
        eprintln!(
            "[AnalyseConfig::load] find_config_txt() = {:?}",
            config_path.as_ref().map(|p| p.display().to_string())
        );
        if let Some(config_path) = config_path {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, value)) = line.split_once('=') {
                        let value = value.trim().trim_matches(['"', '\'']);
                        match key.trim() {
                            "LOG_ANALYSE_DIR" if !value.is_empty() => {
                                let p = Path::new(value);
                                config.log_analyse_dir = if p.is_absolute() {
                                    p.to_string_lossy().into_owned()
                                } else {
                                    app_config_dir.join(p).to_string_lossy().into_owned()
                                };
                            }
                            "SSH_SERVERS" if !value.is_empty() => {
                                match crate::sftp_download::parse_servers_from_config(value) {
                                    Ok(servers) => {
                                        eprintln!(
                                            "[AnalyseConfig::load] loaded {} SSH servers",
                                            servers.len()
                                        );
                                        config.ssh_servers = servers;
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[AnalyseConfig::load] SSH_SERVERS parse error: {:?}",
                                            e
                                        );
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        config
    }

    pub fn save_ssh_servers(&mut self, servers: Vec<SshServerConfig>) -> Result<(), AppError> {
        self.ssh_servers = servers;
        write_config_txt_key(
            "SSH_SERVERS",
            &crate::sftp_download::serialize_servers_to_config(&self.ssh_servers)?,
        )
    }
}

fn default_analyse_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join("Hummingbird/analyse");
    }
    #[cfg(target_os = "windows")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            return parent.join("analyse");
        }
    }
    PathBuf::from("analyse")
}

fn find_config_txt() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home).join("Library/Application Support/Hummingbird/config.txt");
        if path.exists() {
            return Some(path);
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let path = parent.join("config.txt");
            if path.exists() {
                return Some(path);
            }
        }
    }
    #[cfg(target_os = "linux")]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for candidate in &[
            home.join(".config/Hummingbird/config.txt"),
            home.join(".local/share/Hummingbird/config.txt"),
        ] {
            if candidate.exists() {
                return Some(candidate.clone());
            }
        }
    }
    None
}

fn write_config_txt_key(key: &str, value: &str) -> Result<(), AppError> {
    let config_path = find_config_txt().unwrap_or_else(|| {
        #[cfg(target_os = "macos")]
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/Hummingbird/config.txt");
        }
        PathBuf::from("config.txt")
    });

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    }

    let content = if config_path.exists() {
        std::fs::read_to_string(&config_path).unwrap_or_default()
    } else {
        String::new()
    };

    let mut found = false;
    let new_lines: Vec<String> = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if let Some((k, _)) = trimmed.split_once('=') {
                if k.trim() == key {
                    found = true;
                    return format!("{key}={value}");
                }
            }
            line.to_string()
        })
        .collect();

    let new_content = if found {
        new_lines.join("\n")
    } else {
        if content.is_empty() || content.ends_with('\n') {
            format!("{content}{key}={value}\n")
        } else {
            format!("{content}\n{key}={value}\n")
        }
    };

    std::fs::write(&config_path, new_content).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    Ok(())
}

// ── Tauri Command DTOs ──────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct AnalyseStatus {
    pub active: bool,
    pub stage: Option<String>,
    pub progress_pct: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyseResult {
    pub summary: crate::log_stats::LogSummary,
    pub ai_reports: Vec<String>,
}

// ── Tauri Command handlers ───────────────────────────────────

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::settings::SettingsStore;

const ANALYSE_EVENT: &str = "analyse-event";

#[tauri::command]
pub fn get_analyse_config(app: AppHandle) -> AnalyseConfig {
    let app_config_dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    AnalyseConfig::load(&app_config_dir)
}

#[tauri::command]
pub fn save_ssh_servers(
    app: AppHandle,
    servers: Vec<SshServerConfig>,
) -> Result<Vec<SshServerConfig>, AppError> {
    let app_config_dir = app
        .path()
        .app_config_dir()
        .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let mut config = AnalyseConfig::load(&app_config_dir);
    config.save_ssh_servers(servers)?;
    Ok(config.ssh_servers)
}

#[tauri::command]
pub async fn select_analyse_dir(app: AppHandle) -> Result<String, AppError> {
    let (tx, rx) = mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.recv().ok().flatten();
    let Some(path) = folder else {
        return Err(AppError::new(ErrorCode::Cancelled));
    };
    let path_buf = path
        .into_path()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    Ok(path_buf.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn test_ssh_connection(server: SshServerConfig) -> Result<String, AppError> {
    eprintln!(
        "[test_ssh_connection] name={} host={}:{} user={} app_root=\"{}\" has_password={} has_key={}",
        server.name,
        server.host,
        server.port,
        server.user,
        server.app_root,
        server.password.is_some(),
        server.private_key.is_some(),
    );
    crate::sftp_download::list_remote_logs(&server)?;
    Ok(format!("成功连接到 {}", server.name))
}

#[tauri::command]
pub fn list_remote_logs_command(server: SshServerConfig) -> Result<Vec<RemoteFile>, AppError> {
    let mut files = crate::sftp_download::list_remote_logs(&server)?;
    files.sort_by(|a, b| b.name.cmp(&a.name)); // newest first
    Ok(files)
}

#[tauri::command]
pub async fn download_logs_command(
    app: AppHandle,
    server: SshServerConfig,
    remote_files: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let config = get_analyse_config(app.clone());
    let local_dir = PathBuf::from(&config.log_analyse_dir);
    let cancellation = CancellationToken::new();
    let downloaded =
        crate::sftp_download::download_logs(&server, &remote_files, &local_dir, &cancellation)?;
    Ok(downloaded
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub async fn start_log_analysis(
    app: AppHandle,
    state: State<'_, AnalyseAppState>,
    file_paths: Vec<String>,
) -> Result<(), AppError> {
    let paths: Vec<PathBuf> = file_paths.iter().map(PathBuf::from).collect();
    let settings = SettingsStore::load_or_migrate(&app);

    let handle = app.clone();
    let events: crate::analyse_task::AnalyseEventSink = Arc::new(move |event| {
        let _ = handle.emit(ANALYSE_EVENT, event);
    });

    state.task_manager.start(paths, settings, events)?;
    Ok(())
}

#[tauri::command]
pub fn cancel_log_analysis(state: State<'_, AnalyseAppState>) -> Result<(), AppError> {
    state.task_manager.cancel()
}

#[tauri::command]
pub fn get_analyse_status(state: State<'_, AnalyseAppState>) -> AnalyseStatus {
    AnalyseStatus {
        active: state.task_manager.is_active(),
        stage: None,
        progress_pct: 0,
        detail: String::new(),
    }
}

pub struct AnalyseAppState {
    pub task_manager: crate::analyse_task::AnalyseTaskManager,
}

#[tauri::command]
pub async fn select_log_folder(app: AppHandle) -> Result<Vec<String>, AppError> {
    let (tx, rx) = mpsc::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.recv().ok().flatten();
    let Some(path) = folder else {
        return Ok(Vec::new());
    };
    let dir = path
        .into_path()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("service-exchange")
                && (name.ends_with(".log") || name.ends_with(".gz"))
            {
                files.push(entry.path().to_string_lossy().into_owned());
            }
        }
    }
    Ok(files)
}

#[tauri::command]
pub async fn select_key_file(app: AppHandle) -> Result<String, AppError> {
    let (tx, rx) = mpsc::channel();
    app.dialog()
        .file()
        .add_filter("PEM 密钥", &["pem", "key", ""])
        .pick_file(move |file| {
            let _ = tx.send(file);
        });
    let file = rx.recv().ok().flatten();
    let Some(path) = file else {
        return Err(AppError::new(ErrorCode::Cancelled));
    };
    let path_buf = path
        .into_path()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    std::fs::read_to_string(&path_buf).map_err(|_| AppError::new(ErrorCode::FileNotFound))
}
