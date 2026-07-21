use crate::{
    error::{AppError, ErrorCode},
    extraction::{validate_input, MAX_INPUT_BYTES},
    naming::{load_naming_catalog, ResourcePaths},
    output::OutputDirectoryCapability,
    settings::{Settings, SettingsStore},
    task::{CleanupTarget, EventSink, ExtractionRequest, ExtractionTaskManager, TaskStatus},
};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Notify;
use uuid::Uuid;

const TASK_EVENT_NAME: &str = "task-event";
const INPUT_EXTENSIONS: &[&str] = &["pdf", "docx", "xls", "xlsx", "csv", "doc"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedInput {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InputDropResult {
    Success { input: SelectedInput },
    Error { error: AppError },
}

pub struct CommandState {
    session: Mutex<CommandSession>,
    task_manager: ExtractionTaskManager,
    staging_root: PathBuf,
    drop_generation: AtomicU64,
    start_block_generation: AtomicU64,
    start_in_progress: AtomicBool,
    update_in_progress: AtomicBool,
    input_transition: Mutex<()>,
    input_lifecycle: InputLifecycleCoordinator,
}

#[derive(Default)]
struct InputLifecycleCoordinator {
    state: Mutex<InputLifecycleState>,
    idle: Notify,
}

#[derive(Default)]
struct InputLifecycleState {
    exit_started: bool,
    inflight: usize,
}

struct InputOperationGuard<'a> {
    coordinator: &'a InputLifecycleCoordinator,
}

struct CommandSession {
    settings: Settings,
    authorized_input: Option<AuthorizedInput>,
    authorized_absolute_outputs: HashSet<PathBuf>,
}

#[derive(Debug)]
struct AuthorizedInput {
    original_canonical: PathBuf,
    display_path: PathBuf,
    staged_path: PathBuf,
    staging_dir: PathBuf,
    selected: SelectedInput,
    cleanup_armed: bool,
}

impl AuthorizedInput {
    fn cleanup_now(&mut self) {
        if !self.cleanup_armed {
            return;
        }
        self.cleanup_armed = false;
        let _ = fs::remove_file(&self.staged_path);
        let _ = fs::remove_dir(&self.staging_dir);
    }

    fn disarm(&mut self) {
        self.cleanup_armed = false;
    }

    fn cleanup_checked(mut self) -> Vec<CleanupTarget> {
        if !self.cleanup_armed {
            return Vec::new();
        }
        self.cleanup_armed = false;
        match fs::remove_file(&self.staged_path) {
            Ok(()) => match fs::remove_dir(&self.staging_dir) {
                Ok(()) => Vec::new(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(_) => vec![CleanupTarget::StagingDirectory(self.staging_dir.clone())],
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::remove_dir(&self.staging_dir) {
                    Ok(()) => Vec::new(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(_) => vec![CleanupTarget::StagingDirectory(self.staging_dir.clone())],
                }
            }
            Err(_) => vec![CleanupTarget::StagingDirectory(self.staging_dir.clone())],
        }
    }
}

impl InputLifecycleCoordinator {
    fn begin(&self) -> Result<InputOperationGuard<'_>, AppError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.exit_started {
            return Err(AppError::new(ErrorCode::TaskActive));
        }
        state.inflight += 1;
        Ok(InputOperationGuard { coordinator: self })
    }

    fn is_exit_started(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exit_started
    }

    fn mark_exit_started(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .exit_started = true;
    }

    async fn wait_for_idle(&self) {
        self.wait_for_idle_with_hook(|| {}).await;
    }

    async fn wait_for_idle_with_hook<F>(&self, mut before_wait: F)
    where
        F: FnMut(),
    {
        let mut idle = Box::pin(self.idle.notified());
        loop {
            idle.as_mut().enable();
            if self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .inflight
                == 0
            {
                return;
            }
            before_wait();
            idle.as_mut().await;
            idle.set(self.idle.notified());
        }
    }
}

impl Drop for InputOperationGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .coordinator
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.inflight = state.inflight.saturating_sub(1);
        if state.inflight == 0 {
            self.coordinator.idle.notify_waiters();
        }
    }
}

impl Drop for AuthorizedInput {
    fn drop(&mut self) {
        self.cleanup_now();
    }
}

impl CommandState {
    pub fn new(settings: Settings, task_manager: ExtractionTaskManager) -> Self {
        let staging_root = std::env::temp_dir().join(format!(
            "hummingbird-command-state-{}",
            Uuid::new_v4().simple()
        ));
        Self::new_with_staging(settings, task_manager, staging_root)
    }

    fn new_with_staging(
        settings: Settings,
        task_manager: ExtractionTaskManager,
        staging_root: PathBuf,
    ) -> Self {
        let mut authorized_absolute_outputs = HashSet::new();
        let configured_output = Path::new(&settings.output_directory);
        if configured_output.is_absolute() {
            let _ = fs::create_dir_all(configured_output);
            if let Ok(canonical) = canonical_directory(configured_output) {
                authorized_absolute_outputs.insert(canonical);
            }
        }
        Self {
            session: Mutex::new(CommandSession {
                settings,
                authorized_input: None,
                authorized_absolute_outputs,
            }),
            task_manager,
            staging_root,
            drop_generation: AtomicU64::new(0),
            start_block_generation: AtomicU64::new(0),
            start_in_progress: AtomicBool::new(false),
            update_in_progress: AtomicBool::new(false),
            input_transition: Mutex::new(()),
            input_lifecycle: InputLifecycleCoordinator::default(),
        }
    }

    pub fn production(settings: Settings, staging_root: PathBuf) -> Self {
        cleanup_stale_staging(&staging_root);
        Self::new_with_staging(settings, ExtractionTaskManager::new(), staging_root)
    }

    fn session(&self) -> MutexGuard<'_, CommandSession> {
        self.session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn settings(&self) -> Settings {
        self.session().settings.clone()
    }

    fn task_manager(&self) -> &ExtractionTaskManager {
        &self.task_manager
    }

    pub(crate) fn validate_update_install(&self) -> Result<(), AppError> {
        let status = self.task_manager.status();
        crate::updater::validate_install_readiness(
            status.active,
            status.cleanup_pending,
            self.task_manager.accepts_new_input() && !self.input_lifecycle.is_exit_started(),
        )
    }

