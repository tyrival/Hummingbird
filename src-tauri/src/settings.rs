use crate::error::{AppError, ErrorCode};
use chardetng::EncodingDetector;
use encoding_rs::{GB18030, UTF_16BE, UTF_16LE, WINDOWS_1252};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use tempfile::Builder;

const SCHEMA_VERSION: u32 = 1;
const MIGRATION_VERSION: u32 = 2;
const DEFAULT_BASE_URL: &str = "http://192.168.32.20:3000/v1";
const DEFAULT_MODEL: &str = "deepseek-chat";
const DEFAULT_TIMEOUT_SECONDS: u64 = 600;
const DEFAULT_MAX_TOKENS: u32 = 16_384;
const DEFAULT_OUTPUT_DIRECTORY: &str = "output";
const DEFAULT_CHUNK_MAX_CHARS: usize = 12_000;
const DEFAULT_CONTEXT_CHARS: usize = 1_500;
const MIN_CHUNK_MAX_CHARS: usize = 8_000;
const MAX_CHUNK_MAX_CHARS: usize = 60_000;
const MAX_CONTEXT_CHARS: usize = 3_000;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub migration_version: u32,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout_seconds: u64,
    pub max_tokens: u32,
    pub output_directory: String,
    pub chunk_max_chars: usize,
    pub context_chars: usize,
    pub last_input_dir: Option<String>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Settings")
            .field("schema_version", &self.schema_version)
            .field("migration_version", &self.migration_version)
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("timeout_seconds", &self.timeout_seconds)
            .field("max_tokens", &self.max_tokens)
            .field("output_directory", &self.output_directory)
            .field("chunk_max_chars", &self.chunk_max_chars)
            .field("context_chars", &self.context_chars)
            .field("last_input_dir", &self.last_input_dir)
            .finish()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            migration_version: MIGRATION_VERSION,
            base_url: DEFAULT_BASE_URL.into(),
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            max_tokens: DEFAULT_MAX_TOKENS,
            output_directory: DEFAULT_OUTPUT_DIRECTORY.into(),
            chunk_max_chars: DEFAULT_CHUNK_MAX_CHARS,
            context_chars: DEFAULT_CONTEXT_CHARS,
            last_input_dir: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), AppError> {
        let valid_url =
            self.base_url.starts_with("http://") || self.base_url.starts_with("https://");
        if self.schema_version != SCHEMA_VERSION
            || self.migration_version != MIGRATION_VERSION
            || !valid_url
            || self.model.trim().is_empty()
            || self.output_directory.trim().is_empty()
            || self.timeout_seconds == 0
            || self.max_tokens == 0
            || !(MIN_CHUNK_MAX_CHARS..=MAX_CHUNK_MAX_CHARS).contains(&self.chunk_max_chars)
            || self.context_chars > MAX_CONTEXT_CHARS
        {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        }

        Ok(())
    }
}

pub struct SettingsStore;

