use crate::{error::AppError, naming::NamingCatalog};
use csv::{ReaderBuilder, WriterBuilder};
use regex::Regex;
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

pub const CSV_HEADER: &str =
    "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SanitizedCsv {
    pub csv: String,
    pub warnings: Vec<String>,
    pub diagnostics: CsvDiagnostics,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CsvDiagnostics {
    pub valid_records: usize,
    pub repaired_missing_unit: usize,
    pub rejected_records: usize,
    pub column_counts: BTreeMap<usize, usize>,
}

impl CsvDiagnostics {
    pub fn summary(&self) -> String {
        let columns = self
            .column_counts
            .iter()
            .map(|(width, count)| format!("{width}列={count}"))
            .collect::<Vec<_>>()
            .join("、");
        format!(
            "有效记录 {}；修复缺失 unit {}；拒绝记录 {}；列数分布 {}",
            self.valid_records,
            self.repaired_missing_unit,
            self.rejected_records,
            if columns.is_empty() { "无" } else { &columns }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedCsv {
    pub csv: String,
    pub warnings: Vec<String>,
}

const CIRCUIT_NUMBER: &str = "0-9一二三四五六七八九十百";
const CONCRETE_CIRCUIT_NUMBER: &str = "0-9一二三四五六七八九十";

static CONCRETE_CIRCUIT_PREFIXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        format!(r"(?i)^CH\s*(?P<number>[{CONCRETE_CIRCUIT_NUMBER}]+)(?P<rest>.*)$"),
        format!(r"(?i)^Channel\s*(?P<number>[{CONCRETE_CIRCUIT_NUMBER}]+)(?P<rest>.*)$"),
        format!(r"^第\s*(?P<number>[{CONCRETE_CIRCUIT_NUMBER}]+)\s*(?:路|回路)(?P<rest>.*)$"),
        format!(r"^(?P<number>[{CONCRETE_CIRCUIT_NUMBER}]+)\s*号回路(?P<rest>.*)$"),
        format!(r"^回路\s*(?P<number>[{CONCRETE_CIRCUIT_NUMBER}]+)(?P<rest>.*)$"),
    ]
    .into_iter()
    .map(|pattern| Regex::new(&pattern).expect("valid circuit prefix regex"))
    .collect()
});

static CIRCUIT_RANGE_HEADINGS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        format!(
            r"(?i)^(?:回路\s*)?(?:第\s*)?[{CIRCUIT_NUMBER}]+\s*(?:路|回路)?\s*(?:-|~|～|—|–|至|到)\s*(?:第\s*)?[{CIRCUIT_NUMBER}]+\s*(?:路|回路)?(?:\s*(?:遥测)?(?:数据|寄存器|参数))?$"
        ),
        format!(r"(?i)^CH\s*[{CIRCUIT_NUMBER}]+\s*(?:-|~|～|—|–|至|到)\s*(?:CH\s*)?[{CIRCUIT_NUMBER}]+(?:\s+(?:Registers?|Data|Parameters?))?$"),
        format!(r"(?i)^Channel\s*[{CIRCUIT_NUMBER}]+\s*(?:-|~|～|—|–|to)\s*(?:Channel\s*)?[{CIRCUIT_NUMBER}]+(?:\s+(?:Registers?|Data|Parameters?))?$"),
    ]
    .into_iter()
    .map(|pattern| Regex::new(&pattern).expect("valid circuit range regex"))
    .collect()
});

static AMBIGUOUS_CIRCUIT_RANGE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^(?:(?:CH|Channel)\s*[{CIRCUIT_NUMBER}]+\s*(?:-|~|～|—|–|至|到|to)|(?:回路\s*)?(?:第\s*)?[{CIRCUIT_NUMBER}]+\s*(?:路|回路)?\s*(?:-|~|～|—|–|至|到))"
    ))
    .expect("valid ambiguous circuit range regex")
});