    pub(crate) fn begin_update_install(&self) -> Result<UpdateInstallGuard<'_>, AppError> {
        let _transition = self
            .input_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.validate_update_install()?;
        if self.start_in_progress.load(Ordering::SeqCst)
            || self
                .update_in_progress
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return Err(AppError::new(ErrorCode::UpdateBlocked));
        }
        Ok(UpdateInstallGuard { state: self })
    }

    fn save_settings_transaction<F>(
        &self,
        app_data_dir: &Path,
        settings: Settings,
        persist: F,
    ) -> Result<Settings, AppError>
    where
        F: FnOnce(&Settings) -> Result<(), AppError>,
    {
        let mut session = self.session();
        validate_settings_output(&settings, &session, app_data_dir)?;
        persist(&settings)?;
        session.settings = settings.clone();
        Ok(settings)
    }

    #[cfg(test)]
    pub(crate) fn authorize_os_dropped_paths(
        &self,
        paths: &[PathBuf],
    ) -> Result<SelectedInput, AppError> {
        let generation = self.next_drop_generation();
        self.authorize_os_dropped_paths_if_current(paths, generation)
            .unwrap_or_else(|| Err(AppError::new(ErrorCode::Cancelled)))
    }

    pub(crate) fn next_drop_generation(&self) -> u64 {
        self.drop_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub(crate) fn authorize_os_dropped_paths_if_current(
        &self,
        paths: &[PathBuf],
        generation: u64,
    ) -> Option<Result<SelectedInput, AppError>> {
        let _operation = match self.begin_input_operation() {
            Ok(operation) => operation,
            Err(error) => {
                return self
                    .is_drop_generation_current(generation)
                    .then_some(Err(error));
            }
        };
        let input_blocked =
            self.start_in_progress.load(Ordering::SeqCst) || !self.task_manager.accepts_new_input();
        if let Err(error) = stage_drop_when_idle(input_blocked, || Ok(())) {
            return self
                .is_drop_generation_current(generation)
                .then_some(Err(error));
        }
        let [path] = paths else {
            return self
                .is_drop_generation_current(generation)
                .then(|| Err(AppError::new(ErrorCode::UnsupportedFormat)));
        };
        let snapshot = match stage_input_snapshot(path, path.clone(), &self.staging_root) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return self
                    .is_drop_generation_current(generation)
                    .then_some(Err(error));
            }
        };
        self.commit_drop_snapshot_if_current(generation, snapshot)
    }

    fn commit_drop_snapshot_if_current(
        &self,
        generation: u64,
        snapshot: AuthorizedInput,
    ) -> Option<Result<SelectedInput, AppError>> {
        let _transition = self
            .input_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut session = self.session();
        let blocked_by_start = generation < self.start_block_generation.load(Ordering::SeqCst);
        if self.start_in_progress.load(Ordering::SeqCst)
            || self.input_lifecycle.is_exit_started()
            || blocked_by_start
            || !self.task_manager.accepts_new_input()
        {
            return self
                .is_drop_generation_current(generation)
                .then(|| Err(AppError::new(ErrorCode::TaskActive)))
                .or_else(|| blocked_by_start.then(|| Err(AppError::new(ErrorCode::TaskActive))));
        }
        if !self.is_drop_generation_current(generation) {
            return None;
        }
        let _old = session.authorized_input.replace(snapshot);
        Some(Ok(session
            .authorized_input
            .as_ref()
            .expect("authorized input was inserted")
            .selected
            .clone()))
    }

    fn is_drop_generation_current(&self, generation: u64) -> bool {
        self.drop_generation.load(Ordering::SeqCst) == generation
    }

    fn begin_task_start(&self) -> Result<TaskStartGuard<'_>, AppError> {
        let transition = self
            .input_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.input_lifecycle.is_exit_started() {
            return Err(AppError::new(ErrorCode::TaskActive));
        }
        if self.update_in_progress.load(Ordering::SeqCst) {
            return Err(AppError::new(ErrorCode::TaskActive));
        }
        self.start_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| AppError::new(ErrorCode::TaskActive))?;
        let generation = self.next_drop_generation();
        self.start_block_generation
            .store(generation, Ordering::SeqCst);
        Ok(TaskStartGuard {
            state: self,
            _transition: transition,
        })
    }

    fn begin_input_operation(&self) -> Result<InputOperationGuard<'_>, AppError> {
        self.input_lifecycle.begin()
    }

    async fn begin_exit_and_wait_for_inputs(&self) -> Result<(), AppError> {
        let _transition = self
            .input_transition
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.update_in_progress.load(Ordering::SeqCst) {
            return Err(AppError::new(ErrorCode::UpdateBlocked));
        }
        self.input_lifecycle.mark_exit_started();
        drop(_transition);
        self.input_lifecycle.wait_for_idle().await;
        Ok(())
    }

    async fn prepare_exit_inner(&self) -> Result<TaskStatus, AppError> {
        self.begin_exit_and_wait_for_inputs().await?;
        let authorized = self.session().authorized_input.take();
        if let Some(authorized) = authorized {
            for target in authorized.cleanup_checked() {
                self.task_manager.register_pending_cleanup(target);
            }
        }
        self.task_manager.prepare_exit().await
    }

    pub(crate) async fn prepare_exit_for_update(&self) -> Result<TaskStatus, AppError> {
        self.prepare_exit_inner().await
    }

    fn authorize_selected_input_transaction<F>(
        &self,
        path: &Path,
        persist: F,
    ) -> Result<SelectedInput, AppError>
    where
        F: FnOnce(&Settings) -> Result<(), AppError>,
    {
        let _operation = self.begin_input_operation()?;
        let mut snapshot = stage_input_snapshot(path, path.to_path_buf(), &self.staging_root)?;
        let parent = snapshot
            .original_canonical
            .parent()
            .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?;
        let mut session = self.session();
        let mut updated = session.settings.clone();
        updated.last_input_dir = Some(parent.to_string_lossy().into_owned());
        updated.validate()?;
        if let Err(error) = persist(&updated) {
            snapshot.cleanup_now();
            return Err(error);
        }
        session.settings = updated;
        let _old = session.authorized_input.replace(snapshot);
        let selected = session
            .authorized_input
            .as_ref()
            .expect("authorized input was inserted")
            .selected
            .clone();
        drop(session);
        Ok(selected)
    }

    #[cfg(test)]
    fn consume_authorized_input(&self, path: &Path) -> Result<AuthorizedInput, AppError> {
        let mut session = self.session();
        take_authorized_input(&mut session, path)
    }

    fn authorize_output_directory(&self, path: &Path) -> Result<PathBuf, AppError> {
        let canonical = canonical_directory(path)?;
        self.session()
            .authorized_absolute_outputs
            .insert(canonical.clone());
        Ok(canonical)
    }

    #[cfg(test)]
    fn resolve_output_for_task(&self, app_data_dir: &Path) -> Result<PathBuf, AppError> {
        let mut session = self.session();
        resolve_authorized_output(&mut session, app_data_dir, true)
    }

    fn resolve_existing_output(&self, app_data_dir: &Path) -> Result<PathBuf, AppError> {
        let mut session = self.session();
        resolve_authorized_output(&mut session, app_data_dir, true)
    }
}

