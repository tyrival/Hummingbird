use crate::error::{AppError, ErrorCode};
use crate::extraction::DocumentKind;
use calamine::{open_workbook_auto, Data, ExcelDateTime, Reader};
use chardetng::EncodingDetector;
use csv::ReaderBuilder;
use encoding_rs::{GB18030, UTF_16BE, UTF_16LE, WINDOWS_1252};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(super) fn extract(path: &Path, kind: DocumentKind) -> Result<String, AppError> {
    match kind {
        DocumentKind::Xls | DocumentKind::Xlsx => extract_workbook(path),
        DocumentKind::Csv => extract_csv(path),
        _ => Err(AppError::new(ErrorCode::UnsupportedFormat)),
    }
}

fn extract_workbook(path: &Path) -> Result<String, AppError> {
    let mut workbook = open_workbook_auto(path)
        .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;
    let sheet_names = workbook.sheet_names().to_owned();
    let mut sections = Vec::new();

    for sheet_name in sheet_names {
        let range = workbook
            .worksheet_range(&sheet_name)
            .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;
        let rows = range
            .rows()
            .map(|row| row.iter().map(format_cell).collect())
            .collect();
        let lines = normalize_rows(rows);
        if lines.iter().any(|line| !line.is_empty()) {
            sections.push(format!("=== Sheet: {sheet_name} ===\n{}", lines.join("\n")));
        }
    }

    Ok(sections.join("\n\n"))
}

fn format_cell(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::Bool(true) => "TRUE".to_owned(),
        Data::Bool(false) => "FALSE".to_owned(),
        Data::DateTime(value) => format_excel_datetime(value),
        Data::DateTimeIso(value) => value.replacen('T', " ", 1),
        Data::DurationIso(value) => value.to_owned(),
        Data::Float(value) if value.is_finite() && value.fract() == 0.0 => {
            format!("{value:.0}")
        }
        _ => value.to_string(),
    }
}

fn format_excel_datetime(value: &ExcelDateTime) -> String {
    const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1_000;

    if value.is_duration() {
        let total_millis = (value.as_f64() * MILLIS_PER_DAY as f64).round() as i64;
        let days = total_millis.div_euclid(MILLIS_PER_DAY);
        let remainder = total_millis.rem_euclid(MILLIS_PER_DAY);
        let hours = remainder / 3_600_000;
        let minutes = remainder % 3_600_000 / 60_000;
        let seconds = remainder % 60_000 / 1_000;
        let millis = remainder % 1_000;
        let clock = if millis == 0 {
            format!("{hours}:{minutes:02}:{seconds:02}")
        } else {
            format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}000")
        };
        return match days {
            0 => clock,
            1 => format!("1 day, {clock}"),
            _ => format!("{days} days, {clock}"),
        };
    }

    let (year, month, day, hour, minute, second, millis) = value.to_ymd_hms_milli();
    let clock = format_clock(hour, minute, second, millis);
    if (0.0..1.0).contains(&value.as_f64()) {
        clock
    } else {
        format!("{year:04}-{month:02}-{day:02} {clock}")
    }
}

fn format_clock(hour: u8, minute: u8, second: u8, millis: u16) -> String {
    if millis == 0 {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{hour:02}:{minute:02}:{second:02}.{millis:03}000")
    }
}

fn extract_csv(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let content = decode_csv(&bytes)?;
    let delimiter = detect_delimiter(&content);
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(content.as_bytes());
    let rows = reader
        .records()
        .map(|record| {
            record
                .map(|record| record.iter().map(str::to_owned).collect())
                .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))
        })
        .collect::<Result<Vec<Vec<String>>, AppError>>()?;
    let lines = normalize_rows(rows);

    if lines.iter().any(|line| !line.is_empty()) {
        Ok(format!("=== CSV ===\n{}", lines.join("\n")))
    } else {
        Ok(String::new())
    }
}

fn decode_csv(bytes: &[u8]) -> Result<String, AppError> {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8(bytes.to_vec())
            .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (text, _, had_errors) = UTF_16LE.decode(bytes);
        return (!had_errors)
            .then(|| text.into_owned())
            .ok_or_else(|| AppError::new(ErrorCode::ParseFailed));
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let (text, _, had_errors) = UTF_16BE.decode(bytes);
        return (!had_errors)
            .then(|| text.into_owned())
            .ok_or_else(|| AppError::new(ErrorCode::ParseFailed));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_owned());
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
        Err(AppError::new(ErrorCode::ParseFailed))
    } else {
        Ok(text.into_owned())
    }
}