pub fn parse_circuit_parameter_name(name: &str) -> Option<(u32, String)> {
    let candidate = strip_doc_prefixes(name);

    for pattern in CONCRETE_CIRCUIT_PREFIXES.iter() {
        let captures = match pattern.captures(candidate) {
            Some(captures) => captures,
            None => continue,
        };
        let rest = captures.name("rest")?.as_str();
        if starts_range_separator(rest) {
            return None;
        }
        let attribute = rest.trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '_' | ':' | '-' | '：')
        });
        let number = parse_circuit_number(captures.name("number")?.as_str())?;
        if !attribute.is_empty() {
            return Some((number, attribute.to_owned()));
        }
    }
    None
}

pub fn is_circuit_range_heading(line: &str) -> bool {
    let line = line.trim();
    CIRCUIT_RANGE_HEADINGS
        .iter()
        .any(|pattern| pattern.is_match(line))
}

pub fn sanitize_csv(input: &str, catalog: &NamingCatalog) -> Result<SanitizedCsv, AppError> {
    let input = strip_outer_markdown_fence(input);
    let mut reader = ReaderBuilder::new()
        .flexible(true)
        .has_headers(false)
        .from_reader(input.as_bytes());
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    let mut diagnostics = CsvDiagnostics::default();

    for record in reader.records() {
        let record = match record {
            Ok(record) => record,
            Err(_) => {
                warnings.push("忽略了无法解析的 CSV 记录。".into());
                diagnostics.rejected_records += 1;
                continue;
            }
        };
        let mut row = record
            .iter()
            .map(str::trim)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if row.is_empty() || is_header(&row) || is_legacy_comment(&row) {
            continue;
        }
        *diagnostics.column_counts.entry(row.len()).or_default() += 1;
        if repair_missing_unit(&mut row) {
            warnings.push("检测到缺少 unit 空列的 11 列记录，已安全修复。".into());
            diagnostics.repaired_missing_unit += 1;
        }
        if row.len() != 12 || row.first().is_none_or(|id| id.parse::<u64>().is_err()) {
            warnings.push("忽略了格式损坏的 CSV 记录。".into());
            diagnostics.rejected_records += 1;
            continue;
        }
        normalize_row(&mut row, catalog, &mut warnings);
        rows.push(row);
        diagnostics.valid_records += 1;
    }

    Ok(SanitizedCsv {
        csv: write_rows(&rows, true),
        warnings,
        diagnostics,
    })
}

fn repair_missing_unit(row: &mut Vec<String>) -> bool {
    if row.len() != 11
        || row[0].parse::<u64>().is_err()
        || row[1]
            .parse::<u32>()
            .ok()
            .filter(|group| *group >= 1)
            .is_none()
        || row[2].is_empty()
        || row[4]
            .parse::<u32>()
            .ok()
            .filter(|reg_type| (1..=12).contains(reg_type))
            .is_none()
        || [5, 6, 7, 8, 10]
            .into_iter()
            .any(|index| row[index].parse::<u32>().is_err())
        || row[8] != "3"
    {
        return false;
    }
    let Some(address) = parse_register_address(&row[3]) else {
        return false;
    };
    row[3] = address.to_string();
    row.insert(3, String::new());
    true
}

fn parse_register_address(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some(hex) = value.strip_suffix('H').or_else(|| value.strip_suffix('h')) {
        return u64::from_str_radix(hex, 16).ok();
    }
    value.parse().ok()
}

