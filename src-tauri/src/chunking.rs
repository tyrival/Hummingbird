use crate::{
    error::{AppError, ErrorCode},
    register_csv::is_circuit_range_heading,
};

pub const DEFAULT_MAX_CHARS: usize = 12_000;
pub const DEFAULT_CONTEXT_CHARS: usize = 1_500;
const MIN_BISECT_CHARS: usize = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkPolicy {
    pub max_chars: usize,
    pub context_chars: usize,
}

impl Default for ChunkPolicy {
    fn default() -> Self {
        Self {
            max_chars: DEFAULT_MAX_CHARS,
            context_chars: DEFAULT_CONTEXT_CHARS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentChunk {
    pub index: usize,
    pub text: String,
    pub prior_context: Option<String>,
}

pub fn split_document_text(text: &str, policy: ChunkPolicy) -> Vec<DocumentChunk> {
    if text.is_empty() || policy.max_chars == 0 {
        return Vec::new();
    }

    let characters = text.chars().collect::<Vec<_>>();
    let structure = Structure::analyze(text);
    let mut texts = Vec::new();
    let mut cursor = 0;

    while cursor < characters.len() {
        let prefix = structure.inherited_prefix(cursor, policy.max_chars);
        let prefix_chars = prefix.chars().count();
        let capacity = policy.max_chars.saturating_sub(prefix_chars);
        if capacity == 0 {
            return Vec::new();
        }

        let limit = (cursor + capacity).min(characters.len());
        let end = if limit == characters.len() {
            limit
        } else {
            structure.preferred_boundary(cursor, limit).unwrap_or(limit)
        };
        if end <= cursor {
            return Vec::new();
        }

        let body = characters[cursor..end].iter().collect::<String>();
        texts.push(format!("{prefix}{body}"));
        cursor = end;
    }

    let mut chunks: Vec<DocumentChunk> = Vec::with_capacity(texts.len());
    for (index, text) in texts.into_iter().enumerate() {
        let prior_context = chunks
            .last()
            .filter(|_| policy.context_chars > 0)
            .map(|previous| tail_chars(&previous.text, policy.context_chars));
        chunks.push(DocumentChunk {
            index,
            text,
            prior_context,
        });
    }
    chunks
}

pub fn bisect_chunk(
    chunk: &DocumentChunk,
    policy: ChunkPolicy,
) -> Result<[DocumentChunk; 2], AppError> {
    let characters = chunk.text.chars().collect::<Vec<_>>();
    if characters.len() <= MIN_BISECT_CHARS {
        return Err(AppError::new(ErrorCode::ContextTooLarge));
    }

    let structure = Structure::analyze(&chunk.text);
    let midpoint = characters.len() / 2;
    let lower = characters.len() / 4;
    let upper = characters.len() - lower;
    let split = structure
        .balanced_boundary(lower.max(1), upper.min(characters.len() - 1), midpoint)
        .unwrap_or(midpoint);
    if split == 0 || split >= characters.len() {
        return Err(AppError::new(ErrorCode::ContextTooLarge));
    }

    let left_text = characters[..split].iter().collect::<String>();
    let right_body = characters[split..].iter().collect::<String>();
    let right_prefix = structure.inherited_prefix(split, usize::MAX);
    let right_text = format!("{right_prefix}{right_body}");
    if left_text.chars().count() >= characters.len()
        || right_text.chars().count() >= characters.len()
    {
        return Err(AppError::new(ErrorCode::ContextTooLarge));
    }

    Ok([
        DocumentChunk {
            index: chunk.index,
            text: left_text.clone(),
            prior_context: chunk.prior_context.clone(),
        },
        DocumentChunk {
            index: chunk.index + 1,
            text: right_text,
            prior_context: (policy.context_chars > 0)
                .then(|| tail_chars(&left_text, policy.context_chars)),
        },
    ])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum BoundaryPriority {
    BlankSection,
    SheetHeading,
    RangeHeading,
    Line,
}

#[derive(Clone, Debug)]
struct Boundary {
    offset: usize,
    priority: BoundaryPriority,
}

#[derive(Clone, Debug)]
enum HeadingKind {
    Sheet,
    Range,
}

#[derive(Clone, Debug)]
struct Heading {
    offset: usize,
    kind: HeadingKind,
    text: String,
}

#[derive(Clone, Debug, Default)]
struct Structure {
    boundaries: Vec<Boundary>,
    headings: Vec<Heading>,
}

impl Structure {
    fn analyze(text: &str) -> Self {
        let mut structure = Self::default();
        let mut offset = 0;

        for line_with_ending in text.split_inclusive('\n') {
            let line = line_with_ending
                .strip_suffix('\n')
                .unwrap_or(line_with_ending);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let line_chars = line_with_ending.chars().count();
            let trimmed = line.trim();

            if is_sheet_heading(trimmed) {
                if offset > 0 {
                    structure.boundaries.push(Boundary {
                        offset,
                        priority: BoundaryPriority::SheetHeading,
                    });
                }
                structure.headings.push(Heading {
                    offset,
                    kind: HeadingKind::Sheet,
                    text: trimmed.to_owned(),
                });
            } else if is_circuit_range_heading(trimmed) {
                if offset > 0 {
                    structure.boundaries.push(Boundary {
                        offset,
                        priority: BoundaryPriority::RangeHeading,
                    });
                }
                structure.headings.push(Heading {
                    offset,
                    kind: HeadingKind::Range,
                    text: trimmed.to_owned(),
                });
            }

            offset += line_chars;
            if line_with_ending.ends_with('\n') {
                structure.boundaries.push(Boundary {
                    offset,
                    priority: if trimmed.is_empty() {
                        BoundaryPriority::BlankSection
                    } else {
                        BoundaryPriority::Line
                    },
                });
            }
        }

        structure
    }

    fn preferred_boundary(&self, start: usize, limit: usize) -> Option<usize> {
        self.boundaries
            .iter()
            .filter(|boundary| boundary.offset > start && boundary.offset <= limit)
            .min_by_key(|candidate| (candidate.priority, std::cmp::Reverse(candidate.offset)))
            .map(|boundary| boundary.offset)
    }

    fn balanced_boundary(&self, lower: usize, upper: usize, midpoint: usize) -> Option<usize> {
        self.boundaries
            .iter()
            .filter(|boundary| boundary.offset >= lower && boundary.offset <= upper)
            .min_by_key(|candidate| {
                (
                    candidate.priority,
                    candidate.offset.abs_diff(midpoint),
                    candidate.offset,
                )
            })
            .map(|boundary| boundary.offset)
    }

    fn inherited_prefix(&self, cursor: usize, max_chars: usize) -> String {
        let event_at_cursor = self
            .headings
            .iter()
            .find(|heading| heading.offset == cursor);
        if matches!(
            event_at_cursor.map(|event| &event.kind),
            Some(HeadingKind::Sheet)
        ) {
            return String::new();
        }

        let mut sheet = None;
        let mut range = None;
        for heading in self
            .headings
            .iter()
            .filter(|heading| heading.offset < cursor)
        {
            match heading.kind {
                HeadingKind::Sheet => {
                    sheet = Some(heading.text.as_str());
                    range = None;
                }
                HeadingKind::Range => range = Some(heading.text.as_str()),
            }
        }
        if matches!(
            event_at_cursor.map(|event| &event.kind),
            Some(HeadingKind::Range)
        ) {
            range = None;
        }

        let candidates = [sheet, range].into_iter().flatten().collect::<Vec<_>>();
        if candidates.is_empty() {
            return String::new();
        }
        let prefix = format!("{}\n", candidates.join("\n"));
        if prefix.chars().count() < max_chars {
            prefix
        } else {
            String::new()
        }
    }
}

fn is_sheet_heading(line: &str) -> bool {
    line == "=== CSV ===" || line.starts_with("=== Sheet:")
}

fn tail_chars(value: &str, count: usize) -> String {
    let characters = value.chars().collect::<Vec<_>>();
    characters[characters.len().saturating_sub(count)..]
        .iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{bisect_chunk, split_document_text, ChunkPolicy, DocumentChunk};

    const STRUCTURED_FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/chunking/structured-sections.txt"
    ));

    #[test]
    fn policy_defaults_to_twelve_thousand_characters_with_fifteen_hundred_context() {
        assert_eq!(
            ChunkPolicy::default(),
            ChunkPolicy {
                max_chars: 12_000,
                context_chars: 1_500,
            }
        );
    }

    #[test]
    fn repeats_the_current_sheet_heading_and_preserves_all_payload_rows() {
        let chunks = split_document_text(
            STRUCTURED_FIXTURE,
            ChunkPolicy {
                max_chars: 48,
                context_chars: 12,
            },
        );

        assert!(chunks.len() > 2);
        assert!(chunks.iter().all(|chunk| chunk.text.chars().count() <= 48));
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].prior_context, None);
        assert!(chunks
            .iter()
            .skip(1)
            .all(|chunk| chunk.text.starts_with("=== Sheet:")));
        for row in [
            "row-one-12345",
            "row-two-12345",
            "row-three-12345",
            "other-row-one",
            "other-row-two",
        ] {
            assert_eq!(
                chunks
                    .iter()
                    .map(|chunk| chunk.text.matches(row).count())
                    .sum::<usize>(),
                1,
                "payload row was lost or duplicated: {row}"
            );
        }
    }

