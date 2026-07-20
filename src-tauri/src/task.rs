use crate::{
    ai::AiClient,
    chunking::{bisect_chunk, split_document_text, ChunkPolicy},
    error::{redact_secrets, AppError, ErrorCode},
    extraction::{extract_document, validate_input},
    naming::NamingCatalog,
    output::{save_csv, save_csv_in_directory, OutputDirectoryCapability},
    prompt::build_system_prompt,
    register_csv::{merge_csv_results, sanitize_csv, SanitizedCsv},
    settings::Settings,
};
use chrono::Local;
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type EventSink = Arc<dyn Fn(TaskEvent) + Send + Sync + 'static>;

#[derive(Clone)]
pub struct ExtractionRequest {
    pub input_path: PathBuf,
    pub output_name_source: Option<PathBuf>,
    pub staged_input_dir: Option<PathBuf>,
    pub output_directory_capability: Option<OutputDirectoryCapability>,
    pub settings: Settings,
    pub catalog: NamingCatalog,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStage {
    ValidatingInput,
    ExtractingText,
    PreparingChunks,
    CallingAi,
    MergingResults,
    SavingOutput,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum TaskEvent {
    Stage {
        task_id: Uuid,
        stage: TaskStage,
    },
    Log {
        task_id: Uuid,
        level: LogLevel,
        message: String,
    },
    Progress {
        task_id: Uuid,
        completed_chunks: usize,
        total_chunks: usize,
    },
    Completed {
        task_id: Uuid,
        output_path: PathBuf,
        record_count: usize,
    },
    Cancelled {
        task_id: Uuid,
    },
    Failed {
        task_id: Uuid,
        error: AppError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub task_id: Option<Uuid>,
    pub active: bool,
    pub completed_chunks: usize,
    pub total_chunks: usize,
    pub stage: Option<TaskStage>,
    pub output_path: Option<PathBuf>,
    pub record_count: Option<usize>,
    pub error: Option<AppError>,
    pub cleanup_pending: bool,
    pub safe_to_exit: bool,
}

#[derive(Clone)]
pub struct ExtractionTaskManager {
    inner: Arc<Mutex<ManagerState>>,
    extract: ExtractFunction,
    terminal_changes: Arc<watch::Sender<u64>>,
}

type ExtractFunction = Arc<
    dyn Fn(PathBuf, crate::extraction::DocumentKind) -> Result<String, AppError>
        + Send
        + Sync
        + 'static,
>;

#[derive(Default)]
struct ManagerState {
    active: Option<ActiveTask>,
    last_status: Option<TaskStatus>,
    terminal_history: HashMap<Uuid, TaskStatus>,
    terminal_order: VecDeque<Uuid>,
    cleanup_pending: Vec<CleanupTarget>,
    exiting: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CleanupTarget {
    File(PathBuf),
    StagingDirectory(PathBuf),
}

struct ActiveTask {
    task_id: Uuid,
    cancellation: CancellationToken,
    completed_chunks: usize,
    total_chunks: usize,
    stage: TaskStage,
    worker: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl ExtractionTaskManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ManagerState::default())),
            extract: Arc::new(|path, kind| extract_document(&path, kind)),
            terminal_changes: Arc::new(watch::channel(0).0),
        }
    }

