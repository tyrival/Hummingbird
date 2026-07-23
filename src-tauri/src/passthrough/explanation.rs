use super::register_map::RegisterMap;
use super::{FactSource, ParseWarning, RegisterValue};
use crate::ai::AiClient;
use crate::chunking::{bisect_chunk, split_document_text, ChunkPolicy};
use crate::error::{AppError, ErrorCode};
use crate::extraction::{extract_document, validate_input};
use crate::naming::NamingCatalog;
use crate::prompt::build_system_prompt;
use crate::register_csv::{merge_csv_results, sanitize_csv, SanitizedCsv};
use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRegisterExplanation {
    pub register_address: u16,
    pub parameter_code: Option<String>,
    pub parameter_name: Option<String>,
    pub converted_value: Option<String>,
    pub meaning: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ExplanationConfidence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationConfidence {
    High,
    Medium,
    Low,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedExplanation {
    pub register_address: u16,
    pub parameter_code: Option<String>,
    pub parameter_name: Option<String>,
    pub converted_value: Option<String>,
    pub meaning: Option<String>,
    pub evidence_ids: Vec<String>,
    pub confidence: ExplanationConfidence,
    pub source: FactSource,
}

pub fn validate_ai_explanations(
    registers: &[RegisterValue],
    allowed_evidence_ids: &HashSet<String>,
    candidates: Vec<AiRegisterExplanation>,
) -> (Vec<ValidatedExplanation>, Vec<ParseWarning>) {
    let known_addresses = registers
        .iter()
        .filter_map(|register| register.address)
        .collect::<HashSet<_>>();
    let mut accepted = HashMap::<u16, ValidatedExplanation>::new();
    let mut warnings = Vec::new();
    for candidate in candidates {
        if !known_addresses.contains(&candidate.register_address) {
            warnings.push(ParseWarning {
                code: "ai_fact_conflict".to_owned(),
                message: format!(
                    "AI 返回了代码结果中不存在的地址 0x{:04X}。",
                    candidate.register_address
                ),
            });
            continue;
        }
        if candidate.evidence_ids.is_empty()
            || candidate
                .evidence_ids
                .iter()
                .any(|id| !allowed_evidence_ids.contains(id))
        {
            warnings.push(ParseWarning {
                code: "missing_evidence".to_owned(),
                message: format!(
                    "地址 0x{:04X} 的 AI 解释缺少有效依据。",
                    candidate.register_address
                ),
            });
            continue;
        }
        accepted
            .entry(candidate.register_address)
            .or_insert(ValidatedExplanation {
                register_address: candidate.register_address,
                parameter_code: candidate.parameter_code,
                parameter_name: candidate.parameter_name,
                converted_value: candidate.converted_value,
                meaning: candidate.meaning,
                evidence_ids: candidate.evidence_ids,
                confidence: candidate.confidence,
                source: FactSource::AiExplanation,
            });
    }
    (accepted.into_values().collect(), warnings)
}

pub async fn build_manual_register_map(
    path: &Path,
    settings: &Settings,
    catalog: &NamingCatalog,
    cancellation: &CancellationToken,
) -> Result<RegisterMap, AppError> {
    if cancellation.is_cancelled() {
        return Err(AppError::new(ErrorCode::Cancelled));
    }
    let metadata = std::fs::metadata(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let kind = validate_input(path, &metadata)?;
    let document_text = extract_document(path, kind)?;
    if cancellation.is_cancelled() {
        return Err(AppError::new(ErrorCode::Cancelled));
    }
    let policy = ChunkPolicy {
        max_chars: settings.chunk_max_chars,
        context_chars: settings.context_chars,
    };
    let chunks = split_document_text(&document_text, policy);
    if chunks.is_empty() {
        return Err(AppError::new(ErrorCode::NoExtractableText));
    }
    let client = AiClient::new(settings)?;
    let system_prompt = build_system_prompt(catalog);
    let mut queue = VecDeque::from(chunks);
    let mut results = Vec::<SanitizedCsv>::new();
    let mut completed = 0_usize;

    while let Some(chunk) = queue.pop_front() {
        if cancellation.is_cancelled() {
            return Err(AppError::new(ErrorCode::Cancelled));
        }
        let total = completed + queue.len() + 1;
        let raw = client
            .extract_chunk(&system_prompt, &chunk, completed + 1, total, cancellation)
            .await?;
        let mut sanitized = sanitize_csv(&raw, catalog)?;
        if sanitized.diagnostics.valid_records == 0 && sanitized.diagnostics.rejected_records > 0 {
            let diagnostics = sanitized.diagnostics.summary();
            let corrected = client
                .extract_chunk_with_correction(
                    &system_prompt,
                    &chunk,
                    completed + 1,
                    total,
                    &diagnostics,
                    cancellation,
                )
                .await?;
            sanitized = sanitize_csv(&corrected, catalog)?;
            if sanitized.diagnostics.valid_records == 0
                && sanitized.diagnostics.rejected_records > 0
            {
                let [left, right] = bisect_chunk(&chunk, policy)
                    .map_err(|_| AppError::new(ErrorCode::InvalidAiCsv))?;
                queue.push_front(right);
                queue.push_front(left);
                continue;
            }
        }
        if sanitized.diagnostics.valid_records == 0 {
            return Err(AppError::new(ErrorCode::InvalidAiCsv));
        }
        results.push(sanitized);
        completed += 1;
    }

    let merged = merge_csv_results(&results);
    RegisterMap::from_awt_csv(&merged.csv, catalog)
}

#[cfg(test)]
mod tests {
    use super::{validate_ai_explanations, AiRegisterExplanation, ExplanationConfidence};
    use crate::passthrough::{FactSource, RegisterValue};
    use std::collections::HashSet;

    #[test]
    fn rejects_unknown_addresses_and_missing_evidence() {
        let registers = vec![RegisterValue {
            address: Some(1),
            identifier: None,
            raw_hex: "0001".to_owned(),
            source: FactSource::Code,
        }];
        let candidates = vec![
            AiRegisterExplanation {
                register_address: 2,
                parameter_code: None,
                parameter_name: Some("猜测".to_owned()),
                converted_value: None,
                meaning: None,
                evidence_ids: vec!["row-1".to_owned()],
                confidence: ExplanationConfidence::High,
            },
            AiRegisterExplanation {
                register_address: 1,
                parameter_code: None,
                parameter_name: Some("无依据".to_owned()),
                converted_value: None,
                meaning: None,
                evidence_ids: Vec::new(),
                confidence: ExplanationConfidence::Low,
            },
        ];
        let (accepted, warnings) =
            validate_ai_explanations(&registers, &HashSet::from(["row-1".to_owned()]), candidates);
        assert!(accepted.is_empty());
        assert_eq!(warnings.len(), 2);
    }
}
