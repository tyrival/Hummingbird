#[cfg(test)]
mod tests {
    use super::{
        check_outcome, install_mode_for_linux_environment, is_newer_version,
        validate_install_readiness, version_matches_expected, InstallMode, PendingUpdateCache,
        UpdateCheckOutcome, UpdateDownloadEvent, UpdateInfo, UpdateRelease,
    };

    fn release(version: &str) -> UpdateRelease {
        UpdateRelease {
            version: version.to_owned(),
            notes: Some("修复更新流程".to_owned()),
            published_at: Some("2026-07-20T08:00:00Z".to_owned()),
        }
    }

    #[test]
    fn semver_only_accepts_a_strictly_newer_release() {
        assert_eq!(is_newer_version("1.2.3", "1.2.4"), Ok(true));
        assert_eq!(is_newer_version("1.2.3", "1.2.3"), Ok(false));
        assert_eq!(is_newer_version("1.2.3", "1.2.2"), Ok(false));
        assert_eq!(is_newer_version("1.2.3-beta.1", "1.2.3"), Ok(true));
        assert!(is_newer_version("1.2.3", "not-a-version").is_err());
    }

    #[test]
    fn newer_release_preserves_notes_and_publication_date() {
        let outcome = check_outcome("1.2.3", Ok(Some(release("1.3.0"))), true)
            .expect("manual check should succeed");

        assert_eq!(
            outcome,
            UpdateCheckOutcome::Available {
                current_version: "1.2.3".to_owned(),
                release: release("1.3.0"),
            }
        );
    }

    #[test]
    fn equal_and_older_releases_report_no_update() {
        for version in ["1.2.3", "1.2.2"] {
            assert_eq!(
                check_outcome("1.2.3", Ok(Some(release(version))), true).unwrap(),
                UpdateCheckOutcome::NotAvailable {
                    current_version: "1.2.3".to_owned(),
                }
            );
        }
    }

    #[test]
    fn background_failures_are_silent_but_manual_failures_are_errors() {
        assert_eq!(
            check_outcome("1.2.3", Err("offline".to_owned()), false).unwrap(),
            UpdateCheckOutcome::NotAvailable {
                current_version: "1.2.3".to_owned(),
            }
        );
        let error = check_outcome("1.2.3", Err("offline".to_owned()), true)
            .expect_err("manual checks must surface a safe update error");
        assert_eq!(
            serde_json::to_value(error).unwrap()["code"],
            "update_failed"
        );
    }

    #[test]
    fn installation_is_blocked_by_active_exit_or_cleanup_state() {
        for readiness in [
            (true, false, true),
            (false, true, true),
            (false, false, false),
        ] {
            let error = validate_install_readiness(readiness.0, readiness.1, readiness.2)
                .expect_err("unsafe lifecycle state must block updates");
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "update_blocked"
            );
        }
        assert!(validate_install_readiness(false, false, true).is_ok());
    }

    #[test]
    fn linux_appimage_updates_in_app_while_deb_opens_the_release_page() {
        assert_eq!(
            install_mode_for_linux_environment(Some("/tmp/Hummingbird.AppImage")),
            InstallMode::InApp
        );
        assert_eq!(
            install_mode_for_linux_environment(None),
            InstallMode::ManualDeb
        );
    }

    #[test]
    fn update_dto_and_progress_events_use_the_frontend_contract() {
        let info = UpdateInfo {
            available: true,
            current_version: "1.2.3".to_owned(),
            version: Some("1.3.0".to_owned()),
            notes: Some("修复更新流程".to_owned()),
            published_at: Some("2026-07-20T08:00:00Z".to_owned()),
            install_mode: InstallMode::InApp,
            release_page_url: super::RELEASE_PAGE_URL.to_owned(),
        };
        assert_eq!(
            serde_json::to_value(info).unwrap(),
            serde_json::json!({
                "available": true,
                "currentVersion": "1.2.3",
                "version": "1.3.0",
                "notes": "修复更新流程",
                "publishedAt": "2026-07-20T08:00:00Z",
                "installMode": "in_app",
                "releasePageUrl": "https://github.com/tyrival/Hummingbird-Releases/releases/latest"
            })
        );
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Started {
                content_length: Some(1024),
            })
            .unwrap(),
            serde_json::json!({"type": "started", "contentLength": 1024})
        );
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Chunk { chunk_length: 256 }).unwrap(),
            serde_json::json!({"type": "chunk", "chunkLength": 256})
        );
        assert_eq!(
            serde_json::to_value(UpdateDownloadEvent::Finished).unwrap(),
            serde_json::json!({"type": "finished"})
        );
    }

    #[test]
    fn tauri_configuration_points_only_to_the_public_release_manifest() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
        assert_eq!(
            config["plugins"]["updater"]["endpoints"],
            serde_json::json!([
                "https://github.com/tyrival/Hummingbird-Releases/releases/latest/download/latest.json"
            ])
        );
        let pubkey = config["plugins"]["updater"]["pubkey"]
            .as_str()
            .expect("updater pubkey must be an inline string");
        assert_ne!(pubkey, "REPLACE_WITH_TAURI_UPDATER_PUBLIC_KEY");
        assert!(pubkey.len() > 100);
        assert!(pubkey
            .bytes()
            .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=') }));
        assert!(!pubkey.contains('\\'));
        assert!(!pubkey.contains('\n') && !pubkey.contains('\r'));
    }

    #[test]
    fn downloaded_update_version_must_exactly_match_the_version_the_user_saw() {
        assert!(version_matches_expected("1.2.3", "1.2.3").is_ok());
        for candidate in ["1.2.4", "1.2.3-beta.1", "v1.2.3", "not-semver"] {
            let error = version_matches_expected("1.2.3", candidate)
                .expect_err("different or non-canonical versions must be rejected");
            assert_eq!(
                serde_json::to_value(error).unwrap()["code"],
                "update_failed"
            );
        }
    }

    #[test]
    fn pending_payload_is_one_shot_and_mismatch_clears_stale_bytes() {
        let cache = PendingUpdateCache::default();
        cache.store("1.2.3".to_owned(), vec![1_u8, 2, 3]);

        assert!(cache.take_exact("1.2.4").is_err());
        assert!(cache.take_exact("1.2.3").is_err());

        cache.store("1.2.3".to_owned(), vec![4_u8, 5]);
        assert_eq!(cache.take_exact("1.2.3").unwrap(), vec![4, 5]);
        assert!(cache.take_exact("1.2.3").is_err());
    }
}
use crate::error::{AppError, ErrorCode};
use semver::Version;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