    #[cfg(test)]
    fn with_extractor(extract: ExtractFunction) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ManagerState::default())),
            extract,
            terminal_changes: Arc::new(watch::channel(0).0),
        }
    }

    pub fn start(&self, request: ExtractionRequest, events: EventSink) -> Result<Uuid, AppError> {
        request.settings.validate()?;
        let task_id = Uuid::new_v4();
        let cancellation = CancellationToken::new();
        {
            let mut state = self.lock();
            if state.active.is_some() || state.exiting {
                return Err(AppError::new(ErrorCode::TaskActive));
            }
            state.active = Some(ActiveTask {
                task_id,
                cancellation: cancellation.clone(),
                completed_chunks: 0,
                total_chunks: 0,
                stage: TaskStage::ValidatingInput,
                worker: None,
            });
            state.last_status = None;
        }

        let manager = self.clone();
        let worker_manager = manager.clone();
        let worker_events = events.clone();
        let worker_cancellation = cancellation.clone();
        let (start_sender, start_receiver) = tokio::sync::oneshot::channel();
        let handle = tauri::async_runtime::spawn(async move {
            let mut staged_cleanup = request
                .staged_input_dir
                .clone()
                .map(|directory| StagedInputCleanup::new(request.input_path.clone(), directory));
            if start_receiver.await.is_err() {
                let mut error =
                    AppError::internal(ErrorCode::ParseFailed, "task supervisor failed to start");
                if let Some(cleanup) = staged_cleanup.as_mut() {
                    if cleanup.clean_now().is_err() {
                        manager.register_pending_cleanup(CleanupTarget::StagingDirectory(
                            cleanup.directory.clone(),
                        ));
                        error = AppError::new(ErrorCode::SaveFailed);
                    }
                }
                manager.finish(task_id, Err(error), &events);
                return;
            }
            let worker = tokio::spawn(async move {
                worker_manager
                    .run_task(task_id, request, worker_cancellation, &worker_events)
                    .await
            });
            let mut result = worker.await.unwrap_or_else(|_| {
                Err(AppError::internal(
                    ErrorCode::ParseFailed,
                    "task worker terminated unexpectedly",
                ))
            });
            if let Some(cleanup) = staged_cleanup.as_mut() {
                if cleanup.clean_now().is_err() {
                    manager.register_pending_cleanup(CleanupTarget::StagingDirectory(
                        cleanup.directory.clone(),
                    ));
                    if let Ok(completion) = &result {
                        if remove_file_if_present(&completion.output_path).is_err() {
                            manager.register_pending_cleanup(CleanupTarget::File(
                                completion.output_path.clone(),
                            ));
                        }
                    }
                    result = Err(AppError::new(ErrorCode::SaveFailed));
                }
            }
            manager.finish(task_id, result, &events);
        });
        if let Some(active) = self.lock().active.as_mut() {
            if active.task_id == task_id {
                active.worker = Some(handle);
            }
        }
        let _ = start_sender.send(());
        Ok(task_id)
    }

    pub fn cancel(&self) -> Result<(), AppError> {
        let state = self.lock();
        let active = state
            .active
            .as_ref()
            .ok_or_else(|| AppError::new(ErrorCode::NoActiveTask))?;
        active.cancellation.cancel();
        Ok(())
    }

    pub async fn cancel_and_wait(&self) -> Result<TaskStatus, AppError> {
        let task_id = {
            let state = self.lock();
            let active = state
                .active
                .as_ref()
                .ok_or_else(|| AppError::new(ErrorCode::NoActiveTask))?;
            active.cancellation.cancel();
            active.task_id
        };
        let status = self.wait_for_terminal(task_id).await?;
        if status.stage == Some(TaskStage::Failed) {
            return Err(status
                .error
                .clone()
                .unwrap_or_else(|| AppError::new(ErrorCode::SaveFailed)));
        }
        Ok(status)
    }

    async fn wait_for_terminal(&self, task_id: Uuid) -> Result<TaskStatus, AppError> {
        let mut terminal_changes = self.terminal_changes.subscribe();
        loop {
            if let Some(status) = self.lock().terminal_history.get(&task_id).cloned() {
                return Ok(status);
            }
            terminal_changes
                .changed()
                .await
                .map_err(|_| AppError::new(ErrorCode::SaveFailed))?;
        }
    }

    pub async fn prepare_exit(&self) -> Result<TaskStatus, AppError> {
        let active_task = {
            let mut state = self.lock();
            state.exiting = true;
            state.active.as_ref().map(|active| {
                active.cancellation.cancel();
                active.task_id
            })
        };
        if let Some(task_id) = active_task {
            let _ = self.wait_for_terminal(task_id).await?;
        }
        self.retry_pending_cleanup()
    }

    fn retry_pending_cleanup(&self) -> Result<TaskStatus, AppError> {
        let targets = self.lock().cleanup_pending.clone();
        let mut failed = Vec::new();
        for target in targets {
            if cleanup_target(&target).is_err() {
                failed.push(target);
            }
        }
        let mut state = self.lock();
        state.cleanup_pending = failed;
        let pending = !state.cleanup_pending.is_empty();
        let active_is_none = state.active.is_none();
        let history_update = state.last_status.as_mut().and_then(|status| {
            status.cleanup_pending = pending;
            status.safe_to_exit = !pending && active_is_none;
            status.task_id.map(|task_id| (task_id, status.clone()))
        });
        if let Some((_task_id, status)) = history_update {
            record_terminal_status(&mut state, status);
        }
        if pending {
            return Err(AppError::new(ErrorCode::SaveFailed));
        }
        Ok(state.last_status.clone().unwrap_or(TaskStatus {
            task_id: None,
            active: false,
            completed_chunks: 0,
            total_chunks: 0,
            stage: None,
            output_path: None,
            record_count: None,
            error: None,
            cleanup_pending: false,
            safe_to_exit: true,
        }))
    }

    pub(crate) fn register_pending_cleanup(&self, target: CleanupTarget) {
        let mut state = self.lock();
        if !state.cleanup_pending.contains(&target) {
            state.cleanup_pending.push(target);
        }
    }

    pub fn accepts_new_input(&self) -> bool {
        let state = self.lock();
        !state.exiting && state.active.is_none()
    }

    pub fn status(&self) -> TaskStatus {
        let state = self.lock();
        if let Some(active) = state.active.as_ref() {
            TaskStatus {
                task_id: Some(active.task_id),
                active: true,
                completed_chunks: active.completed_chunks,
                total_chunks: active.total_chunks,
                stage: Some(active.stage),
                output_path: None,
                record_count: None,
                error: None,
                cleanup_pending: !state.cleanup_pending.is_empty(),
                safe_to_exit: false,
            }
        } else {
            let pending = !state.cleanup_pending.is_empty();
            let mut status = state.last_status.clone().unwrap_or(TaskStatus {
                task_id: None,
                active: false,
                completed_chunks: 0,
                total_chunks: 0,
                stage: None,
                output_path: None,
                record_count: None,
                error: None,
                cleanup_pending: pending,
                safe_to_exit: !pending,
            });
            status.cleanup_pending = pending;
            status.safe_to_exit = !pending;
            status
        }
    }

    pub fn completed_output_parent(
        &self,
        requested: &std::path::Path,
    ) -> Result<PathBuf, AppError> {
        let output_path = {
            let state = self.lock();
            let status = state
                .last_status
                .as_ref()
                .filter(|status| {
                    !status.active
                        && status.stage == Some(TaskStage::Completed)
                        && status.output_path.as_deref() == Some(requested)
                })
                .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?;
            status
                .output_path
                .clone()
                .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?
        };
        let metadata = fs::symlink_metadata(&output_path)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        if !metadata.file_type().is_file() {
            return Err(AppError::new(ErrorCode::FileNotFound));
        }
        let parent = output_path
            .parent()
            .ok_or_else(|| AppError::new(ErrorCode::FileNotFound))?;
        let canonical_parent =
            fs::canonicalize(parent).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        if canonical_parent != parent {
            return Err(AppError::new(ErrorCode::FileNotFound));
        }
        Ok(canonical_parent)
    }

    pub fn is_active(&self) -> bool {
        self.lock().active.is_some()
    }

    async fn run_task(
        &self,
        task_id: Uuid,
        request: ExtractionRequest,
        cancellation: CancellationToken,
        events: &EventSink,
    ) -> Result<TaskCompletion, AppError> {
        self.set_stage(task_id, TaskStage::ValidatingInput, events);
        ensure_not_cancelled(&cancellation)?;
        let metadata = fs::metadata(&request.input_path)
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        let kind = validate_input(&request.input_path, &metadata)?;

        self.set_stage(task_id, TaskStage::ExtractingText, events);
        emit_log(
            events,
            task_id,
            LogLevel::Info,
            "正在提取文档文字。",
            &request.settings.api_key,
        );
        ensure_not_cancelled(&cancellation)?;
        let input_path = request.input_path.clone();
        let extract = self.extract.clone();
        let parsing = tauri::async_runtime::spawn_blocking(move || extract(input_path, kind));
        let document_text = tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                return Err(AppError::new(ErrorCode::Cancelled));
            }
            result = parsing => {
                result.map_err(|_| AppError::new(ErrorCode::ParseFailed))??
            }
        };
        ensure_not_cancelled(&cancellation)?;

        self.set_stage(task_id, TaskStage::PreparingChunks, events);
        let policy = ChunkPolicy {
            max_chars: request.settings.chunk_max_chars,
            context_chars: request.settings.context_chars,
        };
        let chunks = split_document_text(&document_text, policy);
        if chunks.is_empty() {
            return Err(AppError::new(ErrorCode::NoExtractableText));
        }
        let mut queue = VecDeque::from(chunks);
        let mut total_chunks = queue.len();
        let mut completed_chunks = 0;
        self.set_progress(task_id, completed_chunks, total_chunks, events);

        let system_prompt = build_system_prompt(&request.catalog);
        let client = AiClient::new(&request.settings)?;
        let mut results: Vec<SanitizedCsv> = Vec::new();
        self.set_stage(task_id, TaskStage::CallingAi, events);

        while let Some(chunk) = queue.pop_front() {
            ensure_not_cancelled(&cancellation)?;
            emit_log(
                events,
                task_id,
                LogLevel::Info,
                &format!("正在处理第 {}/{} 块。", completed_chunks + 1, total_chunks),
                &request.settings.api_key,
            );
            match client
                .extract_chunk(
                    &system_prompt,
                    &chunk,
                    completed_chunks + 1,
                    total_chunks,
                    &cancellation,
                )
                .await
            {
                Ok(raw_csv) => {
                    let sanitized = sanitize_csv(&raw_csv, &request.catalog)?;
                    if sanitized_record_count(&sanitized.csv) == 0 && !sanitized.warnings.is_empty()
                    {
                        return Err(AppError::new(ErrorCode::InvalidAiCsv));
                    }
                    for warning in &sanitized.warnings {
                        emit_log(
                            events,
                            task_id,
                            LogLevel::Warn,
                            warning,
                            &request.settings.api_key,
                        );
                    }
                    results.push(sanitized);
                    completed_chunks += 1;
                    self.set_progress(task_id, completed_chunks, total_chunks, events);
                }
                Err(error) if error_code(&error) == "context_too_large" => {
                    let [left, right] = bisect_chunk(&chunk, policy)?;
                    queue.push_front(right);
                    queue.push_front(left);
                    total_chunks += 1;
                    emit_log(
                        events,
                        task_id,
                        LogLevel::Warn,
                        "当前块超过模型上下文限制，已二分后继续处理。",
                        &request.settings.api_key,
                    );
                    self.set_progress(task_id, completed_chunks, total_chunks, events);
                }
                Err(error) => return Err(error),
            }
        }

        ensure_not_cancelled(&cancellation)?;
        self.set_stage(task_id, TaskStage::MergingResults, events);
        let merged = merge_csv_results(&results);
        for warning in &merged.warnings {
            emit_log(
                events,
                task_id,
                LogLevel::Warn,
                warning,
                &request.settings.api_key,
            );
        }
        let record_count = sanitized_record_count(&merged.csv);

        ensure_not_cancelled(&cancellation)?;
        self.set_stage(task_id, TaskStage::SavingOutput, events);
        let output_directory = PathBuf::from(&request.settings.output_directory);
        let output_name_source = request
            .output_name_source
            .as_ref()
            .unwrap_or(&request.input_path);
        let output_path = if let Some(capability) = request.output_directory_capability.as_ref() {
            save_csv_in_directory(capability, output_name_source, &merged.csv, Local::now())?
        } else {
            save_csv(
                &output_directory,
                output_name_source,
                &merged.csv,
                Local::now(),
            )?
        };
        if cancellation.is_cancelled() {
            if remove_file_if_present(&output_path).is_err() {
                self.register_pending_cleanup(CleanupTarget::File(output_path));
                return Err(AppError::new(ErrorCode::SaveFailed));
            }
            return Err(AppError::new(ErrorCode::Cancelled));
        }

        Ok(TaskCompletion {
            output_path,
            record_count,
        })
    }

    fn set_stage(&self, task_id: Uuid, stage: TaskStage, events: &EventSink) {
        if let Some(active) = self.lock().active.as_mut() {
            if active.task_id == task_id {
                active.stage = stage;
            }
        }
        emit_event(events, TaskEvent::Stage { task_id, stage });
    }

    fn set_progress(
        &self,
        task_id: Uuid,
        completed_chunks: usize,
        total_chunks: usize,
        events: &EventSink,
    ) {
        if let Some(active) = self.lock().active.as_mut() {
            if active.task_id == task_id {
                active.completed_chunks = completed_chunks;
                active.total_chunks = total_chunks;
            }
        }
        emit_event(
            events,
            TaskEvent::Progress {
                task_id,
                completed_chunks,
                total_chunks,
            },
        );
    }

    fn finish(
        &self,
        task_id: Uuid,
        mut result: Result<TaskCompletion, AppError>,
        events: &EventSink,
    ) {
        let terminal_stage = {
            let mut state = self.lock();
            if state.active.as_ref().map(|task| task.task_id) != Some(task_id) {
                return;
            }
            let cancelled = state
                .active
                .as_ref()
                .is_some_and(|task| task.cancellation.is_cancelled());
            if cancelled && result.is_ok() {
                result = match result.as_ref() {
                    Ok(completion) if fs::remove_file(&completion.output_path).is_err() => {
                        let target = CleanupTarget::File(completion.output_path.clone());
                        if !state.cleanup_pending.contains(&target) {
                            state.cleanup_pending.push(target);
                        }
                        Err(AppError::new(ErrorCode::SaveFailed))
                    }
                    _ => Err(AppError::new(ErrorCode::Cancelled)),
                };
            }
            let terminal_stage = match &result {
                Ok(_) => TaskStage::Completed,
                Err(error) if error_code(error) == "cancelled" => TaskStage::Cancelled,
                Err(_) => TaskStage::Failed,
            };
            let mut active = state.active.take().expect("matching active task exists");
            let _worker = active.worker.take();
            let (output_path, record_count, error) = match &result {
                Ok(completion) => (
                    Some(completion.output_path.clone()),
                    Some(completion.record_count),
                    None,
                ),
                Err(error) if error_code(error) != "cancelled" => (None, None, Some(error.clone())),
                Err(_) => (None, None, None),
            };
            let status = TaskStatus {
                task_id: Some(task_id),
                active: false,
                completed_chunks: active.completed_chunks,
                total_chunks: active.total_chunks,
                stage: Some(terminal_stage),
                output_path,
                record_count,
                error,
                cleanup_pending: !state.cleanup_pending.is_empty(),
                safe_to_exit: state.cleanup_pending.is_empty(),
            };
            state.last_status = Some(status.clone());
            record_terminal_status(&mut state, status);
            terminal_stage
        };

        self.terminal_changes.send_modify(|generation| {
            *generation = generation.wrapping_add(1);
        });

        emit_event(
            events,
            TaskEvent::Stage {
                task_id,
                stage: terminal_stage,
            },
        );
        match result {
            Ok(completion) => emit_event(
                events,
                TaskEvent::Completed {
                    task_id,
                    output_path: completion.output_path,
                    record_count: completion.record_count,
                },
            ),
            Err(error) if error_code(&error) == "cancelled" => {
                emit_event(events, TaskEvent::Cancelled { task_id })
            }
            Err(error) => emit_event(events, TaskEvent::Failed { task_id, error }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ManagerState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

const TERMINAL_HISTORY_LIMIT: usize = 64;

fn record_terminal_status(state: &mut ManagerState, status: TaskStatus) {
    let Some(task_id) = status.task_id else {
        return;
    };
    if !state.terminal_history.contains_key(&task_id) {
        state.terminal_order.push_back(task_id);
    }
    state.terminal_history.insert(task_id, status);
    while state.terminal_order.len() > TERMINAL_HISTORY_LIMIT {
        if let Some(expired) = state.terminal_order.pop_front() {
            state.terminal_history.remove(&expired);
        }
    }
}

impl Default for ExtractionTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

struct TaskCompletion {
    output_path: PathBuf,
    record_count: usize,
}

struct StagedInputCleanup {
    input_path: PathBuf,
    directory: PathBuf,
    cleaned: bool,
}

impl StagedInputCleanup {
    fn new(input_path: PathBuf, directory: PathBuf) -> Self {
        Self {
            input_path,
            directory,
            cleaned: false,
        }
    }

    fn clean_now(&mut self) -> Result<(), AppError> {
        if self.cleaned {
            return Ok(());
        }
        remove_file_if_present(&self.input_path)?;
        match fs::remove_dir(&self.directory) {
            Ok(()) => {
                self.cleaned = true;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleaned = true;
                Ok(())
            }
            Err(_) => Err(AppError::new(ErrorCode::SaveFailed)),
        }
    }
}

impl Drop for StagedInputCleanup {
    fn drop(&mut self) {
        let _ = self.clean_now();
    }
}

fn remove_file_if_present(path: &std::path::Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::new(ErrorCode::SaveFailed)),
    }
}

fn cleanup_target(target: &CleanupTarget) -> Result<(), AppError> {
    match target {
        CleanupTarget::File(path) => remove_file_if_present(path),
        CleanupTarget::StagingDirectory(path) => match fs::remove_dir_all(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::new(ErrorCode::SaveFailed)),
        },
    }
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), AppError> {
    if cancellation.is_cancelled() {
        Err(AppError::new(ErrorCode::Cancelled))
    } else {
        Ok(())
    }
}

