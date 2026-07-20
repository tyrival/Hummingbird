use crate::error::{AppError, ErrorCode};
use chardetng::EncodingDetector;
use encoding_rs::{GB18030, WINDOWS_1252};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;

const CSV_RESOURCE: &str = "t_electric_param.csv";
const MARKDOWN_RESOURCE: &str = "naming-convention.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamingEntry {
    pub code: String,
    pub meaning: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamingCatalog {
    pub entries: Vec<NamingEntry>,
    pub names: HashSet<String>,
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourcePaths {
    pub csv: PathBuf,
    pub markdown: PathBuf,
}

impl ResourcePaths {
    pub fn new(csv: impl Into<PathBuf>, markdown: impl Into<PathBuf>) -> Self {
        Self {
            csv: csv.into(),
            markdown: markdown.into(),
        }
    }

    pub fn development() -> Self {
        let resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        Self::bundled_from(resources)
    }

    pub fn bundled_from(resource_dir: impl Into<PathBuf>) -> Self {
        let resource_dir = resource_dir.into();
        Self::new(
            resource_dir.join(CSV_RESOURCE),
            resource_dir.join(MARKDOWN_RESOURCE),
        )
    }

    pub fn bundled(app: &tauri::AppHandle) -> Result<Self, AppError> {
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
        Ok(Self::bundled_from(resource_dir))
    }
}

/// Loads the authoritative CSV catalog, using the legacy Markdown table only when the CSV is unusable.
pub fn load_naming_catalog(resources: &ResourcePaths) -> Result<NamingCatalog, AppError> {
    if let Ok(entries) = read_csv_entries(&resources.csv) {
        if !entries.is_empty() {
            return Ok(build_catalog(entries));
        }
    }

    match read_markdown_entries(&resources.markdown) {
        Ok(entries) if !entries.is_empty() => Ok(build_catalog(entries)),
        Ok(_) | Err(()) if !resources.csv.exists() && !resources.markdown.exists() => {
            Err(AppError::new(ErrorCode::FileNotFound))
        }
        Err(()) | Ok(_) => Err(AppError::new(ErrorCode::ParseFailed)),
    }
}

fn read_csv_entries(path: &Path) -> Result<Vec<NamingEntry>, ()> {
    let text = decode_resource(&fs::read(path).map_err(|_| ())?)?;
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.as_bytes());
    let headers = reader.headers().map_err(|_| ())?;
    let code_index = headers.iter().position(|header| header == "ParamCode");
    let meaning_index = headers.iter().position(|header| header == "ParamName");
    let (Some(code_index), Some(meaning_index)) = (code_index, meaning_index) else {
        return Err(());
    };

    let mut entries = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|_| ())?;
        let code = record.get(code_index).unwrap_or_default().trim();
        let meaning = record.get(meaning_index).unwrap_or_default().trim();
        merge_entry(&mut entries, code, meaning);
    }
    Ok(entries)
}

fn read_markdown_entries(path: &Path) -> Result<Vec<NamingEntry>, ()> {
    let text = decode_resource(&fs::read(path).map_err(|_| ())?)?;
    let mut entries = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("---") {
            continue;
        }
        let cells: Vec<_> = line
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .collect();
        if cells.len() < 3 {
            continue;
        }
        let code = cells[1];
        if matches!(code, "名称" | "method") || code.starts_with('.') {
            continue;
        }
        let meaning = strip_html(cells[2]);
        merge_entry(&mut entries, code, &meaning);
    }
    Ok(entries)
}

fn decode_resource(bytes: &[u8]) -> Result<String, ()> {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.trim_start_matches('\u{feff}').to_owned());
    }

    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let detected = detector.guess(None, true);
    let encoding = if detected == GB18030 || detected.name() == "GBK" {
        GB18030
    } else {
        WINDOWS_1252
    };
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        Err(())
    } else {
        Ok(text.into_owned())
    }
}

fn merge_entry(entries: &mut Vec<NamingEntry>, code: &str, meaning: &str) {
    if code.is_empty() || meaning.is_empty() {
        return;
    }
    if let Some(entry) = entries.iter_mut().find(|entry| entry.code == code) {
        if !entry.meaning.split(" / ").any(|known| known == meaning) {
            entry.meaning.push_str(" / ");
            entry.meaning.push_str(meaning);
        }
        return;
    }
    entries.push(NamingEntry {
        code: code.into(),
        meaning: meaning.into(),
    });
}

fn strip_html(value: &str) -> String {
    let mut cleaned = String::new();
    let mut in_tag = false;
    for character in value.replace("<br/>", " ").replace("<br>", " ").chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => cleaned.push(character),
            _ => {}
        }
    }
    cleaned.trim().to_owned()
}