pub fn merge_csv_results(results: &[SanitizedCsv]) -> MergedCsv {
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    let mut warnings = Vec::new();

    for result in results {
        warnings.extend(result.warnings.iter().cloned());
        let mut reader = ReaderBuilder::new()
            .flexible(true)
            .from_reader(result.csv.as_bytes());
        for record in reader.records().flatten() {
            let row = record
                .iter()
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if row.len() != 12 || is_header(&row) || row[0].parse::<u64>().is_err() {
                continue;
            }
            let key = row[1..].to_vec();
            if seen.insert(key) {
                let order = records.len();
                records.push(Record { row, order });
            }
        }
    }

    records.sort_by_key(|record| {
        let (address, width, _) = record_info(&record.row);
        (
            address.is_none(),
            address.unwrap_or_default(),
            width,
            record.order,
        )
    });

    let mut retained = Vec::new();
    let mut occupied: Vec<(u64, u64, u32)> = Vec::new();
    for record in records {
        let (Some(address), width, reg_type) = record_info(&record.row) else {
            retained.push(record.row);
            continue;
        };
        let end = address + width - 1;
        let conflicts = occupied
            .iter()
            .filter(|(existing_start, existing_end, _)| {
                address <= *existing_end && end >= *existing_start
            })
            .collect::<Vec<_>>();
        let shares_bit_register = !conflicts.is_empty()
            && conflicts.iter().all(|(existing_start, _, existing_type)| {
                address == *existing_start && is_bit_type(reg_type) && is_bit_type(*existing_type)
            });
        if !conflicts.is_empty() && !shares_bit_register {
            warnings.push(format!(
                "寄存器地址 {address} 与已保留记录占用冲突，已保留先排序的记录。"
            ));
            continue;
        }
        occupied.push((address, end, reg_type));
        retained.push(record.row);
    }

    for (index, row) in retained.iter_mut().enumerate() {
        row[0] = (index + 1).to_string();
    }
    MergedCsv {
        csv: write_rows(&retained, true),
        warnings,
    }
}

#[derive(Debug)]
struct Record {
    row: Vec<String>,
    order: usize,
}

fn normalize_row(row: &mut [String], catalog: &NamingCatalog, warnings: &mut Vec<String>) {
    if row[1]
        .parse::<u32>()
        .ok()
        .filter(|group| *group >= 1)
        .is_none()
    {
        row[1] = "1".into();
    }

    let original_name = row[2].clone();
    let Some((circuit, attribute)) = parse_circuit_parameter_name(&original_name) else {
        if original_name.is_empty() || strip_doc_prefixes(&original_name).is_empty() {
            warnings.push("参数名为空，已保持原样。".into());
        } else if looks_like_ambiguous_circuit_name(&original_name) {
            warnings.push(format!(
                "无法从参数名“{original_name}”提取完整回路属性，已保持原样。"
            ));
        } else {
            row[2] = canonical_or_doc(&original_name, catalog);
        }
        return;
    };

    row[1] = circuit.to_string();
    row[2] = canonical_or_doc(&attribute, catalog);
}

fn canonical_or_doc(name: &str, catalog: &NamingCatalog) -> String {
    let name = strip_doc_prefixes(name);
    if let Some(entry) = catalog
        .entries
        .iter()
        .find(|entry| entry.code.eq_ignore_ascii_case(name))
    {
        return entry.code.clone();
    }
    let name = name.trim_start_matches("DOC_").trim();
    format!("DOC_{name}")
}

fn write_rows(rows: &[Vec<String>], include_header: bool) -> String {
    let mut writer = WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    if include_header {
        writer
            .write_record(CSV_HEADER.split(','))
            .expect("CSV header is valid");
    }
    for row in rows {
        writer
            .write_record(row)
            .expect("sanitized row is valid CSV");
    }
    let bytes = writer.into_inner().expect("in-memory CSV write succeeds");
    String::from_utf8(bytes)
        .expect("CSV writer emits UTF-8")
        .trim_end()
        .to_owned()
}

fn is_header(row: &[String]) -> bool {
    row.len() == 12 && row.iter().map(String::as_str).eq(CSV_HEADER.split(','))
}

fn is_legacy_comment(row: &[String]) -> bool {
    row.len() == 12 && row[0] == "流水号" && row[1] == "组号" && row[2] == "参数名"
}

fn starts_range_separator(rest: &str) -> bool {
    rest.trim_start().starts_with(['-', '~', '～', '—', '–'])
        || rest.trim_start().starts_with("至")
        || rest.trim_start().starts_with("到")
        || rest.trim_start().to_ascii_lowercase().starts_with("to")
}

fn starts_with_concrete_prefix(name: &str) -> bool {
    let candidate = strip_doc_prefixes(name);
    CONCRETE_CIRCUIT_PREFIXES
        .iter()
        .any(|pattern| pattern.is_match(candidate))
}

fn looks_like_ambiguous_circuit_name(name: &str) -> bool {
    let candidate = strip_doc_prefixes(name);
    starts_with_concrete_prefix(candidate) || AMBIGUOUS_CIRCUIT_RANGE_PREFIX.is_match(candidate)
}

