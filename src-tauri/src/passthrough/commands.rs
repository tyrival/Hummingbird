use super::explanation::build_manual_register_map;
use super::register_map::RegisterMap;
use super::{parse_message_pairs, AppliedRegisterExplanation, MessageParseResult};
use crate::commands::CommandState;
use crate::error::{AppError, ErrorCode};
use crate::naming::{load_naming_catalog, ResourcePaths};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct PassthroughAppState {
    active: Mutex<Option<ActivePassthroughTask>>,
    generation: AtomicU64,
}

struct ActivePassthroughTask {
    id: u64,
    cancellation: CancellationToken,
}

struct PassthroughTask<'a> {
    state: &'a PassthroughAppState,
    id: u64,
    cancellation: CancellationToken,
}

impl PassthroughAppState {
    fn begin(&self) -> Result<PassthroughTask<'_>, AppError> {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.is_some() {
            return Err(AppError::new(ErrorCode::TaskActive));
        }
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let cancellation = CancellationToken::new();
        *active = Some(ActivePassthroughTask {
            id,
            cancellation: cancellation.clone(),
        });
        Ok(PassthroughTask {
            state: self,
            id,
            cancellation,
        })
    }

    fn cancel(&self) {
        if let Some(active) = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            active.cancellation.cancel();
        }
    }
}

impl Drop for PassthroughTask<'_> {
    fn drop(&mut self) {
        let mut active = self
            .state
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_ref().is_some_and(|task| task.id == self.id) {
            *active = None;
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassthroughParseRequest {
    pub request_hex: String,
    pub response_hex: Option<String>,
    pub source: Option<PassthroughSource>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassthroughSource {
    pub kind: PassthroughSourceKind,
    pub path: String,
    pub file_name: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassthroughSourceKind {
    Manual,
    AwtTemplate,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassthroughBatchResult {
    pub results: Vec<MessageParseResult>,
    pub source_warning: Option<String>,
    pub mapping_diagnostics: Option<RegisterMapDiagnostics>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterMapDiagnostics {
    pub extracted_count: usize,
    pub matched_count: usize,
    pub unmatched_addresses: Vec<u16>,
}

#[tauri::command]
pub async fn parse_passthrough_messages(
    app: AppHandle,
    state: State<'_, CommandState>,
    passthrough_state: State<'_, PassthroughAppState>,
    request: PassthroughParseRequest,
) -> Result<PassthroughBatchResult, AppError> {
    let task = passthrough_state.begin()?;
    let mut results = parse_message_pairs(&request.request_hex, request.response_hex.as_deref())?;
    let mut mapping_diagnostics = None;
    let source_warning = if let Some(source) = request.source {
        match source.kind {
            PassthroughSourceKind::AwtTemplate => {
                let bytes = state.read_authorized_input(Path::new(&source.path))?;
                let csv = String::from_utf8(bytes)
                    .map_err(|_| AppError::new(ErrorCode::InvalidPassthroughSource))?;
                let resources = ResourcePaths::bundled(&app)?;
                let catalog = load_naming_catalog(&resources)?;
                let register_map = RegisterMap::from_awt_csv(&csv, &catalog)?;
                mapping_diagnostics = Some(apply_register_map(&mut results, &register_map));
                None
            }
            PassthroughSourceKind::Manual => {
                let path = state.authorized_input_path(Path::new(&source.path))?;
                let resources = ResourcePaths::bundled(&app)?;
                let catalog = load_naming_catalog(&resources)?;
                let settings = state.settings();
                let register_map =
                    build_manual_register_map(&path, &settings, &catalog, &task.cancellation)
                        .await?;
                mapping_diagnostics = Some(apply_register_map(&mut results, &register_map));
                None
            }
        }
    } else {
        None
    };
    Ok(PassthroughBatchResult {
        results,
        source_warning,
        mapping_diagnostics,
    })
}

#[tauri::command]
pub fn cancel_passthrough_parse(state: State<'_, PassthroughAppState>) {
    state.cancel();
}

fn apply_register_map(
    results: &mut [MessageParseResult],
    register_map: &RegisterMap,
) -> RegisterMapDiagnostics {
    let mut addresses = results
        .iter()
        .flat_map(|result| {
            result
                .registers
                .iter()
                .filter_map(|register| register.address)
        })
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    let unmatched_addresses = addresses
        .iter()
        .copied()
        .filter(|address| !register_map.contains_address(*address))
        .collect::<Vec<_>>();
    let matched_count = addresses.len().saturating_sub(unmatched_addresses.len());
    for result in results {
        result.explanations = register_map
            .explain(&result.registers)
            .into_iter()
            .map(|explanation| AppliedRegisterExplanation {
                address: explanation.address,
                parameter_code: explanation.parameter_code,
                parameter_name: explanation.parameter_name,
                unit: explanation.unit,
                raw_hex: explanation.raw_hex,
                converted_value: explanation.converted_value,
                meaning: explanation.meaning,
                source: explanation.source,
                warnings: explanation.warnings,
            })
            .collect();
    }
    RegisterMapDiagnostics {
        extracted_count: register_map.definition_count(),
        matched_count,
        unmatched_addresses,
    }
}

#[cfg(test)]
mod tests {
    use super::PassthroughAppState;

    #[test]
    fn cancellation_releases_the_active_passthrough_slot() {
        let state = PassthroughAppState::default();
        let task = state.begin().unwrap();
        assert!(!task.cancellation.is_cancelled());
        state.cancel();
        assert!(task.cancellation.is_cancelled());
        drop(task);
        assert!(state.begin().is_ok());
    }
}