fn build_catalog(entries: Vec<NamingEntry>) -> NamingCatalog {
    let names = entries
        .iter()
        .map(|entry| entry.code.to_ascii_lowercase())
        .collect();
    let reference = build_reference(&entries);
    NamingCatalog {
        entries,
        names,
        reference,
    }
}

fn build_reference(entries: &[NamingEntry]) -> String {
    const MAX_LINE_CHARS: usize = 130;
    let mut lines = Vec::new();
    let mut current = String::new();

    for entry in entries {
        let item = format!("{}={}", entry.code, entry.meaning.replace(',', "，"));
        let separator_len = usize::from(!current.is_empty()) * 2;
        if !current.is_empty()
            && current.chars().count() + separator_len + item.chars().count() > MAX_LINE_CHARS
        {
            lines.push(current);
            current = item;
        } else if current.is_empty() {
            current = item;
        } else {
            current.push_str(", ");
            current.push_str(&item);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{load_naming_catalog, NamingEntry, ResourcePaths};
    use std::{fs, path::PathBuf};
    use tempfile::TempDir;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/naming");

    #[test]
    fn reads_gb18030_csv_and_preserves_known_codes() {
        let catalog = load_fixture_catalog("gb18030.csv", "fallback.md");

        assert!(catalog.entries.len() > 100);
        assert!(catalog.entries.contains(&entry("Ua", "A相电压")));
        assert!(catalog
            .entries
            .contains(&entry("FrHigh", "频率输入上限 / 输入频率高于上限")));
    }

    #[test]
    fn reads_utf8_bom_csv_with_reordered_headers() {
        let catalog = load_fixture_catalog("reordered-utf8-bom.csv", "fallback.md");

        assert_eq!(
            catalog.entries,
            vec![entry("Ua", "A相电压"), entry("Ub", "B相电压")]
        );
    }

    #[test]
    fn skips_empty_rows_merges_distinct_meanings_and_preserves_first_seen_order() {
        let catalog = load_fixture_catalog("duplicates-and-empty.csv", "fallback.md");

        assert_eq!(
            catalog.entries,
            vec![
                entry("Ua", "A相电压"),
                entry("FrHigh", "频率输入上限 / 输入频率高于上限"),
            ]
        );
    }

    #[test]
    fn valid_csv_is_authoritative_over_markdown_fallback() {
        let catalog = load_fixture_catalog("authority.csv", "fallback.md");

        assert_eq!(catalog.entries, vec![entry("Ua", "A相电压")]);
        assert!(!catalog.names.contains("oldonly"));
    }

    #[test]
    fn invalid_csv_falls_back_to_markdown_and_derives_case_insensitive_names_once() {
        let catalog = load_fixture_catalog("invalid.csv", "fallback.md");

        assert_eq!(
            catalog.entries,
            vec![entry("Ua", "A相电压"), entry("OldOnly", "旧文档参数")]
        );
        assert_eq!(catalog.names.len(), 2);
        assert!(catalog.names.contains("ua"));
        assert!(catalog.names.contains("oldonly"));
        assert!(catalog.reference.contains("OldOnly=旧文档参数"));
    }

    #[test]
    fn current_development_resources_load_known_codes() {
        let catalog = load_naming_catalog(&ResourcePaths::development()).unwrap();

        assert!(catalog.names.contains("ua"));
        assert!(catalog.names.contains("frhigh"));
    }

    #[test]
    fn bundled_paths_resolve_the_same_catalog_when_resources_are_copied() {
        let sandbox = TempDir::new().unwrap();
        let resource_dir = sandbox.path().join("resources");
        fs::create_dir(&resource_dir).unwrap();
        copy_fixture("authority.csv", resource_dir.join("t_electric_param.csv"));
        copy_fixture("fallback.md", resource_dir.join("naming-convention.md"));

        let catalog = load_naming_catalog(&ResourcePaths::bundled_from(resource_dir)).unwrap();

        assert_eq!(catalog.entries, vec![entry("Ua", "A相电压")]);
    }

    fn load_fixture_catalog(csv: &str, markdown: &str) -> super::NamingCatalog {
        load_naming_catalog(&ResourcePaths::new(
            fixture_path(csv),
            fixture_path(markdown),
        ))
        .unwrap()
    }

    fn copy_fixture(name: &str, destination: PathBuf) {
        fs::copy(fixture_path(name), destination).unwrap();
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(FIXTURES).join(name)
    }

    fn entry(code: &str, meaning: &str) -> NamingEntry {
        NamingEntry {
            code: code.into(),
            meaning: meaning.into(),
        }
    }
}
