use crate::error::{AppError, ErrorCode};
use cap_std::{ambient_authority, fs::Dir};
use chrono::{DateTime, Local};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct OutputDirectoryCapability {
    dir: Arc<Dir>,
    canonical_path: PathBuf,
}

impl OutputDirectoryCapability {
    pub fn open(canonical_path: PathBuf) -> Result<Self, AppError> {
        let dir = Dir::open_ambient_dir(&canonical_path, ambient_authority())
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        Ok(Self {
            dir: Arc::new(dir),
            canonical_path,
        })
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

pub fn save_csv_in_directory(
    output: &OutputDirectoryCapability,
    original: &Path,
    csv: &str,
    now: DateTime<Local>,
) -> Result<PathBuf, AppError> {
    validate_capability_binding(output)?;
    let stem = safe_stem(original);
    let timestamp = now.format("%Y%m%d_%H%M%S");
    let mut bytes = Vec::with_capacity(csv.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(csv.as_bytes());
    for _ in 0..32 {
        let suffix = &Uuid::new_v4().simple().to_string()[..6];
        let file_name = format!("{stem}_{timestamp}_{suffix}.csv");
        let options = cap_std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .clone();
        match output.dir.open_with(&file_name, &options) {
            Ok(mut file) => {
                if file
                    .write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .is_err()
                {
                    drop(file);
                    let _ = output.dir.remove_file(&file_name);
                    return Err(AppError::new(ErrorCode::SaveFailed));
                }
                if validate_capability_binding(output).is_err() {
                    drop(file);
                    let _ = output.dir.remove_file(&file_name);
                    return Err(AppError::new(ErrorCode::SaveFailed));
                }
                return Ok(output.canonical_path.join(file_name));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(AppError::new(ErrorCode::SaveFailed)),
        }
    }
    Err(AppError::new(ErrorCode::SaveFailed))
}

fn validate_capability_binding(output: &OutputDirectoryCapability) -> Result<(), AppError> {
    let current = fs::canonicalize(&output.canonical_path)
        .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    if current != output.canonical_path {
        return Err(AppError::new(ErrorCode::SaveFailed));
    }
    Ok(())
}

pub fn save_csv(
    output_dir: &Path,
    original: &Path,
    csv: &str,
    now: DateTime<Local>,
) -> Result<PathBuf, AppError> {
    save_csv_with_suffixes(output_dir, original, csv, now, || {
        Uuid::new_v4().simple().to_string()[..6].to_owned()
    })
}

fn save_csv_with_suffixes<F>(
    output_dir: &Path,
    original: &Path,
    csv: &str,
    now: DateTime<Local>,
    next_suffix: F,
) -> Result<PathBuf, AppError>
where
    F: FnMut() -> String,
{
    fs::create_dir_all(output_dir).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let stem = safe_stem(original);
    let timestamp = now.format("%Y%m%d_%H%M%S");
    let mut next_suffix = next_suffix;
    let mut bytes = Vec::with_capacity(csv.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(csv.as_bytes());

    for _ in 0..32 {
        let suffix = next_suffix();
        let path = output_dir.join(format!("{stem}_{timestamp}_{suffix}.csv"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                write_created_file(&path, &bytes, file)?;
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(AppError::new(ErrorCode::SaveFailed)),
        }
    }
    Err(AppError::new(ErrorCode::SaveFailed))
}

fn write_created_file<W: Write>(path: &Path, bytes: &[u8], mut writer: W) -> Result<(), AppError> {
    let result = writer.write_all(bytes);
    drop(writer);
    if result.is_err() {
        let _ = fs::remove_file(path);
        return Err(AppError::new(ErrorCode::SaveFailed));
    }
    Ok(())
}

fn safe_stem(original: &Path) -> String {
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    let mut cleaned = stem
        .chars()
        .filter(|character| {
            !character.is_control()
                && !matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
        .collect::<String>();
    cleaned = cleaned
        .trim_matches(|character: char| character.is_whitespace() || character == '.')
        .to_owned();
    if cleaned.is_empty() || is_windows_reserved_name(&cleaned) {
        cleaned.insert(0, '_');
    }
    if cleaned == "_" {
        "output".into()
    } else {
        cleaned
    }
}

fn is_windows_reserved_name(value: &str) -> bool {
    let value = value.to_ascii_uppercase();
    matches!(value.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || value
            .strip_prefix("COM")
            .or_else(|| value.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

#[cfg(test)]
mod tests {
    use super::{
        save_csv, save_csv_in_directory, save_csv_with_suffixes, write_created_file,
        OutputDirectoryCapability,
    };
    use chrono::{Local, TimeZone};
    use std::{
        fs,
        io::{self, Write},
    };
    use tempfile::tempdir;

    #[test]
    fn saves_utf8_bom_with_a_cross_platform_safe_timestamped_filename() {
        let directory = tempdir().expect("temporary output directory");
        let now = Local
            .with_ymd_and_hms(2026, 7, 19, 12, 34, 56)
            .single()
            .expect("valid local timestamp");
        let path = save_csv(
            directory.path(),
            std::path::Path::new("原:文件?<>|*.pdf"),
            "id,group\n1,1",
            now,
        )
        .expect("CSV should save");
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("UTF-8 filename");
        let bytes = fs::read(&path).expect("output should be readable");

        assert!(
            file_name.starts_with("原文件_20260719_123456_"),
            "{file_name}"
        );
        assert!(file_name.ends_with(".csv"));
        assert!(!file_name.chars().any(|character| matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )));
        let suffix = &file_name[file_name.len() - 10..file_name.len() - 4];
        assert_eq!(suffix.len(), 6);
        assert!(suffix
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit()));
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        assert_eq!(&bytes[3..], b"id,group\n1,1");
    }

    #[test]
    fn retries_a_filename_collision_without_overwriting_the_existing_csv() {
        let directory = tempdir().expect("temporary output directory");
        let now = Local
            .with_ymd_and_hms(2026, 7, 19, 12, 34, 56)
            .single()
            .expect("valid local timestamp");
        let existing = directory.path().join("input_20260719_123456_aaaaaa.csv");
        fs::write(&existing, b"original").expect("seed collision file");
        let mut suffixes = ["aaaaaa".to_owned(), "bbbbbb".to_owned()].into_iter();

        let saved = save_csv_with_suffixes(
            directory.path(),
            std::path::Path::new("input.pdf"),
            "id,group\n1,1",
            now,
            || suffixes.next().expect("enough suffixes"),
        )
        .expect("collision retry should save");

        assert_eq!(
            existing,
            directory.path().join("input_20260719_123456_aaaaaa.csv")
        );
        assert_eq!(fs::read(&existing).expect("existing remains"), b"original");
        assert_eq!(
            saved.file_name().and_then(|name| name.to_str()),
            Some("input_20260719_123456_bbbbbb.csv")
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_capability_safely_rejects_a_retargeted_authorized_path() {
        use std::os::unix::fs::symlink;

        let sandbox = tempdir().unwrap();
        let authorized = sandbox.path().join("authorized");
        let moved = sandbox.path().join("moved-original");
        let attacker = sandbox.path().join("attacker");
        fs::create_dir(&authorized).unwrap();
        fs::create_dir(&attacker).unwrap();
        let capability = OutputDirectoryCapability::open(fs::canonicalize(&authorized).unwrap())
            .expect("directory handle should open");
        fs::rename(&authorized, &moved).unwrap();
        symlink(&attacker, &authorized).unwrap();
        let now = Local.with_ymd_and_hms(2026, 7, 19, 12, 34, 56).unwrap();

        assert!(save_csv_in_directory(
            &capability,
            std::path::Path::new("manual.pdf"),
            "id,group\n1,1",
            now,
        )
        .is_err());
        assert_eq!(fs::read_dir(&moved).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&attacker).unwrap().count(), 0);
    }

    #[test]
    fn removes_a_new_output_file_when_writing_fails_after_partial_bytes() {
        let directory = tempdir().expect("temporary output directory");
        let path = directory.path().join("partial.csv");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("partial output should be created");
        let writer = FailingWriter {
            file,
            remaining_before_failure: 4,
        };

        let error = write_created_file(&path, b"longer than four bytes", writer)
            .expect_err("synthetic write should fail");

        assert_eq!(error_code(error), "save_failed");
        assert!(!path.exists(), "failed output must not remain on disk");
    }

    struct FailingWriter {
        file: fs::File,
        remaining_before_failure: usize,
    }

    impl Write for FailingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining_before_failure == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "synthetic disk failure",
                ));
            }
            let count = bytes.len().min(self.remaining_before_failure);
            let written = self.file.write(&bytes[..count])?;
            self.remaining_before_failure -= written;
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    fn error_code(error: crate::error::AppError) -> String {
        serde_json::to_value(error).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}