impl SettingsStore {
    /// Loads the versioned configuration, migrating one legacy config on first launch.
    /// Any migration error falls back to defaults so settings never block application startup.
    pub fn load_or_migrate(app: &AppHandle) -> Settings {
        let settings_dir = match app.path().app_config_dir() {
            Ok(path) => path,
            Err(_) => {
                eprintln!("warning: unable to resolve the settings directory; using defaults");
                return Settings::default();
            }
        };
        let output_base_dir = app
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| settings_dir.clone());
        let outcome = Self::load_or_migrate_from_paths(
            settings_dir.join("settings.json"),
            &output_base_dir,
            &legacy_candidate_paths(),
        );
        for warning in outcome.warnings {
            eprintln!("warning: settings migration: {warning}");
        }
        outcome.settings
    }

    pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), AppError> {
        settings.validate()?;
        let settings_dir = app
            .path()
            .app_config_dir()
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        Self::save_to_path(&settings_dir.join("settings.json"), settings)
    }

    fn load_or_migrate_from_paths(
        settings_path: PathBuf,
        output_base_dir: &Path,
        legacy_candidates: &[PathBuf],
    ) -> LoadOutcome {
        if settings_path.exists() {
            let loaded = fs::read(&settings_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Settings>(&bytes).ok())
                .and_then(|settings| migrate_existing_settings(settings).ok());
            return match loaded {
                Some((settings, migrated)) if settings.validate().is_ok() => {
                    if migrated {
                        let outcome = LoadOutcome::warning(
                            settings,
                            "migrated version 1 chunk defaults to 12000/1500",
                        );
                        if Self::save_to_path(&settings_path, &outcome.settings).is_err() {
                            return LoadOutcome::warning(
                                outcome.settings,
                                "settings migration succeeded in memory but could not be persisted",
                            );
                        }
                        outcome
                    } else {
                        LoadOutcome::new(settings)
                    }
                }
                _ => LoadOutcome::warning(
                    Settings::default(),
                    "existing settings.json is unreadable or invalid; using defaults",
                ),
            };
        }

        let mut settings = Settings::default();
        let mut warnings = Vec::new();

        if let Some((legacy_path, bytes)) =
            first_readable_candidate(legacy_candidates, &mut warnings)
        {
            match decode_legacy_config(&bytes) {
                Ok(text) => {
                    apply_legacy_values(&mut settings, &text, output_base_dir, &mut warnings)
                }
                Err(()) => warnings.push(format!(
                    "legacy config at {} could not be decoded; using defaults",
                    legacy_path.display()
                )),
            }
        }

        if settings.validate().is_err() {
            settings = Settings::default();
            warnings.push("legacy config contains invalid settings; using defaults".into());
        }

        if Self::save_to_path(&settings_path, &settings).is_err() {
            warnings.push("could not persist migrated settings; using defaults in memory".into());
        }
        LoadOutcome { settings, warnings }
    }

    fn save_to_path(settings_path: &Path, settings: &Settings) -> Result<(), AppError> {
        settings.validate()?;
        let parent = settings_path
            .parent()
            .ok_or_else(|| AppError::new(ErrorCode::SaveFailed))?;
        fs::create_dir_all(parent).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;

        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        let file_name = settings_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("settings.json");
        let mut temporary = Builder::new()
            .prefix(&format!(".{file_name}."))
            .suffix(".tmp")
            .tempfile_in(parent)
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        set_conservative_permissions(temporary.path())
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        temporary
            .persist(settings_path)
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;

        Ok(())
    }
}

fn migrate_existing_settings(mut settings: Settings) -> Result<(Settings, bool), ()> {
    if settings.schema_version != SCHEMA_VERSION {
        return Err(());
    }
    match settings.migration_version {
        MIGRATION_VERSION => Ok((settings, false)),
        1 => {
            if settings.chunk_max_chars == 30_000 {
                settings.chunk_max_chars = DEFAULT_CHUNK_MAX_CHARS;
            }
            if settings.context_chars == 3_000 {
                settings.context_chars = DEFAULT_CONTEXT_CHARS;
            }
            settings.migration_version = MIGRATION_VERSION;
            Ok((settings, true))
        }
        _ => Err(()),
    }
}

fn set_conservative_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn first_readable_candidate(
    candidates: &[PathBuf],
    warnings: &mut Vec<String>,
) -> Option<(PathBuf, Vec<u8>)> {
    for candidate in candidates {
        match fs::read(candidate) {
            Ok(bytes) => return Some((candidate.clone(), bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => warnings.push(format!(
                "legacy config at {} is not readable",
                candidate.display()
            )),
        }
    }
    None
}

fn decode_legacy_config(bytes: &[u8]) -> Result<String, ()> {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_with(UTF_16LE, &bytes[2..]);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_with(UTF_16BE, &bytes[2..]);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.trim_start_matches('\u{feff}').to_owned());
    }
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let detected = detector.guess(None, true);
    // The legacy contract supports GB18030 and CP1252, not other East-Asian
    // encodings. Restrict detection to that declared set so an ambiguous
    // CP1252 sequence cannot be silently reinterpreted as EUC-KR or similar.
    let encoding = if detected == GB18030 || detected.name() == "GBK" {
        GB18030
    } else {
        WINDOWS_1252
    };
    decode_with(encoding, bytes)
}

fn decode_with(encoding: &'static encoding_rs::Encoding, bytes: &[u8]) -> Result<String, ()> {
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        Err(())
    } else {
        Ok(text.into_owned())
    }
}