struct TaskStartGuard<'a> {
    state: &'a CommandState,
    _transition: MutexGuard<'a, ()>,
}

pub(crate) struct UpdateInstallGuard<'a> {
    state: &'a CommandState,
}

impl Drop for UpdateInstallGuard<'_> {
    fn drop(&mut self) {
        self.state.update_in_progress.store(false, Ordering::SeqCst);
    }
}

impl Drop for TaskStartGuard<'_> {
    fn drop(&mut self) {
        self.state.start_in_progress.store(false, Ordering::SeqCst);
    }
}

impl Drop for CommandState {
    fn drop(&mut self) {
        let session = self
            .session
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        drop(session.authorized_input.take());
        let _ = fs::remove_dir_all(&self.staging_root);
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, CommandState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, CommandState>,
    settings: Settings,
) -> Result<Settings, AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    state.save_settings_transaction(&app_data_dir, settings, |validated| {
        SettingsStore::save(&app, validated)
    })
}

#[tauri::command]
pub async fn select_input_file(
    app: AppHandle,
    state: State<'_, CommandState>,
) -> Result<Option<SelectedInput>, AppError> {
    let _operation = state.begin_input_operation()?;
    let current = state.settings();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("选择设备说明书或寄存器表")
        .add_filter("支持的文档", INPUT_EXTENSIONS);
    if let Some(directory) = current
        .last_input_dir
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_dir())
    {
        dialog = dialog.set_directory(directory);
    }

    let Some(selected) = dialog.blocking_pick_file() else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let selected = state
        .authorize_selected_input_transaction(&path, |next| SettingsStore::save(&app, next))?;
    Ok(Some(selected))
}

#[tauri::command]
pub async fn select_output_directory(
    app: AppHandle,
    state: State<'_, CommandState>,
) -> Result<Option<String>, AppError> {
    let current = state.settings();
    let base = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let configured_output = Path::new(&current.output_directory);
    let current_output = if configured_output.is_absolute() {
        configured_output.to_path_buf()
    } else if validate_relative_output(configured_output).is_ok() {
        base.join(configured_output)
    } else {
        base
    };
    let mut dialog = app
        .dialog()
        .file()
        .set_title("选择 CSV 输出目录")
        .set_can_create_directories(true);
    if current_output.is_dir() {
        dialog = dialog.set_directory(current_output);
    }

    let Some(selected) = dialog.blocking_pick_folder() else {
        return Ok(None);
    };
    let path = selected
        .into_path()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let canonical = state.authorize_output_directory(&path)?;
    Ok(Some(canonical.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn start_extraction(
    app: AppHandle,
    state: State<'_, CommandState>,
    input_path: String,
) -> Result<Uuid, AppError> {
    let _start_guard = state.begin_task_start()?;
    let requested_input = PathBuf::from(input_path);
    let mut authorized = {
        let mut session = state.session();
        take_authorized_input(&mut session, &requested_input)?
    };
    if state.task_manager().is_active() {
        return Err(AppError::new(ErrorCode::TaskActive));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    let resources = ResourcePaths::bundled(&app)?;
    let catalog = load_naming_catalog(&resources)?;
    let event_app = app.clone();
    let events: EventSink = Arc::new(move |event| {
        let _ = event_app.emit(TASK_EVENT_NAME, event);
    });

    let (mut settings, authorized_output) = {
        let mut session = state.session();
        let settings = session.settings.clone();
        let output = resolve_authorized_output(&mut session, &app_data_dir, true)?;
        (settings, output)
    };
    let output_capability = OutputDirectoryCapability::open(authorized_output.clone())?;
    settings.output_directory = authorized_output.to_string_lossy().into_owned();
    let result = state.task_manager().start(
        ExtractionRequest {
            input_path: authorized.staged_path.clone(),
            output_name_source: Some(authorized.original_canonical.clone()),
            staged_input_dir: Some(authorized.staging_dir.clone()),
            output_directory_capability: Some(output_capability),
            settings,
            catalog,
        },
        events,
    );
    if result.is_ok() {
        authorized.disarm();
    }
    result
}

#[tauri::command]
pub fn cancel_extraction(state: State<'_, CommandState>) -> Result<(), AppError> {
    cancel_task(state.task_manager())
}

#[tauri::command]
pub async fn cancel_extraction_and_wait(
    state: State<'_, CommandState>,
) -> Result<TaskStatus, AppError> {
    state.task_manager().cancel_and_wait().await
}

#[tauri::command]
pub async fn prepare_exit(state: State<'_, CommandState>) -> Result<TaskStatus, AppError> {
    state.prepare_exit_inner().await
}

#[tauri::command]
pub fn get_task_status(state: State<'_, CommandState>) -> TaskStatus {
    state.task_manager().status()
}

#[tauri::command]
pub fn open_output_directory(
    app: AppHandle,
    state: State<'_, CommandState>,
) -> Result<(), AppError> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let output = state.resolve_existing_output(&app_data_dir)?;
    open_existing_directory_with(&output, |directory| {
        app.opener()
            .open_path(directory.to_string_lossy().into_owned(), None::<String>)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))
    })
}

#[tauri::command]
pub fn open_task_output_directory(
    app: AppHandle,
    state: State<'_, CommandState>,
    output_path: String,
) -> Result<(), AppError> {
    let parent = state
        .task_manager()
        .completed_output_parent(Path::new(&output_path))?;
    open_existing_directory_with(&parent, |directory| {
        app.opener()
            .open_path(directory.to_string_lossy().into_owned(), None::<String>)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))
    })
}