fn detect_delimiter(content: &str) -> u8 {
    let mut best = delimiter_score(content, b',');
    for delimiter in *b";\t|" {
        let candidate = delimiter_score(content, delimiter);
        if candidate.is_better_than(&best) {
            best = candidate;
        }
    }
    best.delimiter
}

#[derive(Clone, Copy, Debug)]
struct DelimiterScore {
    delimiter: u8,
    rows: usize,
    structured_rows: usize,
    mode_columns: usize,
    consistent_rows: usize,
    variance: usize,
    quote_boundary_errors: usize,
    unquoted_delimiters: usize,
    quoted_fields: usize,
}

impl DelimiterScore {
    fn is_better_than(&self, other: &Self) -> bool {
        match (self.mode_columns > 1).cmp(&(other.mode_columns > 1)) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Greater,
        }
        match ratio_cmp(
            self.structured_rows,
            self.rows,
            other.structured_rows,
            other.rows,
        ) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Greater,
        }
        match ratio_cmp(
            self.consistent_rows,
            self.rows,
            other.consistent_rows,
            other.rows,
        ) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Greater,
        }
        match ratio_cmp(self.variance, self.rows, other.variance, other.rows) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Less,
        }
        match self.mode_columns.cmp(&other.mode_columns) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Greater,
        }
        match self.quote_boundary_errors.cmp(&other.quote_boundary_errors) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Less,
        }
        match self.unquoted_delimiters.cmp(&other.unquoted_delimiters) {
            Ordering::Equal => {}
            ordering => return ordering == Ordering::Greater,
        }
        self.quoted_fields > other.quoted_fields
    }
}