fn apply_legacy_values(
    settings: &mut Settings,
    text: &str,
    output_base_dir: &Path,
    warnings: &mut Vec<String>,
) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let value = raw_value.trim().trim_matches(['\"', '\'']);
        match key.trim() {
            "AI_API_BASE_URL" => settings.base_url = value.into(),
            "AI_API_KEY" => settings.api_key = value.into(),
            "AI_MODEL" => settings.model = value.into(),
            "AI_REQUEST_TIMEOUT" => match value.parse() {
                Ok(timeout) if timeout > 0 => settings.timeout_seconds = timeout,
                _ => warnings.push("AI_REQUEST_TIMEOUT is invalid; using default".into()),
            },
            "AI_MAX_TOKENS" => match value.parse() {
                Ok(max_tokens) if max_tokens > 0 => settings.max_tokens = max_tokens,
                _ => warnings.push("AI_MAX_TOKENS is invalid; using default".into()),
            },
            "OUTPUT_DIR" if !value.is_empty() => {
                let path = Path::new(value);
                settings.output_directory = if path.is_absolute() {
                    path.to_string_lossy().into_owned()
                } else {
                    output_base_dir.join(path).to_string_lossy().into_owned()
                };
            }
            "LAST_INPUT_DIR" if !value.is_empty() => settings.last_input_dir = Some(value.into()),
            _ => {}
        }
    }
}

fn legacy_candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        candidates
            .push(PathBuf::from(home).join("Library/Application Support/Hummingbird/config.txt"));
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        if let Ok(executable) = std::env::current_exe() {
            if let Some(parent) = executable.parent() {
                candidates.push(parent.join("config.txt"));
            }
        }
        #[cfg(target_os = "windows")]
        if let Some(app_data) = std::env::var_os("APPDATA") {
            candidates.push(PathBuf::from(app_data).join("Hummingbird/config.txt"));
        }
        #[cfg(target_os = "linux")]
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.push(home.join(".config/Hummingbird/config.txt"));
            candidates.push(home.join(".local/share/Hummingbird/config.txt"));
        }
    }

    candidates
}

struct LoadOutcome {
    settings: Settings,
    warnings: Vec<String>,
}