#[tauri::command]
pub fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[cfg(test)]
fn save_validated_settings_with<F>(settings: &Settings, persist: F) -> Result<(), AppError>
where
    F: FnOnce(&Settings) -> Result<(), AppError>,
{
    settings.validate()?;
    persist(settings)
}

fn validated_input(path: &Path) -> Result<(PathBuf, SelectedInput), AppError> {
    let canonical = fs::canonicalize(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let metadata = fs::metadata(&canonical).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    validate_input(&canonical, &metadata)?;
    let file_name = canonical
        .file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?;
    Ok((
        canonical.clone(),
        SelectedInput {
            path: canonical.to_string_lossy().into_owned(),
            file_name,
            size_bytes: metadata.len(),
        },
    ))
}

fn stage_input_snapshot(
    source_path: &Path,
    display_path: PathBuf,
    staging_root: &Path,
) -> Result<AuthorizedInput, AppError> {
    let (canonical, mut selected) = validated_input(source_path)?;
    fs::create_dir_all(staging_root).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    set_private_directory_permissions(staging_root)?;
    let staging_dir = staging_root.join(Uuid::new_v4().simple().to_string());
    fs::create_dir(&staging_dir).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    set_private_directory_permissions(&staging_dir)?;
    let file_name = canonical
        .file_name()
        .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?;
    let staged_path = staging_dir.join(file_name);

    let copied = (|| {
        let mut source_options = OpenOptions::new();
        source_options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            source_options.custom_flags(libc::O_NOFOLLOW);
        }
        let source = source_options
            .open(&canonical)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        let source_metadata = source
            .metadata()
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        validate_input(&canonical, &source_metadata)?;
        selected.size_bytes = source_metadata.len();

        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        set_private_file_permissions(&staged_path)?;
        let copied = std::io::copy(&mut source.take(MAX_INPUT_BYTES + 1), &mut destination)
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        if copied > MAX_INPUT_BYTES {
            return Err(AppError::new(ErrorCode::FileTooLarge));
        }
        destination
            .flush()
            .and_then(|_| destination.sync_all())
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        let staged_metadata =
            fs::metadata(&staged_path).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        validate_input(&staged_path, &staged_metadata)?;
        Ok(())
    })();
    if let Err(error) = copied {
        let _ = fs::remove_file(&staged_path);
        let _ = fs::remove_dir(&staging_dir);
        return Err(error);
    }

    Ok(AuthorizedInput {
        original_canonical: canonical,
        display_path,
        staged_path,
        staging_dir,
        selected,
        cleanup_armed: true,
    })
}

fn set_private_directory_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn set_private_file_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn cleanup_stale_staging(staging_root: &Path) {
    let Ok(entries) = fs::read_dir(staging_root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_uuid_directory = name.len() == 32
            && name.bytes().all(|byte| byte.is_ascii_hexdigit())
            && entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_uuid_directory {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn open_existing_directory_with<F>(path: &Path, open: F) -> Result<(), AppError>
where
    F: FnOnce(&Path) -> Result<(), AppError>,
{
    if !path.is_dir() {
        return Err(AppError::new(ErrorCode::FileNotFound));
    }
    open(path)
}

fn cancel_task(task_manager: &ExtractionTaskManager) -> Result<(), AppError> {
    task_manager.cancel()
}

fn stage_drop_when_idle<T, F>(task_active: bool, stage: F) -> Result<T, AppError>
where
    F: FnOnce() -> Result<T, AppError>,
{
    if task_active {
        return Err(AppError::new(ErrorCode::TaskActive));
    }
    stage()
}

fn take_authorized_input(
    session: &mut CommandSession,
    requested: &Path,
) -> Result<AuthorizedInput, AppError> {
    let matches = session.authorized_input.as_ref().is_some_and(|authorized| {
        authorized.display_path == requested
            || authorized.original_canonical == requested
            || fs::canonicalize(requested).is_ok_and(|path| path == authorized.original_canonical)
    });
    if !matches {
        return Err(AppError::new(ErrorCode::FileNotFound));
    }
    session
        .authorized_input
        .take()
        .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))
}

fn validate_settings_output(
    settings: &Settings,
    session: &CommandSession,
    app_data_dir: &Path,
) -> Result<(), AppError> {
    settings.validate()?;
    let configured = Path::new(&settings.output_directory);
    if configured.is_absolute() {
        let canonical = canonical_directory(configured)?;
        if !session.authorized_absolute_outputs.contains(&canonical) {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        }
    } else {
        validate_relative_output(configured)?;
        if app_data_dir.as_os_str().is_empty() {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        }
    }
    Ok(())
}

fn resolve_authorized_output(
    session: &mut CommandSession,
    app_data_dir: &Path,
    create: bool,
) -> Result<PathBuf, AppError> {
    validate_settings_output(&session.settings, session, app_data_dir)?;
    let configured = Path::new(&session.settings.output_directory);
    if configured.is_absolute() {
        let canonical = canonical_directory(configured)?;
        if !session.authorized_absolute_outputs.contains(&canonical) {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        }
        Ok(canonical)
    } else {
        resolve_relative_output(app_data_dir, configured, create)
    }
}

fn validate_relative_output(path: &Path) -> Result<(), AppError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(AppError::new(ErrorCode::InvalidSettings));
    }
    Ok(())
}