fn strip_doc_prefixes(name: &str) -> &str {
    let mut candidate = name.trim();
    while let Some(without_prefix) = candidate.strip_prefix("DOC_") {
        candidate = without_prefix.trim_start();
    }
    candidate
}

fn parse_circuit_number(value: &str) -> Option<u32> {
    if let Ok(number) = value.parse::<u32>() {
        return (number >= 1).then_some(number);
    }
    let digits = |character| match character {
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    };
    if value == "十" {
        return Some(10);
    }
    if let Some((tens, ones)) = value.split_once('十') {
        if tens.chars().count() > 1 || ones.chars().count() > 1 || ones.contains('十') {
            return None;
        }
        let tens = if tens.is_empty() {
            Some(1)
        } else {
            digits(tens.chars().next()?)
        }?;
        let ones = if ones.is_empty() {
            Some(0)
        } else {
            digits(ones.chars().next()?)
        }?;
        return Some(tens * 10 + ones);
    }
    let mut characters = value.chars();
    let digit = digits(characters.next()?)?;
    characters.next().is_none().then_some(digit)
}

fn strip_outer_markdown_fence(input: &str) -> &str {
    let leading = input.len() - input.trim_start().len();
    let trimmed_start = &input[leading..];
    if !trimmed_start.starts_with("```") {
        return input;
    }
    let Some(first_line_end) = trimmed_start.find('\n') else {
        return input;
    };
    let body_start = leading + first_line_end + 1;
    let after_opening = &input[body_start..];
    let trailing = after_opening.trim_end();
    let Some(closing_start) = trailing.strip_suffix("```").map(|body| body.len()) else {
        return input;
    };
    let body_before_fence = &after_opening[..closing_start];
    let Some(body) = body_before_fence
        .strip_suffix("\r\n")
        .or_else(|| body_before_fence.strip_suffix('\n'))
        .or_else(|| body_before_fence.strip_suffix('\r'))
    else {
        return input;
    };
    body
}

fn record_info(row: &[String]) -> (Option<u64>, u64, u32) {
    let address = row.get(4).and_then(|value| value.parse::<u64>().ok());
    let reg_type = row
        .get(5)
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_default();
    let width = match reg_type {
        8 | 9 | 12 => 2,
        10 | 11 => 4,
        _ => 1,
    };
    (address, width, reg_type)
}

fn is_bit_type(reg_type: u32) -> bool {
    matches!(reg_type, 1..=3)
}