impl LoadOutcome {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            warnings: Vec::new(),
        }
    }

    fn warning(settings: Settings, warning: impl Into<String>) -> Self {
        Self {
            settings,
            warnings: vec![warning.into()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_legacy_config, Settings, SettingsStore};
    use std::{fs, path::Path};

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/config");

    #[test]
    fn decodes_utf8_utf8_bom_utf16_gb18030_and_cp1252_configurations() {
        let utf8 = fs::read(Path::new(FIXTURES).join("utf8-config.txt")).unwrap();
        let utf16 = fs::read(Path::new(FIXTURES).join("utf16-config.txt")).unwrap();
        let gb18030 = fs::read(Path::new(FIXTURES).join("gb18030-config.txt")).unwrap();
        let plain_utf8 = b"AI_MODEL=deepseek-chat\n";
        let cp1252 = b"AI_MODEL=caf\xe9-model\n";

        assert_eq!(
            decode_legacy_config(plain_utf8).unwrap(),
            "AI_MODEL=deepseek-chat\n"
        );
        assert!(decode_legacy_config(&utf8)
            .unwrap()
            .contains("deepseek-chat"));
        assert!(decode_legacy_config(&utf16)
            .unwrap()
            .contains("deepseek-chat"));
        assert!(decode_legacy_config(&gb18030).unwrap().contains("输出目录"));
        assert_eq!(
            decode_legacy_config(cp1252).unwrap(),
            "AI_MODEL=café-model\n"
        );
    }

    #[test]
    fn prefers_cp1252_when_its_bytes_are_also_valid_gb18030() {
        let ambiguous_cp1252 =
            fs::read(Path::new(FIXTURES).join("cp1252-ambiguous-config.txt")).unwrap();

        assert_eq!(
            decode_legacy_config(&ambiguous_cp1252).unwrap(),
            "AI_MODEL=legacy-À©\n"
        );
    }

    #[test]
    fn migrates_only_known_keys_preserves_chinese_paths_and_leaves_legacy_file_untouched() {
        let sandbox = TestSandbox::new("known-keys");
        let legacy_path = sandbox.path.join("config.txt");
        let original = "AI_API_BASE_URL=https://api.example.test/v1\nAI_API_KEY=fixture-key-not-real\nAI_MODEL=deepseek-chat\nAI_REQUEST_TIMEOUT=30\nAI_MAX_TOKENS=2048\nOUTPUT_DIR=迁移输出\nLAST_INPUT_DIR=/tmp/中文 输入\nUNTRUSTED=discard-me\n";
        fs::write(&legacy_path, original).unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(
            sandbox.path.join("settings.json"),
            &sandbox.path,
            std::slice::from_ref(&legacy_path),
        );

        assert_eq!(outcome.settings.base_url, "https://api.example.test/v1");
        assert_eq!(outcome.settings.api_key, "fixture-key-not-real");
        assert_eq!(outcome.settings.timeout_seconds, 30);
        assert_eq!(outcome.settings.max_tokens, 2048);
        assert_eq!(
            outcome.settings.output_directory,
            sandbox.path.join("迁移输出").to_string_lossy()
        );
        assert_eq!(
            outcome.settings.last_input_dir.as_deref(),
            Some("/tmp/中文 输入")
        );
        assert_eq!(outcome.settings.migration_version, 2);
        assert!(!fs::read_to_string(sandbox.path.join("settings.json"))
            .unwrap()
            .contains("UNTRUSTED"));
        assert_eq!(fs::read_to_string(&legacy_path).unwrap(), original);
    }

    #[test]
    fn uses_defaults_and_warns_when_legacy_numeric_values_are_invalid() {
        let sandbox = TestSandbox::new("invalid-numbers");
        let legacy_path = sandbox.path.join("config.txt");
        fs::write(
            &legacy_path,
            "AI_REQUEST_TIMEOUT=0\nAI_MAX_TOKENS=not-a-number\nAI_CHUNK_MAX_CHARS=7000\n",
        )
        .unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(
            sandbox.path.join("settings.json"),
            &sandbox.path,
            &[legacy_path],
        );

        assert_eq!(
            outcome.settings.timeout_seconds,
            Settings::default().timeout_seconds
        );
        assert_eq!(outcome.settings.max_tokens, Settings::default().max_tokens);
        assert!(outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("AI_REQUEST_TIMEOUT")));
        assert!(outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("AI_MAX_TOKENS")));
    }

    #[test]
    fn validates_required_fields_positive_numbers_and_chunk_bounds() {
        let mut settings = Settings::default();
        assert!(settings.validate().is_ok());

        settings.base_url.clear();
        assert!(settings.validate().is_err());
        settings.base_url = "https://api.example.test/v1".into();
        settings.timeout_seconds = 0;
        assert!(settings.validate().is_err());
        settings.timeout_seconds = 1;
        settings.max_tokens = 0;
        assert!(settings.validate().is_err());
        settings.max_tokens = 1;
        settings.chunk_max_chars = 7_999;
        assert!(settings.validate().is_err());
        settings.chunk_max_chars = 60_001;
        assert!(settings.validate().is_err());
        settings.chunk_max_chars = 8_000;
        assert!(settings.validate().is_ok());

        settings.schema_version = 0;
        assert!(settings.validate().is_err());
        settings.schema_version = 1;
        settings.migration_version = 3;
        assert!(settings.validate().is_err());
    }

    #[test]
    fn rejects_existing_settings_json_with_unsupported_versions() {
        let sandbox = TestSandbox::new("unsupported-existing-version");
        let settings_path = sandbox.path.join("settings.json");
        let unsupported = Settings {
            schema_version: 0,
            migration_version: 2,
            ..Settings::default()
        };
        fs::write(&settings_path, serde_json::to_vec(&unsupported).unwrap()).unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(settings_path, &sandbox.path, &[]);

        assert_eq!(outcome.settings, Settings::default());
        assert!(outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid")));
    }

    #[test]
    fn invalid_legacy_settings_fall_back_to_a_complete_default_and_warn() {
        let sandbox = TestSandbox::new("invalid-legacy-settings");
        let legacy_path = sandbox.path.join("config.txt");
        fs::write(
            &legacy_path,
            "AI_API_BASE_URL=not-a-url\nAI_MODEL=\nOUTPUT_DIR=custom-output\n",
        )
        .unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(
            sandbox.path.join("settings.json"),
            &sandbox.path,
            &[legacy_path],
        );

        assert_eq!(outcome.settings, Settings::default());
        assert!(outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("invalid")));
    }

    #[test]
    fn loads_existing_settings_without_reapplying_legacy_migration() {
        let sandbox = TestSandbox::new("existing-settings");
        let settings_path = sandbox.path.join("settings.json");
        let existing = Settings {
            model: "existing-model".into(),
            migration_version: 2,
            ..Settings::default()
        };
        SettingsStore::save_to_path(&settings_path, &existing).unwrap();
        fs::write(sandbox.path.join("config.txt"), "AI_MODEL=legacy-model\n").unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(
            settings_path,
            &sandbox.path,
            &[sandbox.path.join("config.txt")],
        );

        assert_eq!(outcome.settings.model, "existing-model");
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn migrates_only_the_old_chunk_defaults_in_existing_version_one_settings() {
        let sandbox = TestSandbox::new("existing-chunk-defaults");
        let settings_path = sandbox.path.join("settings.json");
        let old_defaults = Settings {
            migration_version: 1,
            chunk_max_chars: 30_000,
            context_chars: 3_000,
            ..Settings::default()
        };
        fs::write(&settings_path, serde_json::to_vec(&old_defaults).unwrap()).unwrap();

        let outcome =
            SettingsStore::load_or_migrate_from_paths(settings_path.clone(), &sandbox.path, &[]);

        assert_eq!(outcome.settings.migration_version, 2);
        assert_eq!(outcome.settings.chunk_max_chars, 12_000);
        assert_eq!(outcome.settings.context_chars, 1_500);
        assert!(outcome
            .warnings
            .iter()
            .any(|warning| warning.contains("chunk defaults")));
        let saved: Settings = serde_json::from_slice(&fs::read(settings_path).unwrap()).unwrap();
        assert_eq!(saved, outcome.settings);
    }

    #[test]
    fn preserves_custom_chunk_values_while_advancing_the_migration_version() {
        let sandbox = TestSandbox::new("existing-custom-chunks");
        let settings_path = sandbox.path.join("settings.json");
        let custom = Settings {
            migration_version: 1,
            chunk_max_chars: 20_000,
            context_chars: 2_000,
            ..Settings::default()
        };
        fs::write(&settings_path, serde_json::to_vec(&custom).unwrap()).unwrap();

        let outcome = SettingsStore::load_or_migrate_from_paths(settings_path, &sandbox.path, &[]);

        assert_eq!(outcome.settings.migration_version, 2);
        assert_eq!(outcome.settings.chunk_max_chars, 20_000);
        assert_eq!(outcome.settings.context_chars, 2_000);
    }

    #[test]
    fn saves_versioned_json_atomically_replaces_an_existing_file_and_leaks_no_api_key() {
        let sandbox = TestSandbox::new("atomic-save");
        let settings = Settings {
            api_key: "fixture-key-not-real".into(),
            ..Settings::default()
        };
        let settings_path = sandbox.path.join("settings.json");

        SettingsStore::save_to_path(&settings_path, &settings).unwrap();
        let replacement = Settings {
            model: "replacement-model".into(),
            ..settings.clone()
        };
        SettingsStore::save_to_path(&settings_path, &replacement).unwrap();

        let json = fs::read_to_string(&settings_path).unwrap();
        assert!(json.contains("\"schemaVersion\": 1"));
        assert!(json.contains("\"migrationVersion\": 2"));
        assert!(json.contains("replacement-model"));
        assert!(!format!("{settings:?}").contains("fixture-key-not-real"));
        let temporary_files = fs::read_dir(&sandbox.path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect::<Vec<_>>();
        assert!(
            temporary_files.is_empty(),
            "leftover temp files: {temporary_files:?}"
        );
    }

    struct TestSandbox {
        path: std::path::PathBuf,
    }

    impl TestSandbox {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "hummingbird-settings-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestSandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