    #[test]
    fn recognizes_the_legacy_sheet_prefix_without_requiring_a_trailing_marker() {
        let heading = "=== Sheet: Legacy";
        let text = format!("{heading}\n{}", "row-content\n".repeat(8));

        let chunks = split_document_text(
            &text,
            ChunkPolicy {
                max_chars: 40,
                context_chars: 8,
            },
        );

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.text.starts_with(heading)));
    }

    #[test]
    fn prefers_blank_line_sections_and_tracks_unicode_prior_context_by_character() {
        let text = "甲乙丙\n丁戊己\n\n第二节\n庚辛壬";
        let chunks = split_document_text(
            text,
            ChunkPolicy {
                max_chars: 14,
                context_chars: 4,
            },
        );

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "甲乙丙\n丁戊己\n\n");
        assert_eq!(chunks[1].text, "第二节\n庚辛壬");
        assert_eq!(chunks[1].prior_context.as_deref(), Some("戊己\n\n"));
    }

    #[test]
    fn repeats_active_circuit_range_heading() {
        let heading = "回路 3-5 遥测数据";
        let text = format!(
            "{heading}\n{}",
            (100..112)
                .map(|address| format!("{address}\t参数{address}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let chunks = split_document_text(
            &text,
            ChunkPolicy {
                max_chars: 50,
                context_chars: 10,
            },
        );

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.text.chars().count() <= 50));
        assert!(chunks.iter().all(|chunk| chunk.text.starts_with(heading)));
        for address in 100..112 {
            let row = format!("{address}\t参数{address}");
            assert_eq!(
                chunks
                    .iter()
                    .map(|chunk| chunk.text.matches(&row).count())
                    .sum::<usize>(),
                1
            );
        }
    }

    #[test]
    fn splits_one_overlong_line_on_unicode_scalar_boundaries_without_loss() {
        let payload = "蜂".repeat(75);
        let text = format!("=== Sheet: Main ===\n{payload}");
        let chunks = split_document_text(
            &text,
            ChunkPolicy {
                max_chars: 40,
                context_chars: 8,
            },
        );

        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.text.chars().count() <= 40));
        let reconstructed = chunks
            .iter()
            .flat_map(|chunk| chunk.text.lines())
            .filter(|line| !line.starts_with("=== Sheet:"))
            .collect::<String>();
        assert_eq!(reconstructed, payload);
    }

    #[test]
    fn bisects_only_the_failing_chunk_and_preserves_heading_and_context() {
        let heading = "=== Sheet: Main ===";
        let original_context = "上一块末尾".to_owned();
        let chunk = DocumentChunk {
            index: 7,
            text: format!("{heading}\n{}", "x".repeat(8_900)),
            prior_context: Some(original_context.clone()),
        };
        let policy = ChunkPolicy {
            max_chars: 12_000,
            context_chars: 16,
        };

        let [left, right] = bisect_chunk(&chunk, policy).unwrap();

        assert_eq!(left.index, 7);
        assert_eq!(right.index, 8);
        assert_eq!(
            left.prior_context.as_deref(),
            Some(original_context.as_str())
        );
        assert!(left.text.starts_with(heading));
        assert!(right.text.starts_with(heading));
        assert!(left.text.chars().count() < chunk.text.chars().count());
        assert!(right.text.chars().count() < chunk.text.chars().count());
        assert_eq!(right.prior_context, Some(tail(&left.text, 16)));
        let payload = [left, right]
            .iter()
            .flat_map(|child| child.text.lines())
            .filter(|line| !line.starts_with("=== Sheet:"))
            .collect::<String>();
        assert_eq!(payload, "x".repeat(8_900));
    }

    #[test]
    fn refuses_to_bisect_at_or_below_the_eight_thousand_character_floor() {
        let chunk = DocumentChunk {
            index: 0,
            text: "界".repeat(8_000),
            prior_context: None,
        };

        let error = bisect_chunk(
            &chunk,
            ChunkPolicy {
                max_chars: 30_000,
                context_chars: 3_000,
            },
        )
        .unwrap_err();
        let serialized = serde_json::to_string(&error).unwrap();

        assert!(serialized.contains("\"code\":\"context_too_large\""));
    }

    fn tail(value: &str, count: usize) -> String {
        let characters = value.chars().collect::<Vec<_>>();
        characters[characters.len().saturating_sub(count)..]
            .iter()
            .collect()
    }
}