#[cfg(test)]
mod tests {
    use super::{
        is_circuit_range_heading, merge_csv_results, parse_circuit_parameter_name, sanitize_csv,
        CSV_HEADER,
    };
    use crate::naming::{NamingCatalog, NamingEntry};
    use std::collections::HashSet;

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/fixtures/register_csv"
    );

    fn catalog() -> NamingCatalog {
        NamingCatalog {
            entries: vec![NamingEntry {
                code: "Ua".into(),
                meaning: "A相电压".into(),
            }],
            names: HashSet::from(["ua".into()]),
            reference: String::new(),
        }
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{FIXTURES}/{name}")).expect("fixture should load")
    }

    #[test]
    fn parses_only_explicit_circuit_prefixes_including_chinese_numerals() {
        assert_eq!(
            parse_circuit_parameter_name("DOC_CH 6 重合闸时间"),
            Some((6, "重合闸时间".into()))
        );
        assert_eq!(
            parse_circuit_parameter_name("DOC_第三回路状态"),
            Some((3, "状态".into()))
        );
        assert_eq!(
            parse_circuit_parameter_name("DOC_回路12预警值"),
            Some((12, "预警值".into()))
        );
        assert_eq!(parse_circuit_parameter_name("DOC_CH1-CH16重合闸参数"), None);
        assert_eq!(parse_circuit_parameter_name("DOC_CH2"), None);
        assert_eq!(
            parse_circuit_parameter_name("DOC_2号回路实时剩余电流值"),
            Some((2, "实时剩余电流值".into()))
        );
        assert_eq!(
            parse_circuit_parameter_name("DOC_第3路保护开关"),
            Some((3, "保护开关".into()))
        );
        assert_eq!(parse_circuit_parameter_name("DOC_第一二路状态"), None);
    }

    #[test]
    fn identifies_standalone_range_headings_without_matching_prose() {
        for heading in [
            "回路 1-2 遥测数据",
            "回路3至5",
            "第一至第三回路",
            "CH1-CH8 Registers",
            "CH1至CH8 Registers",
            "Channel 2-4",
        ] {
            assert!(is_circuit_range_heading(heading), "{heading}");
        }
        assert!(!is_circuit_range_heading("本设备支持 CH1-CH8 共八个回路"));
    }

    #[test]
    fn sanitizes_fixture_with_quotes_headers_doc_prefix_and_warnings() {
        let result =
            sanitize_csv(&fixture("sanitize.input.csv"), &catalog()).expect("CSV should sanitize");

        assert_eq!(result.csv, fixture("sanitize.expected.csv").trim_end());
        assert_eq!(
            result.warnings.len(),
            2,
            "range and empty concrete prefix must be reported"
        );
    }

    #[test]
    fn preserves_ambiguous_names_unchanged_instead_of_forcing_a_group() {
        let input = format!("{CSV_HEADER}\n1,7,DOC_CH1-CH16重合闸参数,,225,6,1,0,0,3,,0\n2,4,DOC_CH2,,226,6,1,0,0,3,,0\n3,8,DOC_第1至第16回路保护参数,,227,6,1,0,0,3,,0");
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");
        let rows: Vec<_> = result.csv.lines().collect();

        assert_eq!(rows[1], "1,7,DOC_CH1-CH16重合闸参数,,225,6,1,0,0,3,,0");
        assert_eq!(rows[2], "2,4,DOC_CH2,,226,6,1,0,0,3,,0");
        assert_eq!(rows[3], "3,8,DOC_第1至第16回路保护参数,,227,6,1,0,0,3,,0");
        assert_eq!(result.warnings.len(), 3);
    }

    #[test]
    fn reads_headerless_csv_and_warns_for_a_first_damaged_record() {
        let result = sanitize_csv(&fixture("headerless-first-damaged.input.csv"), &catalog())
            .expect("CSV should sanitize");

        assert_eq!(
            result.csv,
            fixture("headerless-first-damaged.expected.csv").trim_end()
        );
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn preserves_outer_fenced_crlf_multiline_quoted_data_and_legitimate_doc_group_name() {
        let input = format!(
            "```csv\r\n{CSV_HEADER}\r\n1,1,\"DOC_Alarm\r\n组号\",,100,6,1,0,0,3,,0\r\n2,1,DOC_组号,,101,6,1,0,0,3,,0\r\n```\r\n"
        );
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(result.csv.as_bytes());
        let rows = reader
            .records()
            .collect::<Result<Vec<_>, _>>()
            .expect("output CSV");

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].get(2), Some("DOC_Alarm\r\n组号"));
        assert_eq!(rows[2].get(2), Some("DOC_组号"));
    }

    #[test]
    fn canonicalizes_every_non_ambiguous_doc_name_and_warns_for_empty_name() {
        let input = format!("{CSV_HEADER}\n1,1,DOC_Ua,,100,6,1,0,0,3,,0\n2,1,DOC_DOC_Alarm,,101,6,1,0,0,3,,0\n3,invalid,DOC_公共参数,,102,6,1,0,0,3,,0\n4,1,,,103,6,1,0,0,3,,0");
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");
        let rows: Vec<_> = result.csv.lines().collect();

        assert_eq!(rows[1], "1,1,Ua,,100,6,1,0,0,3,,0");
        assert_eq!(rows[2], "2,1,DOC_Alarm,,101,6,1,0,0,3,,0");
        assert_eq!(rows[3], "3,1,DOC_公共参数,,102,6,1,0,0,3,,0");
        assert_eq!(rows[4], "4,1,,,103,6,1,0,0,3,,0");
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn keeps_empty_doc_prefix_chains_unchanged_with_warnings() {
        let input =
            format!("{CSV_HEADER}\n1,1,DOC_,,100,6,1,0,0,3,,0\n2,1,DOC_DOC_,,101,6,1,0,0,3,,0");
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");
        let rows: Vec<_> = result.csv.lines().collect();

        assert_eq!(rows[1], "1,1,DOC_,,100,6,1,0,0,3,,0");
        assert_eq!(rows[2], "2,1,DOC_DOC_,,101,6,1,0,0,3,,0");
        assert_eq!(result.warnings.len(), 2);
        assert_eq!(result.diagnostics.valid_records, 2);
        assert_eq!(result.diagnostics.repaired_missing_unit, 0);
        assert_eq!(result.diagnostics.rejected_records, 0);
        assert_eq!(result.diagnostics.column_counts.get(&12), Some(&2));
    }

    #[test]
    fn defaults_each_legacy_invalid_group_form_to_one() {
        let input = format!(
            "{CSV_HEADER}\n1,,Ua,,100,6,1,0,0,3,,0\n2,-1,Ua,,101,6,1,0,0,3,,0\n3,CH2,Ua,,102,6,1,0,0,3,,0"
        );
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");
        let rows: Vec<_> = result.csv.lines().collect();

        assert_eq!(rows[1], "1,1,Ua,,100,6,1,0,0,3,,0");
        assert_eq!(rows[2], "2,1,Ua,,101,6,1,0,0,3,,0");
        assert_eq!(rows[3], "3,1,Ua,,102,6,1,0,0,3,,0");
    }

    #[test]
    fn warns_for_under_and_over_sized_rows() {
        let input = format!("{CSV_HEADER}\n1,1,TooShort\n2,1,TooLong,,100,6,1,0,0,3,,0,extra");
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");

        assert_eq!(result.csv, CSV_HEADER);
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn repairs_only_the_known_eleven_column_shape_with_a_missing_unit() {
        let input = format!(
            "{CSV_HEADER}\n1,1,DOC_单三相类别,0x0800,6,1,0,0,3,,0\n2,1,Ua,0301H,6,1,1,1,3,,0"
        );

        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");

        assert_eq!(
            result.csv,
            format!(
                "{CSV_HEADER}\n1,1,DOC_单三相类别,,2048,6,1,0,0,3,,0\n2,1,Ua,,769,6,1,1,1,3,,0"
            )
        );
        assert_eq!(result.warnings.len(), 2);
    }

    #[test]
    fn rejects_an_ambiguous_eleven_column_row_instead_of_guessing() {
        let input = format!("{CSV_HEADER}\n1,1,Ua,not-an-address,6,1,0,0,3,,0");

        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");

        assert_eq!(result.csv, CSV_HEADER);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.diagnostics.valid_records, 0);
        assert_eq!(result.diagnostics.repaired_missing_unit, 0);
        assert_eq!(result.diagnostics.rejected_records, 1);
        assert_eq!(result.diagnostics.column_counts.get(&11), Some(&1));
    }

    #[test]
    fn normalizes_repeated_doc_prefixes_to_one_after_a_concrete_circuit_prefix() {
        let input = format!("{CSV_HEADER}\n1,1,DOC_DOC_CH6重合闸开关,,237,6,1,0,0,3,,0");
        let result = sanitize_csv(&input, &catalog()).expect("CSV should sanitize");

        assert_eq!(
            result.csv.lines().nth(1),
            Some("1,6,DOC_重合闸开关,,237,6,1,0,0,3,,0")
        );
    }

    #[test]
    fn merges_fixtures_deterministically_with_bit_and_register_conflict_rules() {
        let first = sanitize_csv(&fixture("merge-first.input.csv"), &catalog())
            .expect("first should sanitize");
        let second = sanitize_csv(&fixture("merge-second.input.csv"), &catalog())
            .expect("second should sanitize");
        let merged = merge_csv_results(&[first, second]);

        assert_eq!(merged.csv, fixture("merge.expected.csv").trim_end());
    }

    #[test]
    fn returns_only_header_for_empty_merge() {
        assert_eq!(merge_csv_results(&[]).csv, CSV_HEADER);
    }
}