pub const RELEASE_PAGE_URL: &str =
    "https://github.com/tyrival/Hummingbird-Releases/releases/latest";
const UPDATE_EVENT_NAME: &str = "update-download-event";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    InApp,
    ManualDeb,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub install_mode: InstallMode,
    pub release_page_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateDownloadEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        content_length: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Chunk {
        chunk_length: usize,
    },
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateDownloadResult {
    Downloaded,
    OpenedReleasePage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateInstallResult {
    Installed,
}

struct PendingSignedUpdate {
    update: tauri_plugin_updater::Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct SignedUpdateState {
    pending: PendingUpdateCache<PendingSignedUpdate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateRelease {
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheckOutcome {
    Available {
        current_version: String,
        release: UpdateRelease,
    },
    NotAvailable {
        current_version: String,
    },
}

pub fn is_newer_version(current: &str, candidate: &str) -> Result<bool, AppError> {
    let current = Version::parse(current)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    let candidate = Version::parse(candidate)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    Ok(candidate > current)
}

pub fn version_matches_expected(expected: &str, actual: &str) -> Result<(), AppError> {
    let expected_version = Version::parse(expected)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    let actual_version = Version::parse(actual)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    if expected != expected_version.to_string()
        || actual != actual_version.to_string()
        || expected_version != actual_version
    {
        return Err(AppError::new(ErrorCode::UpdateFailed));
    }
    Ok(())
}

struct VersionedPayload<T> {
    version: String,
    payload: T,
}

pub struct PendingUpdateCache<T> {
    pending: Mutex<Option<VersionedPayload<T>>>,
}

impl<T> Default for PendingUpdateCache<T> {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
        }
    }
}

impl<T> PendingUpdateCache<T> {
    pub fn store(&self, version: String, payload: T) {
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(VersionedPayload { version, payload });
    }

    pub fn take_exact(&self, expected_version: &str) -> Result<T, AppError> {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or_else(|| AppError::new(ErrorCode::UpdateFailed))?;
        version_matches_expected(expected_version, &pending.version)?;
        Ok(pending.payload)
    }

    pub fn clear(&self) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

pub fn check_outcome(
    current_version: &str,
    result: Result<Option<UpdateRelease>, String>,
    manual: bool,
) -> Result<UpdateCheckOutcome, AppError> {
    match result {
        Ok(Some(release)) if is_newer_version(current_version, &release.version)? => {
            Ok(UpdateCheckOutcome::Available {
                current_version: current_version.to_owned(),
                release,
            })
        }
        Ok(_) => Ok(UpdateCheckOutcome::NotAvailable {
            current_version: current_version.to_owned(),
        }),
        Err(error) if manual => Err(AppError::internal(ErrorCode::UpdateFailed, error)),
        Err(_) => Ok(UpdateCheckOutcome::NotAvailable {
            current_version: current_version.to_owned(),
        }),
    }
}

pub fn validate_install_readiness(
    active: bool,
    cleanup_pending: bool,
    accepts_new_work: bool,
) -> Result<(), AppError> {
    if active || cleanup_pending || !accepts_new_work {
        Err(AppError::new(ErrorCode::UpdateBlocked))
    } else {
        Ok(())
    }
}

pub fn install_mode_for_linux_environment(appimage: Option<&str>) -> InstallMode {
    if appimage.is_some_and(|path| !path.trim().is_empty()) {
        InstallMode::InApp
    } else {
        InstallMode::ManualDeb
    }
}

fn current_install_mode() -> InstallMode {
    #[cfg(target_os = "linux")]
    {
        let appimage = std::env::var("APPIMAGE").ok();
        install_mode_for_linux_environment(appimage.as_deref())
    }
    #[cfg(not(target_os = "linux"))]
    {
        InstallMode::InApp
    }
}

impl UpdateInfo {
    fn from_outcome(outcome: UpdateCheckOutcome) -> Self {
        let install_mode = current_install_mode();
        match outcome {
            UpdateCheckOutcome::Available {
                current_version,
                release,
            } => Self {
                available: true,
                current_version,
                version: Some(release.version),
                notes: release.notes,
                published_at: release.published_at,
                install_mode,
                release_page_url: RELEASE_PAGE_URL.to_owned(),
            },
            UpdateCheckOutcome::NotAvailable { current_version } => Self {
                available: false,
                current_version,
                version: None,
                notes: None,
                published_at: None,
                install_mode,
                release_page_url: RELEASE_PAGE_URL.to_owned(),
            },
        }
    }
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle, manual: bool) -> Result<UpdateInfo, AppError> {
    let current_version = app.package_info().version.to_string();
    let checked = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    let release = checked
        .map(|update| {
            update.map(|update| UpdateRelease {
                version: update.version,
                notes: update.body,
                published_at: update.date.map(|date| date.to_string()),
            })
        })
        .map_err(|error| error.to_string());
    check_outcome(&current_version, release, manual).map(UpdateInfo::from_outcome)
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    state: State<'_, crate::commands::CommandState>,
    signed_update: State<'_, SignedUpdateState>,
    expected_version: String,
) -> Result<UpdateDownloadResult, AppError> {
    let _install_guard = state.begin_update_install()?;
    signed_update.pending.clear();

    let updater = app
        .updater()
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    let update = updater
        .check()
        .await
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?
        .ok_or_else(|| AppError::new(ErrorCode::UpdateFailed))?;
    version_matches_expected(&expected_version, &update.version)?;

    if current_install_mode() == InstallMode::ManualDeb {
        app.opener()
            .open_url(RELEASE_PAGE_URL, None::<String>)
            .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
        return Ok(UpdateDownloadResult::OpenedReleasePage);
    }

    let progress_app = app.clone();
    let mut started = false;
    let bytes = update
        .download(
            move |chunk_length, content_length| {
                if !started {
                    started = true;
                    let _ = progress_app.emit(
                        UPDATE_EVENT_NAME,
                        UpdateDownloadEvent::Started { content_length },
                    );
                }
                let _ = progress_app.emit(
                    UPDATE_EVENT_NAME,
                    UpdateDownloadEvent::Chunk { chunk_length },
                );
            },
            || {},
        )
        .await
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    app.emit(UPDATE_EVENT_NAME, UpdateDownloadEvent::Finished)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    state.validate_update_install()?;
    signed_update
        .pending
        .store(expected_version, PendingSignedUpdate { update, bytes });
    Ok(UpdateDownloadResult::Downloaded)
}

#[tauri::command]
pub fn install_downloaded_update(
    state: State<'_, crate::commands::CommandState>,
    signed_update: State<'_, SignedUpdateState>,
    expected_version: String,
) -> Result<UpdateInstallResult, AppError> {
    let _install_guard = state.begin_update_install()?;
    let pending = signed_update.pending.take_exact(&expected_version)?;
    state.validate_update_install()?;
    pending
        .update
        .install(pending.bytes)
        .map_err(|error| AppError::internal(ErrorCode::UpdateFailed, error.to_string()))?;
    Ok(UpdateInstallResult::Installed)
}

#[tauri::command]
pub async fn relaunch_app(
    app: AppHandle,
    state: State<'_, crate::commands::CommandState>,
) -> Result<(), AppError> {
    state.validate_update_install()?;
    let status = state.prepare_exit_for_update().await.map_err(|error| {
        AppError::internal(
            ErrorCode::UpdateBlocked,
            error
                .internal_detail()
                .unwrap_or("application cleanup did not complete"),
        )
    })?;
    if !status.safe_to_exit {
        return Err(AppError::new(ErrorCode::UpdateBlocked));
    }
    app.request_restart();
    Ok(())
}