fn sanitized_record_count(csv: &str) -> usize {
    csv::Reader::from_reader(csv.as_bytes()).records().count()
}

fn error_code(error: &AppError) -> String {
    serde_json::to_value(error).expect("AppError always serializes")["code"]
        .as_str()
        .expect("error code is serialized as a string")
        .to_owned()
}

fn emit_log(events: &EventSink, task_id: Uuid, level: LogLevel, message: &str, api_key: &str) {
    emit_event(
        events,
        TaskEvent::Log {
            task_id,
            level,
            message: redact_secrets(message, &[api_key]),
        },
    );
}

fn emit_event(events: &EventSink, event: TaskEvent) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| events(event)));
}

#[cfg(test)]
mod tests {
    use super::{EventSink, ExtractionRequest, ExtractionTaskManager, TaskEvent, TaskStage};
    use crate::{
        naming::{NamingCatalog, NamingEntry},
        output::OutputDirectoryCapability,
        register_csv::CSV_HEADER,
        settings::Settings,
    };
    use serde_json::json;
    use std::{
        collections::HashSet,
        fs,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tempfile::TempDir;
    use tokio::sync::mpsc;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, Request, Respond, ResponseTemplate,
    };

    #[tokio::test]
    async fn processes_chunks_sequentially_merges_results_and_reports_progress() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(SequenceResponder::new(vec![
                csv_response(100),
                csv_response(101),
            ]))
            .expect(2)
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, split_document());
        let (events, mut receiver) = channel_events();
        let task_id = fixture
            .manager
            .start(fixture.request(), events)
            .expect("task should start");

        let received = collect_until_terminal(&mut receiver).await;
        let completed = received.iter().find_map(|event| match event {
            TaskEvent::Completed {
                output_path,
                record_count,
                ..
            } => Some((output_path.clone(), *record_count)),
            _ => None,
        });
        let (output_path, record_count) = completed.expect("task should complete");
        assert_eq!(record_count, 2);
        let output = fs::read(output_path).unwrap();
        let csv = std::str::from_utf8(&output[3..]).unwrap();
        assert!(csv.contains("1,1,Ua,,100"));
        assert!(csv.contains("2,1,Ua,,101"));
        assert!(received.iter().any(|event| matches!(
            event,
            TaskEvent::Progress {
                task_id: id,
                completed_chunks: 1,
                total_chunks: 2,
            } if *id == task_id
        )));
        assert!(received.iter().any(|event| matches!(
            event,
            TaskEvent::Progress {
                completed_chunks: 2,
                total_chunks: 2,
                ..
            }
        )));
        let requests = server.received_requests().await.unwrap();
        let first = user_content(&requests[0]);
        let second = user_content(&requests[1]);
        assert!(first.contains("FIRST_MARKER"));
        assert!(second.contains("SECOND_MARKER"));
        assert_eq!(fixture.manager.status().stage, Some(TaskStage::Completed));
        assert!(!fixture.manager.status().active);
    }

    #[tokio::test]
    async fn task_uses_original_name_and_cleans_staged_snapshot_before_terminal_event() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(csv_response(100))
            .expect(1)
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let staging_dir = staging_parent.path().join("snapshot-id");
        fs::create_dir(&staging_dir).unwrap();
        let staged_path = staging_dir.join("manual.csv");
        fs::copy(fixture.input.path().join("manual.csv"), &staged_path).unwrap();
        let mut request = fixture.request();
        request.input_path = staged_path;
        request.output_name_source = Some(fixture.input.path().join("original-manual.csv"));
        request.staged_input_dir = Some(staging_dir.clone());
        request.output_directory_capability = Some(
            OutputDirectoryCapability::open(fs::canonicalize(fixture.output.path()).unwrap())
                .unwrap(),
        );
        let (events, mut receiver) = channel_events();

        fixture.manager.start(request, events).unwrap();
        let received = collect_until_terminal(&mut receiver).await;

        assert!(!staging_dir.exists());
        let output = received
            .iter()
            .find_map(|event| match event {
                TaskEvent::Completed { output_path, .. } => Some(output_path),
                _ => None,
            })
            .expect("task should complete");
        assert!(output
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("original-manual_"));
    }

    #[tokio::test]
    async fn bisects_only_the_context_overflowing_chunk_and_updates_dynamic_total() {
        let server = MockServer::start().await;
        let responder = OverflowResponder::new(8_500);
        Mock::given(method("POST"))
            .respond_with(responder.clone())
            .expect(3)
            .mount(&server)
            .await;
        let document = format!("LONG_SECTION\n{}", "中".repeat(9_500));
        let fixture = TaskFixture::new_with_chunk_max(&server, document, 12_000);
        let (events, mut receiver) = channel_events();
        fixture.manager.start(fixture.request(), events).unwrap();

        let received = collect_until_terminal(&mut receiver).await;

        assert!(received
            .iter()
            .any(|event| matches!(event, TaskEvent::Completed { .. })));
        assert!(received.iter().any(|event| matches!(
            event,
            TaskEvent::Progress {
                completed_chunks: 0,
                total_chunks: 2,
                ..
            }
        )));
        assert!(received.iter().any(|event| matches!(
            event,
            TaskEvent::Progress {
                completed_chunks: 2,
                total_chunks: 2,
                ..
            }
        )));
        assert_eq!(responder.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn rejects_a_second_task_until_the_active_task_has_terminated() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(csv_response(100).set_delay(Duration::from_secs(2)))
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let (request, staging_dir) = staged_request(fixture.request(), &staging_parent);
        let (events, mut receiver) = channel_events();
        fixture.manager.start(request, events).unwrap();
        let (events, _) = channel_events();

        let error = fixture
            .manager
            .start(fixture.request(), events)
            .expect_err("second task must be rejected");
        assert_eq!(error_code(error), "task_active");
        fixture.manager.cancel().unwrap();
        let received = collect_until_terminal(&mut receiver).await;
        assert!(received
            .iter()
            .any(|event| matches!(event, TaskEvent::Cancelled { .. })));
        assert!(!staging_dir.exists());
    }

    #[tokio::test]
    async fn cancellation_is_normal_terminal_state_and_never_saves_partial_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(csv_response(100).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let (request, staging_dir) = staged_request(fixture.request(), &staging_parent);
        let (events, mut receiver) = channel_events();
        fixture.manager.start(request, events).unwrap();
        wait_for_calling_ai(&mut receiver).await;
        fixture.manager.cancel().unwrap();

        let received = collect_until_terminal(&mut receiver).await;

        assert!(received
            .iter()
            .any(|event| matches!(event, TaskEvent::Cancelled { .. })));
        assert!(!received
            .iter()
            .any(|event| matches!(event, TaskEvent::Failed { .. })));
        assert_eq!(fs::read_dir(fixture.output.path()).unwrap().count(), 0);
        assert!(!staging_dir.exists());
        assert_eq!(fixture.manager.status().stage, Some(TaskStage::Cancelled));
        assert_eq!(
            error_code(
                fixture
                    .manager
                    .cancel()
                    .expect_err("no active task remains")
            ),
            "no_active_task"
        );
    }

    #[tokio::test]
    async fn cancel_and_wait_returns_only_after_terminal_cleanup_is_complete() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(csv_response(100).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let (request, staging_dir) = staged_request(fixture.request(), &staging_parent);
        let (events, mut receiver) = channel_events();
        fixture.manager.start(request, events).unwrap();
        wait_for_calling_ai(&mut receiver).await;

        let status = fixture.manager.cancel_and_wait().await.unwrap();

        assert!(!status.active);
        assert_eq!(status.stage, Some(TaskStage::Cancelled));
        assert!(!staging_dir.exists());
        assert_eq!(fs::read_dir(fixture.output.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn cancel_and_wait_reports_a_staged_snapshot_cleanup_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(csv_response(100).set_delay(Duration::from_secs(5)))
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let (request, staging_dir) = staged_request(fixture.request(), &staging_parent);
        fs::write(
            staging_dir.join("unexpected.tmp"),
            b"prevents directory cleanup",
        )
        .unwrap();
        let (events, mut receiver) = channel_events();
        fixture.manager.start(request, events).unwrap();
        wait_for_calling_ai(&mut receiver).await;

        let error = fixture
            .manager
            .cancel_and_wait()
            .await
            .expect_err("staging cleanup failure must block close");

        assert_eq!(error_code(error), "save_failed");
        assert_eq!(fixture.manager.status().stage, Some(TaskStage::Failed));
        assert!(staging_dir.exists());
    }

    #[tokio::test]
    async fn cancellation_releases_the_manager_without_waiting_for_blocking_parsing() {
        use std::sync::{Condvar, Mutex};

        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let extractor_gate = gate.clone();
        let extractor_started = started.clone();
        let manager = ExtractionTaskManager::with_extractor(Arc::new(move |_, _| {
            extractor_started.store(true, Ordering::SeqCst);
            let (lock, condition) = &*extractor_gate;
            let mut released = lock.lock().unwrap();
            while !*released {
                released = condition.wait(released).unwrap();
            }
            Ok("address,name\n100,Ua".into())
        }));
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(input.path().join("manual.csv"), "address,name\n100,Ua").unwrap();
        let request = offline_request(&input, &output);
        let (events, mut receiver) = channel_events();
        manager.start(request, events).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("blocking parser should start");

        let cancelled_at = std::time::Instant::now();
        manager.cancel().unwrap();
        let received = collect_until_terminal(&mut receiver).await;
        let cancellation_latency = cancelled_at.elapsed();

        {
            let (lock, condition) = &*gate;
            *lock.lock().unwrap() = true;
            condition.notify_all();
        }
        assert!(received
            .iter()
            .any(|event| matches!(event, TaskEvent::Cancelled { .. })));
        assert!(
            cancellation_latency < Duration::from_millis(300),
            "cancellation waited {cancellation_latency:?} for blocking parsing"
        );
        assert!(!manager.is_active());
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn stores_a_supervisor_handle_and_cleans_up_after_worker_and_event_sink_panics() {
        let manager = ExtractionTaskManager::with_extractor(Arc::new(|_, _| {
            panic!("synthetic parser panic")
        }));
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(input.path().join("manual.csv"), "address,name\n100,Ua").unwrap();
        let panicking_events: EventSink = Arc::new(|_| panic!("synthetic event sink panic"));
        manager
            .start(offline_request(&input, &output), panicking_events)
            .unwrap();
        assert!(manager
            .lock()
            .active
            .as_ref()
            .and_then(|active| active.worker.as_ref())
            .is_some());

        tokio::time::timeout(Duration::from_secs(2), async {
            while manager.is_active() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("supervisor should always clear active state");

        assert_eq!(manager.status().stage, Some(TaskStage::Failed));
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn invalid_csv_is_a_stable_failure_and_task_events_never_leak_the_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"content": "not csv hb-secret"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let fixture = TaskFixture::new(&server, "address,name\n100,Ua".into());
        let staging_parent = tempfile::tempdir().unwrap();
        let (request, staging_dir) = staged_request(fixture.request(), &staging_parent);
        let (events, mut receiver) = channel_events();
        fixture.manager.start(request, events).unwrap();

        let received = collect_until_terminal(&mut receiver).await;
        let failure = received.iter().find_map(|event| match event {
            TaskEvent::Failed { error, .. } => Some(error.clone()),
            _ => None,
        });
        assert_eq!(
            error_code(failure.expect("task should fail")),
            "invalid_ai_csv"
        );
        let serialized = serde_json::to_string(&received).unwrap();
        assert!(!serialized.contains("hb-secret"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        assert_eq!(fs::read_dir(fixture.output.path()).unwrap().count(), 0);
        assert!(!staging_dir.exists());
    }

    #[test]
    fn serializes_task_events_and_status_for_the_frontend_contract() {
        let task_id = uuid::Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap();
        let event = TaskEvent::Progress {
            task_id,
            completed_chunks: 2,
            total_chunks: 5,
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({
                "type": "progress",
                "taskId": "11111111-2222-4333-8444-555555555555",
                "completedChunks": 2,
                "totalChunks": 5
            })
        );

        let status = super::TaskStatus {
            task_id: Some(task_id),
            active: true,
            completed_chunks: 2,
            total_chunks: 5,
            stage: Some(TaskStage::CallingAi),
            output_path: None,
            record_count: None,
            error: None,
            cleanup_pending: false,
            safe_to_exit: false,
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "taskId": "11111111-2222-4333-8444-555555555555",
                "active": true,
                "completedChunks": 2,
                "totalChunks": 5,
                "stage": "calling_ai",
                "outputPath": null,
                "recordCount": null,
                "error": null,
                "cleanupPending": false,
                "safeToExit": false
            })
        );
    }

    #[test]
    fn cancellation_wins_the_finalization_race_and_removes_a_just_saved_output() {
        let manager = ExtractionTaskManager::new();
        let task_id = uuid::Uuid::new_v4();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        manager.lock().active = Some(super::ActiveTask {
            task_id,
            cancellation,
            completed_chunks: 1,
            total_chunks: 1,
            stage: TaskStage::SavingOutput,
            worker: None,
        });
        let directory = tempfile::tempdir().unwrap();
        let output_path = directory.path().join("just-saved.csv");
        fs::write(&output_path, b"partial").unwrap();
        let (events, mut receiver) = channel_events();

        manager.finish(
            task_id,
            Ok(super::TaskCompletion {
                output_path: output_path.clone(),
                record_count: 1,
            }),
            &events,
        );

        assert!(!output_path.exists());
        assert!(matches!(
            receiver.try_recv(),
            Ok(TaskEvent::Stage {
                stage: TaskStage::Cancelled,
                ..
            })
        ));
        assert!(matches!(
            receiver.try_recv(),
            Ok(TaskEvent::Cancelled { .. })
        ));
        assert_eq!(manager.status().stage, Some(TaskStage::Cancelled));
    }

    #[test]
    fn cancellation_cleanup_failure_is_reported_as_save_failed_not_cancelled() {
        let manager = ExtractionTaskManager::new();
        let task_id = uuid::Uuid::new_v4();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        manager.lock().active = Some(super::ActiveTask {
            task_id,
            cancellation,
            completed_chunks: 1,
            total_chunks: 1,
            stage: TaskStage::SavingOutput,
            worker: None,
        });
        let directory = tempfile::tempdir().unwrap();
        let undeletable_as_file = directory.path().join("still-a-directory");
        fs::create_dir(&undeletable_as_file).unwrap();
        let (events, mut receiver) = channel_events();

        manager.finish(
            task_id,
            Ok(super::TaskCompletion {
                output_path: undeletable_as_file.clone(),
                record_count: 1,
            }),
            &events,
        );

        assert!(undeletable_as_file.is_dir());
        assert!(matches!(
            receiver.try_recv(),
            Ok(TaskEvent::Stage {
                stage: TaskStage::Failed,
                ..
            })
        ));
        let failure = receiver.try_recv().unwrap();
        let TaskEvent::Failed { error, .. } = failure else {
            panic!("cleanup failure must be terminal failure")
        };
        assert_eq!(error_code(error), "save_failed");
        assert_eq!(manager.status().stage, Some(TaskStage::Failed));
    }

    #[tokio::test]
    async fn cancel_and_wait_returns_the_safe_terminal_failure() {
        let manager = ExtractionTaskManager::new();
        let task_id = uuid::Uuid::new_v4();
        let cancellation = tokio_util::sync::CancellationToken::new();
        manager.lock().active = Some(super::ActiveTask {
            task_id,
            cancellation,
            completed_chunks: 1,
            total_chunks: 1,
            stage: TaskStage::SavingOutput,
            worker: None,
        });
        let directory = tempfile::tempdir().unwrap();
        let undeletable_as_file = directory.path().join("still-a-directory");
        fs::create_dir(&undeletable_as_file).unwrap();
        let finish_manager = manager.clone();
        let output_path = undeletable_as_file.clone();
        let (events, _) = channel_events();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            finish_manager.finish(
                task_id,
                Ok(super::TaskCompletion {
                    output_path,
                    record_count: 1,
                }),
                &events,
            );
        });

        let error = manager
            .cancel_and_wait()
            .await
            .expect_err("cleanup failure must block close");

        assert_eq!(error_code(error), "save_failed");
        assert_eq!(manager.status().stage, Some(TaskStage::Failed));
    }

    #[test]
    fn completed_output_parent_accepts_only_the_exact_latest_completed_path() {
        let manager = ExtractionTaskManager::new();
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("result.csv");
        let other = directory.path().join("other.csv");
        fs::write(&output, b"result").unwrap();
        fs::write(&other, b"other").unwrap();
        let output = fs::canonicalize(output).unwrap();
        manager.lock().last_status = Some(super::TaskStatus {
            task_id: Some(uuid::Uuid::new_v4()),
            active: false,
            completed_chunks: 1,
            total_chunks: 1,
            stage: Some(TaskStage::Completed),
            output_path: Some(output.clone()),
            record_count: Some(1),
            error: None,
            cleanup_pending: false,
            safe_to_exit: true,
        });

        assert_eq!(
            manager.completed_output_parent(&output).unwrap(),
            fs::canonicalize(directory.path()).unwrap()
        );
        assert_eq!(
            error_code(manager.completed_output_parent(&other).unwrap_err()),
            "file_not_found"
        );
    }

    #[tokio::test]
    async fn prepare_exit_retries_inactive_pending_cleanup_until_it_is_safe() {
        let manager = ExtractionTaskManager::new();
        let directory = tempfile::tempdir().unwrap();
        let pending_file = directory.path().join("pending-output.csv");
        fs::create_dir(&pending_file).unwrap();
        manager.register_pending_cleanup(super::CleanupTarget::File(pending_file.clone()));

        let error = manager
            .prepare_exit()
            .await
            .expect_err("directory cannot be removed as file");
        assert_eq!(error_code(error), "save_failed");
        assert!(manager.status().cleanup_pending);
        assert!(!manager.status().safe_to_exit);

        fs::remove_dir(&pending_file).unwrap();
        fs::write(&pending_file, b"retryable output").unwrap();
        let status = manager.prepare_exit().await.unwrap();
        assert!(status.safe_to_exit);
        assert!(!status.cleanup_pending);
        assert!(!pending_file.exists());
    }

    #[tokio::test]
    async fn prepare_exit_gate_blocks_a_new_task_even_after_cleanup_is_safe() {
        let manager = ExtractionTaskManager::new();
        let status = manager.prepare_exit().await.unwrap();
        assert!(status.safe_to_exit);
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        fs::write(input.path().join("manual.csv"), "address,name\n100,Ua").unwrap();
        let (events, _) = channel_events();

        let error = manager
            .start(offline_request(&input, &output), events)
            .expect_err("exit gate must reject a new task");
        assert_eq!(error_code(error), "task_active");
    }

    #[tokio::test]
    async fn terminal_history_resolves_the_requested_task_after_last_status_is_replaced() {
        let manager = ExtractionTaskManager::new();
        let first_id = uuid::Uuid::new_v4();
        manager.lock().active = Some(super::ActiveTask {
            task_id: first_id,
            cancellation: tokio_util::sync::CancellationToken::new(),
            completed_chunks: 0,
            total_chunks: 0,
            stage: TaskStage::ValidatingInput,
            worker: None,
        });
        let (events, _) = channel_events();
        manager.finish(
            first_id,
            Err(crate::error::AppError::new(
                crate::error::ErrorCode::Cancelled,
            )),
            &events,
        );
        let second_id = uuid::Uuid::new_v4();
        {
            let mut state = manager.lock();
            state.last_status = None;
            state.active = Some(super::ActiveTask {
                task_id: second_id,
                cancellation: tokio_util::sync::CancellationToken::new(),
                completed_chunks: 0,
                total_chunks: 0,
                stage: TaskStage::ValidatingInput,
                worker: None,
            });
        }

        let status = manager.wait_for_terminal(first_id).await.unwrap();
        assert_eq!(status.task_id, Some(first_id));
        assert_eq!(status.stage, Some(TaskStage::Cancelled));
    }

    #[test]
    fn terminal_history_is_bounded_to_the_latest_sixty_four_tasks() {
        let manager = ExtractionTaskManager::new();
        let mut ids = Vec::new();
        let mut state = manager.lock();
        for _ in 0..65 {
            let task_id = uuid::Uuid::new_v4();
            ids.push(task_id);
            super::record_terminal_status(
                &mut state,
                super::TaskStatus {
                    task_id: Some(task_id),
                    active: false,
                    completed_chunks: 0,
                    total_chunks: 0,
                    stage: Some(TaskStage::Cancelled),
                    output_path: None,
                    record_count: None,
                    error: None,
                    cleanup_pending: false,
                    safe_to_exit: true,
                },
            );
        }

        assert_eq!(state.terminal_history.len(), 64);
        assert!(!state.terminal_history.contains_key(&ids[0]));
        assert!(state.terminal_history.contains_key(ids.last().unwrap()));
    }

    struct TaskFixture {
        manager: ExtractionTaskManager,
        input: TempDir,
        output: TempDir,
        settings: Settings,
    }

    impl TaskFixture {
        fn new(server: &MockServer, document: String) -> Self {
            Self::new_with_chunk_max(server, document, 8_000)
        }

        fn new_with_chunk_max(server: &MockServer, document: String, chunk_max: usize) -> Self {
            let input = tempfile::tempdir().unwrap();
            let output = tempfile::tempdir().unwrap();
            fs::write(input.path().join("manual.csv"), document).unwrap();
            let settings = Settings {
                base_url: format!("{}/v1", server.uri()),
                api_key: "hb-secret".into(),
                output_directory: output.path().to_string_lossy().into_owned(),
                chunk_max_chars: chunk_max,
                context_chars: 100,
                timeout_seconds: 10,
                ..Settings::default()
            };
            Self {
                manager: ExtractionTaskManager::new(),
                input,
                output,
                settings,
            }
        }

        fn request(&self) -> ExtractionRequest {
            ExtractionRequest {
                input_path: self.input.path().join("manual.csv"),
                output_name_source: None,
                staged_input_dir: None,
                output_directory_capability: None,
                settings: self.settings.clone(),
                catalog: catalog(),
            }
        }
    }

    #[derive(Clone)]
    struct SequenceResponder {
        responses: Arc<Vec<ResponseTemplate>>,
        calls: Arc<AtomicUsize>,
    }

    impl SequenceResponder {
        fn new(responses: Vec<ResponseTemplate>) -> Self {
            Self {
                responses: Arc::new(responses),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Respond for SequenceResponder {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            let index = self.calls.fetch_add(1, Ordering::SeqCst);
            self.responses[index.min(self.responses.len() - 1)].clone()
        }
    }

    #[derive(Clone)]
    struct OverflowResponder {
        threshold: usize,
        calls: Arc<AtomicUsize>,
    }

    impl OverflowResponder {
        fn new(threshold: usize) -> Self {
            Self {
                threshold,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Respond for OverflowResponder {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if user_content(request).chars().count() > self.threshold {
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": {"code": "context_length_exceeded"}
                }))
            } else {
                csv_response(100)
            }
        }
    }

    fn split_document() -> String {
        format!(
            "FIRST_MARKER{}\nSECOND_MARKER{}",
            "a".repeat(4_100),
            "b".repeat(4_100)
        )
    }

    fn csv_response(address: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": format!(
                "{CSV_HEADER}\n1,1,Ua,,{address},6,1,1,1,3,,0"
            )}}]
        }))
    }

    fn user_content(request: &Request) -> String {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        body["messages"][1]["content"].as_str().unwrap().to_owned()
    }

    fn catalog() -> NamingCatalog {
        NamingCatalog {
            entries: vec![NamingEntry {
                code: "Ua".into(),
                meaning: "A相电压".into(),
            }],
            names: HashSet::from(["ua".into()]),
            reference: "Ua=A相电压".into(),
        }
    }

    fn offline_request(input: &TempDir, output: &TempDir) -> ExtractionRequest {
        ExtractionRequest {
            input_path: input.path().join("manual.csv"),
            output_name_source: None,
            staged_input_dir: None,
            output_directory_capability: None,
            settings: Settings {
                base_url: "http://127.0.0.1:9/v1".into(),
                output_directory: output.path().to_string_lossy().into_owned(),
                chunk_max_chars: 8_000,
                context_chars: 100,
                ..Settings::default()
            },
            catalog: catalog(),
        }
    }

    fn staged_request(
        mut request: ExtractionRequest,
        staging_parent: &TempDir,
    ) -> (ExtractionRequest, std::path::PathBuf) {
        let original = request.input_path.clone();
        let staging_dir = staging_parent
            .path()
            .join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir(&staging_dir).unwrap();
        let staged = staging_dir.join(original.file_name().unwrap());
        fs::copy(&original, &staged).unwrap();
        request.input_path = staged;
        request.output_name_source = Some(original);
        request.staged_input_dir = Some(staging_dir.clone());
        request.output_directory_capability = Some(
            OutputDirectoryCapability::open(
                fs::canonicalize(&request.settings.output_directory).unwrap(),
            )
            .unwrap(),
        );
        (request, staging_dir)
    }

    fn channel_events() -> (EventSink, mpsc::UnboundedReceiver<TaskEvent>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let events: EventSink = Arc::new(move |event| {
            let _ = sender.send(event);
        });
        (events, receiver)
    }

    async fn wait_for_calling_ai(receiver: &mut mpsc::UnboundedReceiver<TaskEvent>) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = receiver.recv().await {
                if matches!(
                    event,
                    TaskEvent::Stage {
                        stage: TaskStage::CallingAi,
                        ..
                    }
                ) {
                    return;
                }
            }
            panic!("event stream ended before calling AI");
        })
        .await
        .expect("task should reach AI stage");
    }

    async fn collect_until_terminal(
        receiver: &mut mpsc::UnboundedReceiver<TaskEvent>,
    ) -> Vec<TaskEvent> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut events = Vec::new();
            while let Some(event) = receiver.recv().await {
                let terminal = matches!(
                    event,
                    TaskEvent::Completed { .. }
                        | TaskEvent::Cancelled { .. }
                        | TaskEvent::Failed { .. }
                );
                events.push(event);
                if terminal {
                    break;
                }
            }
            events
        })
        .await
        .expect("task should terminate")
    }

    fn error_code(error: crate::error::AppError) -> String {
        serde_json::to_value(error).unwrap()["code"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}