fn resolve_relative_output(
    app_data_dir: &Path,
    relative: &Path,
    create: bool,
) -> Result<PathBuf, AppError> {
    validate_relative_output(relative)?;
    if create {
        fs::create_dir_all(app_data_dir).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
    }
    let base = canonical_directory(app_data_dir)?;
    let mut cursor = base.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        };
        let candidate = cursor.join(name);
        if !candidate.exists() {
            if !create {
                return Err(AppError::new(ErrorCode::FileNotFound));
            }
            fs::create_dir(&candidate).map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        }
        let canonical = canonical_directory(&candidate)?;
        if !canonical.starts_with(&base) {
            return Err(AppError::new(ErrorCode::InvalidSettings));
        }
        cursor = canonical;
    }
    Ok(cursor)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, AppError> {
    let canonical = fs::canonicalize(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    if !canonical.is_dir() {
        return Err(AppError::new(ErrorCode::FileNotFound));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::{
        cancel_task, open_existing_directory_with, save_validated_settings_with, CommandState,
    };
    use crate::{
        settings::Settings,
        task::{CleanupTarget, ExtractionTaskManager},
    };
    use serde_json::Value;
    use std::{
        cell::{Cell, RefCell},
        fs,
        sync::{mpsc, Arc, Mutex},
        thread,
        time::Duration,
    };
    use tempfile::tempdir;

    #[test]
    fn rejects_invalid_settings_before_attempting_persistence() {
        let invalid = Settings {
            base_url: "file:///tmp/not-an-api".into(),
            ..Settings::default()
        };
        let persistence_called = Cell::new(false);

        let error = save_validated_settings_with(&invalid, |_| {
            persistence_called.set(true);
            Ok(())
        })
        .expect_err("invalid settings must fail");

        assert!(!persistence_called.get());
        assert_eq!(error_json(error)["code"], "invalid_settings");
    }

    #[test]
    fn validates_selected_input_before_updating_last_input_directory() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let invalid = sandbox.path().join("manual.txt");
        fs::write(&invalid, b"not a supported document").expect("fixture should be written");
        let original = Settings {
            last_input_dir: Some("previous-directory".into()),
            ..Settings::default()
        };
        let persisted = RefCell::new(Vec::new());
        let state = CommandState::new(original.clone(), ExtractionTaskManager::new());

        let error = state
            .authorize_selected_input_transaction(&invalid, |settings| {
                persisted.borrow_mut().push(settings.clone());
                Ok(())
            })
            .expect_err("unsupported input must fail");

        assert!(persisted.borrow().is_empty());
        assert_eq!(
            state.settings().last_input_dir.as_deref(),
            Some("previous-directory")
        );
        assert_eq!(error_json(error)["code"], "unsupported_format");
    }

    #[test]
    fn persists_parent_only_after_selection_is_valid() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let valid = sandbox.path().join("register.csv");
        fs::write(&valid, b"address,name\n100,Ua").expect("fixture should be written");
        let original = Settings::default();
        let state = CommandState::new(original.clone(), ExtractionTaskManager::new());
        let persisted = RefCell::new(None);

        let selected = state
            .authorize_selected_input_transaction(&valid, |settings| {
                *persisted.borrow_mut() = Some(settings.clone());
                Ok(())
            })
            .expect("valid input should be accepted");
        let updated = state.settings();

        let canonical_valid = fs::canonicalize(&valid).expect("input should canonicalize");
        assert_eq!(selected.path, canonical_valid.to_string_lossy());
        assert_eq!(selected.file_name, "register.csv");
        assert_eq!(selected.size_bytes, 19);
        assert_eq!(
            updated.last_input_dir.as_deref(),
            Some(
                canonical_valid
                    .parent()
                    .expect("input should have parent")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(persisted.into_inner(), Some(updated.clone()));
        assert_eq!(original.last_input_dir, None);
    }

    #[test]
    fn opens_directories_only_and_never_forwards_a_file_to_the_opener() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let file = sandbox.path().join("result.csv");
        fs::write(&file, b"fixture").expect("fixture should be written");
        let opened = RefCell::new(Vec::new());

        let error = open_existing_directory_with(&file, |path| {
            opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        })
        .expect_err("files must not be opened by this command");

        assert!(opened.borrow().is_empty());
        assert_eq!(error_json(error)["code"], "file_not_found");

        open_existing_directory_with(sandbox.path(), |path| {
            opened.borrow_mut().push(path.to_path_buf());
            Ok(())
        })
        .expect("directory should be opened");
        assert_eq!(opened.into_inner(), vec![sandbox.path().to_path_buf()]);
    }

    #[test]
    fn first_open_creates_the_default_relative_output_but_rejects_an_unapproved_absolute_path() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());

        let resolved = state
            .resolve_existing_output(&app_data)
            .expect("first open should safely create the default relative output");
        assert_eq!(resolved, fs::canonicalize(app_data.join("output")).unwrap());
        assert!(resolved.starts_with(fs::canonicalize(&app_data).unwrap()));

        let unapproved = sandbox.path().join("unapproved-absolute");
        fs::create_dir(&unapproved).unwrap();
        state.session().settings.output_directory = unapproved.to_string_lossy().into_owned();
        let error = state
            .resolve_existing_output(&app_data)
            .expect_err("an absolute path must still require explicit authorization");
        assert_eq!(error_json(error)["code"], "invalid_settings");
    }

    #[test]
    fn rejects_active_task_drop_before_running_the_staging_operation() {
        let staging_called = Cell::new(false);

        let error = super::stage_drop_when_idle(true, || {
            staging_called.set(true);
            Ok(())
        })
        .expect_err("active task must reject a new OS drop");

        assert!(!staging_called.get());
        assert_eq!(error_json(error)["code"], "task_active");
    }

    #[test]
    fn task_command_errors_keep_the_public_safe_dto_contract() {
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());

        let error = cancel_task(state.task_manager()).expect_err("idle cancellation must fail");

        assert_eq!(
            error_json(error),
            serde_json::json!({
                "code": "no_active_task",
                "message": "当前没有活动任务。",
                "detail": null
            })
        );
    }

    #[test]
    fn selected_input_serializes_with_the_frontend_field_contract() {
        let selected = super::SelectedInput {
            path: "/tmp/register.csv".into(),
            file_name: "register.csv".into(),
            size_bytes: 42,
        };

        assert_eq!(
            serde_json::to_value(selected).expect("selected input should serialize"),
            serde_json::json!({
                "path": "/tmp/register.csv",
                "fileName": "register.csv",
                "sizeBytes": 42
            })
        );
    }

    #[test]
    fn rejects_valid_but_unapproved_inputs_and_consumes_explicit_drop_authorization_once() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let input = sandbox.path().join("register.csv");
        fs::write(&input, b"address,name\n100,Ua").expect("fixture should be written");
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());

        let error = state
            .consume_authorized_input(&input)
            .expect_err("an arbitrary frontend path must be rejected");
        assert_eq!(error_json(error)["code"], "file_not_found");

        let selected = state
            .authorize_os_dropped_paths(std::slice::from_ref(&input))
            .expect("an explicit drop should authorize a valid file");
        fs::write(&input, b"address,name\n999,Changed").unwrap();
        let canonical = fs::canonicalize(&input).expect("input should canonicalize");
        assert_eq!(selected.path, canonical.to_string_lossy());
        let mut snapshot = state
            .consume_authorized_input(&input)
            .expect("the authorized input should be consumed");
        assert_eq!(snapshot.original_canonical, canonical);
        assert_eq!(
            fs::read(&snapshot.staged_path).unwrap(),
            b"address,name\n100,Ua"
        );
        snapshot.cleanup_now();
        assert!(state.consume_authorized_input(&input).is_err());
    }

    #[test]
    fn replacing_a_drop_authorization_removes_the_previous_staged_snapshot() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let first = sandbox.path().join("first.csv");
        let second = sandbox.path().join("second.csv");
        fs::write(&first, b"address,name\n100,Ua").unwrap();
        fs::write(&second, b"address,name\n101,Ub").unwrap();
        let staging = sandbox.path().join("staging");
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            staging,
        );

        state
            .authorize_os_dropped_paths(std::slice::from_ref(&first))
            .unwrap();
        let first_staged = state
            .session()
            .authorized_input
            .as_ref()
            .unwrap()
            .staged_path
            .clone();
        assert!(first_staged.exists());

        state
            .authorize_os_dropped_paths(std::slice::from_ref(&second))
            .unwrap();
        assert!(!first_staged.exists());
        let mut second_snapshot = state.consume_authorized_input(&second).unwrap();
        assert!(second_snapshot.staged_path.exists());
        second_snapshot.cleanup_now();
    }

    #[test]
    fn slower_older_drop_cannot_overwrite_a_newer_completed_drop() {
        let sandbox = tempdir().unwrap();
        let first = sandbox.path().join("slow-a.csv");
        let second = sandbox.path().join("fast-b.csv");
        fs::write(&first, b"address,name\n100,Ua").unwrap();
        fs::write(&second, b"address,name\n101,Ub").unwrap();
        let staging = sandbox.path().join("staging");
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            staging.clone(),
        );

        let first_generation = state.next_drop_generation();
        let first_snapshot = super::stage_input_snapshot(&first, first.clone(), &staging).unwrap();
        let first_staged = first_snapshot.staged_path.clone();
        let second_generation = state.next_drop_generation();
        let second_snapshot =
            super::stage_input_snapshot(&second, second.clone(), &staging).unwrap();

        assert!(state
            .commit_drop_snapshot_if_current(second_generation, second_snapshot)
            .is_some());
        assert!(state
            .commit_drop_snapshot_if_current(first_generation, first_snapshot)
            .is_none());

        assert!(!first_staged.exists());
        let authorized = state
            .session()
            .authorized_input
            .as_ref()
            .unwrap()
            .display_path
            .clone();
        assert_eq!(authorized, second);
    }

    #[test]
    fn drop_copied_before_task_start_is_rejected_at_commit_and_snapshot_is_cleaned() {
        let sandbox = tempdir().unwrap();
        let input = sandbox.path().join("slow-copy.csv");
        fs::write(&input, b"address,name\n100,Ua").unwrap();
        let staging = sandbox.path().join("staging");
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            staging.clone(),
        );
        let generation = state.next_drop_generation();
        let snapshot = super::stage_input_snapshot(&input, input.clone(), &staging).unwrap();
        let staged = snapshot.staged_path.clone();

        {
            let _start = state.begin_task_start().unwrap();
        }
        let result = state
            .commit_drop_snapshot_if_current(generation, snapshot)
            .expect("start invalidation must produce a safe error event");

        assert_eq!(error_json(result.unwrap_err())["code"], "task_active");
        assert!(!staged.exists());
        assert!(state.session().authorized_input.is_none());
    }

    #[test]
    fn authorized_snapshot_raii_cleans_on_a_start_early_return() {
        let sandbox = tempdir().unwrap();
        let input = sandbox.path().join("manual.csv");
        fs::write(&input, b"address,name\n100,Ua").unwrap();
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            sandbox.path().join("staging"),
        );
        state
            .authorize_os_dropped_paths(std::slice::from_ref(&input))
            .unwrap();
        let snapshot = state.consume_authorized_input(&input).unwrap();
        let staged = snapshot.staged_path.clone();
        assert!(staged.exists());
        drop(snapshot);
        assert!(!staged.exists());
    }

    #[tokio::test]
    async fn prepare_exit_waits_for_an_inflight_drop_before_reporting_safe() {
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let operation = state
            .begin_input_operation()
            .expect("drop should begin while the application is open");
        let mut exit = Box::pin(state.prepare_exit_inner());

        assert!(tokio::time::timeout(Duration::from_millis(20), &mut exit)
            .await
            .is_err());
        drop(operation);

        let status = tokio::time::timeout(Duration::from_secs(1), exit)
            .await
            .expect("exit should resume after the drop operation finishes")
            .expect("idle exit should be safe");
        assert!(status.safe_to_exit);
    }

    #[tokio::test]
    async fn input_idle_notification_is_not_lost_between_condition_check_and_await() {
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let operation = RefCell::new(Some(state.begin_input_operation().unwrap()));

        tokio::time::timeout(
            Duration::from_secs(1),
            state.input_lifecycle.wait_for_idle_with_hook(|| {
                drop(operation.borrow_mut().take());
            }),
        )
        .await
        .expect("a notification in the check/await window must wake the waiter");
    }

    #[test]
    fn staged_file_cleanup_failure_falls_back_to_the_whole_staging_directory() {
        let sandbox = tempdir().unwrap();
        let input = sandbox.path().join("manual.csv");
        fs::write(&input, b"address,name\n100,Ua").unwrap();
        let staging = sandbox.path().join("staging");
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            staging,
        );
        state
            .authorize_selected_input_transaction(&input, |_| Ok(()))
            .unwrap();
        let authorized = state.session().authorized_input.take().unwrap();
        let staged_path = authorized.staged_path.clone();
        let staging_dir = authorized.staging_dir.clone();
        fs::remove_file(&staged_path).unwrap();
        fs::create_dir(&staged_path).unwrap();

        assert_eq!(
            authorized.cleanup_checked(),
            vec![CleanupTarget::StagingDirectory(staging_dir)]
        );
    }

    #[tokio::test]
    async fn prepare_exit_retries_cleanup_of_an_idle_selected_snapshot() {
        let sandbox = tempdir().unwrap();
        let input = sandbox.path().join("manual.csv");
        fs::write(&input, b"address,name\n100,Ua").unwrap();
        let state = CommandState::new_with_staging(
            Settings::default(),
            ExtractionTaskManager::new(),
            sandbox.path().join("staging"),
        );
        state
            .authorize_selected_input_transaction(&input, |_| Ok(()))
            .unwrap();
        let blocker = sandbox.path().join("cleanup-blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let staged_path = {
            let mut session = state.session();
            let authorized = session.authorized_input.as_mut().unwrap();
            authorized.staging_dir = blocker.clone();
            authorized.staged_path.clone()
        };

        let error = state
            .prepare_exit_inner()
            .await
            .expect_err("an unremovable selected snapshot must block exit");
        assert_eq!(error_json(error)["code"], "save_failed");
        assert!(state.task_manager().status().cleanup_pending);
        assert!(!staged_path.exists());

        fs::remove_file(&blocker).unwrap();
        fs::create_dir(&blocker).unwrap();
        let status = state.prepare_exit_inner().await.unwrap();
        assert!(status.safe_to_exit);
        assert!(!status.cleanup_pending);
        assert!(!blocker.exists());
    }

    #[tokio::test]
    async fn native_picker_input_is_rejected_after_exit_has_started() {
        let sandbox = tempdir().unwrap();
        let input = sandbox.path().join("manual.csv");
        fs::write(&input, b"address,name\n100,Ua").unwrap();
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        state.begin_exit_and_wait_for_inputs().await.unwrap();

        let error = state
            .authorize_selected_input_transaction(&input, |_| Ok(()))
            .expect_err("picker staging must be rejected after exit starts");
        assert_eq!(error_json(error)["code"], "task_active");
    }

    #[tokio::test]
    async fn update_install_readiness_tracks_cleanup_and_exit_lifecycle() {
        let manager = ExtractionTaskManager::new();
        let state = CommandState::new(Settings::default(), manager.clone());
        assert!(state.validate_update_install().is_ok());

        manager.register_pending_cleanup(CleanupTarget::File(std::path::PathBuf::from(
            "pending-update-cleanup",
        )));
        assert_eq!(
            error_json(state.validate_update_install().unwrap_err())["code"],
            "update_blocked"
        );

        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        state.begin_exit_and_wait_for_inputs().await.unwrap();
        assert_eq!(
            error_json(state.validate_update_install().unwrap_err())["code"],
            "update_blocked"
        );
        let Err(error) = state.begin_update_install() else {
            panic!("an updater operation must not begin after exit owns the lifecycle gate");
        };
        assert_eq!(error_json(error)["code"], "update_blocked");
    }

    #[tokio::test]
    async fn prepare_exit_is_blocked_without_marking_exit_while_update_is_in_progress() {
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let update = state.begin_update_install().unwrap();

        let error = state
            .prepare_exit_inner()
            .await
            .expect_err("exit must not race an updater operation");
        assert_eq!(error_json(error)["code"], "update_blocked");
        assert!(!state.input_lifecycle.is_exit_started());

        drop(update);
        assert!(state.begin_input_operation().is_ok());
    }

    #[test]
    fn update_install_guard_prevents_a_new_task_start_until_it_is_dropped() {
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let update = state
            .begin_update_install()
            .expect("idle application should allow updater installation");

        let Err(error) = state.begin_update_install() else {
            panic!("a second updater operation must be rejected");
        };
        assert_eq!(error_json(error)["code"], "update_blocked");

        let Err(error) = state.begin_task_start() else {
            panic!("a task must not begin while updater installation is active");
        };
        assert_eq!(error_json(error)["code"], "task_active");
        drop(update);
        assert!(state.begin_task_start().is_ok());
    }

    #[test]
    fn startup_cleanup_removes_only_uuid_staging_directories() {
        let sandbox = tempdir().unwrap();
        let staging = sandbox.path().join("input-staging");
        let stale = staging.join("11111111222243338444555555555555");
        let unrelated = staging.join("keep-me");
        fs::create_dir_all(&stale).unwrap();
        fs::create_dir(&unrelated).unwrap();
        fs::write(stale.join("manual.csv"), b"stale").unwrap();
        fs::write(unrelated.join("note"), b"keep").unwrap();

        super::cleanup_stale_staging(&staging);

        assert!(!stale.exists());
        assert!(unrelated.exists());
    }

    #[test]
    fn rejects_parent_traversal_and_unselected_absolute_output_before_persistence() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        fs::create_dir(&app_data).expect("app data should be created");
        let arbitrary = sandbox.path().join("arbitrary");
        fs::create_dir(&arbitrary).expect("arbitrary directory should be created");
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let persistence_called = Cell::new(false);

        for output_directory in [
            "../escape".to_owned(),
            arbitrary.to_string_lossy().into_owned(),
        ] {
            let candidate = Settings {
                output_directory,
                ..Settings::default()
            };
            let error = state
                .save_settings_transaction(&app_data, candidate, |_| {
                    persistence_called.set(true);
                    Ok(())
                })
                .expect_err("untrusted output must fail");
            assert_eq!(error_json(error)["code"], "invalid_settings");
        }
        assert!(!persistence_called.get());
    }

    #[test]
    fn native_output_authorization_allows_only_the_canonical_selected_directory() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        let selected = sandbox.path().join("selected");
        fs::create_dir(&app_data).expect("app data should be created");
        fs::create_dir(&selected).expect("selected output should be created");
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let canonical = state
            .authorize_output_directory(&selected)
            .expect("native selection should authorize output");
        let candidate = Settings {
            output_directory: selected.to_string_lossy().into_owned(),
            ..Settings::default()
        };

        state
            .save_settings_transaction(&app_data, candidate, |_| Ok(()))
            .expect("authorized output should save");
        assert_eq!(
            state
                .resolve_output_for_task(&app_data)
                .expect("authorized output should resolve"),
            canonical
        );
    }

    #[test]
    fn initial_local_absolute_output_is_trusted_without_a_new_picker_round_trip() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        let existing = sandbox.path().join("existing-output");
        fs::create_dir(&app_data).expect("app data should be created");
        fs::create_dir(&existing).expect("existing output should be created");
        let state = CommandState::new(
            Settings {
                output_directory: existing.to_string_lossy().into_owned(),
                ..Settings::default()
            },
            ExtractionTaskManager::new(),
        );

        assert_eq!(
            state
                .resolve_output_for_task(&app_data)
                .expect("locally loaded absolute output should remain trusted"),
            fs::canonicalize(existing).expect("output should canonicalize")
        );
    }

    #[cfg(unix)]
    #[test]
    fn absolute_output_symlink_retarget_does_not_gain_authorization() {
        use std::os::unix::fs::symlink;

        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        let first = sandbox.path().join("first-output");
        let second = sandbox.path().join("second-output");
        let link = sandbox.path().join("selected-output");
        fs::create_dir(&app_data).unwrap();
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        symlink(&first, &link).unwrap();
        let state = CommandState::new(
            Settings {
                output_directory: link.to_string_lossy().into_owned(),
                ..Settings::default()
            },
            ExtractionTaskManager::new(),
        );

        fs::remove_file(&link).unwrap();
        symlink(&second, &link).unwrap();
        let error = state
            .resolve_output_for_task(&app_data)
            .expect_err("retargeted symlink must not inherit lexical authorization");
        assert_eq!(error_json(error)["code"], "invalid_settings");
    }

    #[cfg(unix)]
    #[test]
    fn relative_output_cannot_escape_app_data_through_a_symlink() {
        use std::os::unix::fs::symlink;

        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        let outside = sandbox.path().join("outside");
        fs::create_dir(&app_data).expect("app data should be created");
        fs::create_dir(&outside).expect("outside should be created");
        symlink(&outside, app_data.join("linked")).expect("symlink should be created");
        let state = CommandState::new(Settings::default(), ExtractionTaskManager::new());
        let candidate = Settings {
            output_directory: "linked/result".into(),
            ..Settings::default()
        };
        state
            .save_settings_transaction(&app_data, candidate, |_| Ok(()))
            .expect("lexically safe relative output may be stored");

        let error = state
            .resolve_output_for_task(&app_data)
            .expect_err("symlink escape must be rejected before task start");
        assert_eq!(error_json(error)["code"], "invalid_settings");
        assert!(!outside.join("result").exists());
    }

    #[test]
    fn settings_persistence_and_memory_replacement_are_one_serial_transaction() {
        let sandbox = tempdir().expect("temporary directory should be created");
        let app_data = sandbox.path().join("app-data");
        fs::create_dir(&app_data).expect("app data should be created");
        let state = Arc::new(CommandState::new(
            Settings::default(),
            ExtractionTaskManager::new(),
        ));
        let persisted = Arc::new(Mutex::new(Vec::new()));
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_state = Arc::clone(&state);
        let first_persisted = Arc::clone(&persisted);
        let first_app_data = app_data.clone();
        let first = thread::spawn(move || {
            let candidate = Settings {
                model: "first".into(),
                ..Settings::default()
            };
            first_state
                .save_settings_transaction(&first_app_data, candidate, |settings| {
                    first_entered_tx.send(()).unwrap();
                    release_first_rx.recv().unwrap();
                    first_persisted.lock().unwrap().push(settings.model.clone());
                    Ok(())
                })
                .unwrap();
        });
        first_entered_rx.recv().unwrap();

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second_state = Arc::clone(&state);
        let second_persisted = Arc::clone(&persisted);
        let second_app_data = app_data.clone();
        let second = thread::spawn(move || {
            let candidate = Settings {
                model: "second".into(),
                ..Settings::default()
            };
            second_state
                .save_settings_transaction(&second_app_data, candidate, |settings| {
                    second_entered_tx.send(()).unwrap();
                    second_persisted
                        .lock()
                        .unwrap()
                        .push(settings.model.clone());
                    Ok(())
                })
                .unwrap();
        });

        assert!(second_entered_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release_first_tx.send(()).unwrap();
        first.join().unwrap();
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        second.join().unwrap();

        assert_eq!(*persisted.lock().unwrap(), vec!["first", "second"]);
        assert_eq!(state.settings().model, "second");
    }

    #[test]
    fn capabilities_are_main_window_only_and_do_not_expose_broad_plugins_or_shell() {
        let capabilities: Value =
            serde_json::from_str(include_str!("../capabilities/default.json"))
                .expect("capability file should contain JSON");
        assert_eq!(capabilities["windows"], serde_json::json!(["main"]));
        let permissions = capabilities["permissions"]
            .as_array()
            .expect("permissions should be an array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert!(permissions.contains(&"dialog:allow-open"));
        assert_eq!(
            permissions
                .iter()
                .copied()
                .filter(|permission| permission.starts_with("core:window:"))
                .collect::<Vec<_>>(),
            vec!["core:window:allow-destroy"]
        );
        assert!(!permissions
            .iter()
            .any(|permission| permission.starts_with("process:")));
        assert!(!permissions
            .iter()
            .any(|permission| permission.starts_with("updater:")));
        assert!(!permissions
            .iter()
            .any(|permission| permission.contains("shell")));
        assert!(!permissions.iter().any(|permission| {
            matches!(
                *permission,
                "dialog:default" | "opener:default" | "process:default" | "updater:default"
            )
        }));
        assert!(!permissions
            .iter()
            .any(|permission| permission.starts_with("opener:")));
        let app_registration = include_str!("lib.rs");
        assert!(app_registration.contains("tauri::DragDropEvent::Drop"));
        assert!(app_registration.contains("authorize_os_dropped_paths"));
        assert!(app_registration.contains("spawn_blocking"));
        assert!(app_registration.contains("input-drop-result"));
    }

    fn error_json(error: crate::error::AppError) -> Value {
        serde_json::to_value(error).expect("AppError should serialize")
    }
}