fn delimiter_score(content: &str, delimiter: u8) -> DelimiterScore {
    let quote_score = scan_rfc4180_boundaries(content, delimiter as char);
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(content.as_bytes());
    let mut column_counts = Vec::new();
    for record in reader.records().take(64) {
        let Ok(record) = record else {
            return DelimiterScore {
                delimiter,
                rows: 1,
                structured_rows: 0,
                mode_columns: 0,
                consistent_rows: 0,
                variance: usize::MAX,
                quote_boundary_errors: quote_score.boundary_errors,
                unquoted_delimiters: quote_score.unquoted_delimiters,
                quoted_fields: quote_score.quoted_fields,
            };
        };
        if record.len() > 1 || record.iter().any(|field| !field.trim().is_empty()) {
            column_counts.push(record.len());
        }
    }

    let mut frequencies = BTreeMap::new();
    for columns in &column_counts {
        *frequencies.entry(*columns).or_insert(0_usize) += 1;
    }
    let mode_columns = frequencies
        .into_iter()
        .max_by_key(|(_, frequency)| *frequency)
        .map(|(columns, _)| columns)
        .unwrap_or(0);
    let consistent_rows = column_counts
        .iter()
        .filter(|columns| **columns == mode_columns)
        .count();
    let structured_rows = column_counts.iter().filter(|columns| **columns > 1).count();
    let variance = column_counts
        .iter()
        .map(|columns| columns.abs_diff(mode_columns).pow(2))
        .sum();

    DelimiterScore {
        delimiter,
        rows: column_counts.len(),
        structured_rows,
        mode_columns,
        consistent_rows,
        variance,
        quote_boundary_errors: quote_score.boundary_errors,
        unquoted_delimiters: quote_score.unquoted_delimiters,
        quoted_fields: quote_score.quoted_fields,
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct QuoteBoundaryScore {
    boundary_errors: usize,
    unquoted_delimiters: usize,
    quoted_fields: usize,
}

#[derive(Clone, Copy, Debug)]
enum CsvFieldState {
    FieldStart,
    Unquoted,
    Quoted,
    AfterQuote,
}

fn scan_rfc4180_boundaries(content: &str, delimiter: char) -> QuoteBoundaryScore {
    let mut score = QuoteBoundaryScore::default();
    let mut state = CsvFieldState::FieldStart;
    let mut completed_records = 0_usize;
    let mut characters = content.chars().peekable();

    while let Some(character) = characters.next() {
        match state {
            CsvFieldState::FieldStart => match character {
                '"' => {
                    score.quoted_fields += 1;
                    state = CsvFieldState::Quoted;
                }
                '\r' | '\n' => {
                    if character == '\n' {
                        completed_records += 1;
                    }
                }
                value if value == delimiter => score.unquoted_delimiters += 1,
                _ => state = CsvFieldState::Unquoted,
            },
            CsvFieldState::Unquoted => match character {
                '"' => score.boundary_errors += 1,
                '\r' | '\n' => {
                    state = CsvFieldState::FieldStart;
                    if character == '\n' {
                        completed_records += 1;
                    }
                }
                value if value == delimiter => {
                    score.unquoted_delimiters += 1;
                    state = CsvFieldState::FieldStart;
                }
                _ => {}
            },
            CsvFieldState::Quoted => {
                if character == '"' {
                    if characters.peek() == Some(&'"') {
                        characters.next();
                    } else {
                        state = CsvFieldState::AfterQuote;
                    }
                }
            }
            CsvFieldState::AfterQuote => match character {
                '\r' | '\n' => {
                    state = CsvFieldState::FieldStart;
                    if character == '\n' {
                        completed_records += 1;
                    }
                }
                value if value == delimiter => {
                    score.unquoted_delimiters += 1;
                    state = CsvFieldState::FieldStart;
                }
                _ => {
                    score.boundary_errors += 1;
                    state = CsvFieldState::Unquoted;
                }
            },
        }

        if completed_records >= 64 {
            break;
        }
    }

    if matches!(state, CsvFieldState::Quoted) {
        score.boundary_errors += 1;
    }
    score
}

fn ratio_cmp(left: usize, left_total: usize, right: usize, right_total: usize) -> Ordering {
    let left_total = left_total.max(1);
    let right_total = right_total.max(1);
    (left as u128 * right_total as u128).cmp(&(right as u128 * left_total as u128))
}

fn normalize_rows(rows: Vec<Vec<String>>) -> Vec<String> {
    let mut lines: Vec<String> = rows
        .into_iter()
        .map(|row| {
            let mut cells: Vec<String> = row.into_iter().map(|cell| clean_cell(&cell)).collect();
            while cells.last().is_some_and(String::is_empty) {
                cells.pop();
            }
            cells.join("\t")
        })
        .collect();

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    lines
}

fn clean_cell(value: &str) -> String {
    value.replace(['\t', '\r', '\n'], " ").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::{extract, format_cell};
    use crate::extraction::DocumentKind;
    use calamine::Data;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/documents")
            .join(name)
    }

    #[test]
    fn xlsx_preserves_sheet_titles_hidden_sheets_internal_cells_and_blank_rows() {
        let text = extract(&fixture("register.xlsx"), DocumentKind::Xlsx)
            .expect("XLSX fixture should parse");

        assert_eq!(
            text,
            "=== Sheet: 寄存器 ===\n\
             地址\t名称\t备注\n\
             1\t\tA相电压\n\
             \n\
             2\t电流\n\n\
             === Sheet: 隐藏参数 ===\n\
             地址\t名称\n\
             3\t隐藏电流"
        );
    }

    #[test]
    fn xls_reads_every_sheet_in_workbook_order() {
        let text =
            extract(&fixture("register.xls"), DocumentKind::Xls).expect("XLS fixture should parse");

        assert_eq!(
            text,
            "=== Sheet: Main ===\nAddress\tName\n16\tVoltage\n\n\
             === Sheet: Other ===\nAddress\n17"
        );
    }

    #[test]
    fn xlsx_formats_dates_times_and_durations_like_the_legacy_extractor() {
        let text = extract(&fixture("dates.xlsx"), DocumentKind::Xlsx)
            .expect("date XLSX fixture should parse");

        assert_eq!(
            text,
            "=== Sheet: Dates ===\n\
             Kind\tValue\n\
             Date\t2026-07-19 00:00:00\n\
             DateTime\t2026-07-19 13:45:06\n\
             Time\t13:45:06\n\
             Duration\t1 day, 2:03:00"
        );
    }

    #[test]
    fn formats_iso_datetime_and_duration_variants_without_raw_serial_values() {
        assert_eq!(
            format_cell(&Data::DateTimeIso("2026-07-19T13:45:06".to_owned())),
            "2026-07-19 13:45:06"
        );
        assert_eq!(
            format_cell(&Data::DurationIso("PT26H3M".to_owned())),
            "PT26H3M"
        );
    }

    #[test]
    fn csv_decodes_utf8_bom_and_preserves_internal_cells_and_blank_rows() {
        let text = extract(&fixture("register-utf8-bom.csv"), DocumentKind::Csv)
            .expect("UTF-8 BOM CSV fixture should parse");

        assert_eq!(
            text,
            "=== CSV ===\n\
             地址\t名称\t备注\n\
             1\t\t电压\n\
             \n\
             2\t电流"
        );
    }

    #[test]
    fn csv_decodes_gb18030_and_detects_semicolon_delimiter() {
        let text = extract(&fixture("register-gb18030.csv"), DocumentKind::Csv)
            .expect("GB18030 CSV fixture should parse");

        assert_eq!(text, "=== CSV ===\n地址\t名称\t说明\n2\t电流\t中文备注");
    }

    #[test]
    fn csv_uses_a_compatible_single_byte_encoding_fallback() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("fallback.csv");
        fs::write(&path, b"Address,Name\n1,Caf\xe9\n").expect("fallback fixture should be written");

        let text = extract(&path, DocumentKind::Csv).expect("fallback CSV should parse");

        assert_eq!(text, "=== CSV ===\nAddress\tName\n1\tCafé");
    }

    #[test]
    fn csv_does_not_misread_ambiguous_cp1252_bytes_as_gb18030() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("ambiguous.csv");
        fs::write(&path, b"Address,Name\n1,legacy-\xc0\xa9\n")
            .expect("ambiguous fixture should be written");

        let text = extract(&path, DocumentKind::Csv).expect("ambiguous CSV should parse");

        assert_eq!(text, "=== CSV ===\nAddress\tName\n1\tlegacy-À©");
    }

    #[test]
    fn csv_detects_tab_and_pipe_delimiters_without_splitting_quoted_values() {
        let directory = tempdir().expect("temporary directory should be created");
        let tab_path = directory.path().join("tab.csv");
        fs::write(&tab_path, "Address\tName\n1\tVoltage\n").expect("tab fixture should be written");
        let pipe_path = directory.path().join("pipe.csv");
        fs::write(&pipe_path, "Address|Name\n1|\"A|B\"\n").expect("pipe fixture should be written");

        assert_eq!(
            extract(&tab_path, DocumentKind::Csv).expect("tab CSV should parse"),
            "=== CSV ===\nAddress\tName\n1\tVoltage"
        );
        assert_eq!(
            extract(&pipe_path, DocumentKind::Csv).expect("pipe CSV should parse"),
            "=== CSV ===\nAddress\tName\n1\tA|B"
        );
    }

    #[test]
    fn csv_prefers_consistent_multiline_columns_over_frequent_field_punctuation() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("punctuation.csv");
        fs::write(
            &path,
            "Address;Description;Unit\n\
             1;alpha,beta,gamma,delta,epsilon,zeta,eta,theta,iota,kappa;V\n\
             2;one,two,three,four,five,six,seven,eight;A\n\
             3;red,orange,yellow,green,blue,indigo,violet,black,white,gray,brown;W\n",
        )
        .expect("punctuation fixture should be written");

        let text = extract(&path, DocumentKind::Csv).expect("semicolon CSV should parse");

        assert_eq!(
            text,
            "=== CSV ===\n\
             Address\tDescription\tUnit\n\
             1\talpha,beta,gamma,delta,epsilon,zeta,eta,theta,iota,kappa\tV\n\
             2\tone,two,three,four,five,six,seven,eight\tA\n\
             3\tred,orange,yellow,green,blue,indigo,violet,black,white,gray,brown\tW"
        );
    }

    #[test]
    fn csv_uses_rfc4180_quote_boundaries_to_break_a_structure_tie() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("headerless-quoted.csv");
        fs::write(&path, "1;\"alpha,beta,gamma\";V\n2;\"red,green,blue\";A\n")
            .expect("headerless quoted fixture should be written");

        let text = extract(&path, DocumentKind::Csv).expect("semicolon CSV should parse");

        assert_eq!(
            text,
            "=== CSV ===\n1\talpha,beta,gamma\tV\n2\tred,green,blue\tA"
        );
    }

    #[test]
    fn formats_spreadsheet_booleans_and_integral_floats_like_the_legacy_extractor() {
        assert_eq!(format_cell(&Data::Bool(true)), "TRUE");
        assert_eq!(format_cell(&Data::Bool(false)), "FALSE");
        assert_eq!(format_cell(&Data::Float(16.0)), "16");
        assert_eq!(format_cell(&Data::Float(16.5)), "16.5");
        assert_eq!(format_cell(&Data::Empty), "");
    }
}
