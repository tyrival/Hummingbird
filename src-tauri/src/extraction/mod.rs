use crate::error::{AppError, ErrorCode};
use std::fs::Metadata;
use std::path::Path;

mod docx;
mod pdf;
mod spreadsheet;

pub(crate) const MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentKind {
    Pdf,
    Docx,
    Xls,
    Xlsx,
    Csv,
    LegacyDoc,
}

pub fn validate_input(path: &Path, metadata: &Metadata) -> Result<DocumentKind, AppError> {
    if !path.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound));
    }
    if metadata.len() == 0 {
        return Err(AppError::new(ErrorCode::NoExtractableText));
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(AppError::new(ErrorCode::FileTooLarge));
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| AppError::new(ErrorCode::UnsupportedFormat))?;

    match extension.as_str() {
        "pdf" => Ok(DocumentKind::Pdf),
        "docx" => Ok(DocumentKind::Docx),
        "xls" => Ok(DocumentKind::Xls),
        "xlsx" => Ok(DocumentKind::Xlsx),
        "csv" => Ok(DocumentKind::Csv),
        "doc" => Ok(DocumentKind::LegacyDoc),
        _ => Err(AppError::new(ErrorCode::UnsupportedFormat)),
    }
}

pub fn extract_document(path: &Path, kind: DocumentKind) -> Result<String, AppError> {
    if kind == DocumentKind::LegacyDoc {
        return Err(AppError::legacy_doc());
    }
    if !path.is_file() {
        return Err(AppError::new(ErrorCode::FileNotFound));
    }

    let text = match kind {
        DocumentKind::Pdf => pdf::extract(path)?,
        DocumentKind::Docx => docx::extract(path)?,
        DocumentKind::Xls | DocumentKind::Xlsx | DocumentKind::Csv => {
            spreadsheet::extract(path, kind)?
        }
        DocumentKind::LegacyDoc => unreachable!("legacy DOC returned before extraction"),
    };

    if text.trim().is_empty() {
        Err(AppError::new(ErrorCode::NoExtractableText))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_document, validate_input, DocumentKind};
    use std::fs::{self, File};
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn error_code(error: crate::error::AppError) -> String {
        serde_json::to_value(error).expect("error should serialize")["code"]
            .as_str()
            .expect("error code should be a string")
            .to_owned()
    }

    fn error_message(error: crate::error::AppError) -> String {
        serde_json::to_value(error).expect("error should serialize")["message"]
            .as_str()
            .expect("error message should be a string")
            .to_owned()
    }

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/documents")
            .join(name)
    }

    #[test]
    fn accepts_supported_extensions_case_insensitively() {
        let directory = tempdir().expect("temporary directory should be created");

        for (filename, expected) in [
            ("manual.PDF", DocumentKind::Pdf),
            ("manual.DoCx", DocumentKind::Docx),
            ("register.XLS", DocumentKind::Xls),
            ("register.XlSx", DocumentKind::Xlsx),
            ("register.CsV", DocumentKind::Csv),
            ("manual.DoC", DocumentKind::LegacyDoc),
        ] {
            let path = directory.path().join(filename);
            fs::write(&path, b"fixture").expect("fixture should be written");
            let metadata = fs::metadata(&path).expect("metadata should be available");

            assert_eq!(validate_input(&path, &metadata), Ok(expected));
        }
    }

    #[test]
    fn rejects_a_missing_path_even_when_metadata_was_obtained_earlier() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("missing.pdf");
        fs::write(&path, b"fixture").expect("fixture should be written");
        let metadata = fs::metadata(&path).expect("metadata should be available");
        fs::remove_file(&path).expect("fixture should be removed");

        let error = validate_input(&path, &metadata).expect_err("missing file should fail");

        assert_eq!(error_code(error), "file_not_found");
    }

    #[test]
    fn rejects_an_empty_file_before_parsing() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("empty.pdf");
        File::create(&path).expect("empty fixture should be created");
        let metadata = fs::metadata(&path).expect("metadata should be available");

        let error = validate_input(&path, &metadata).expect_err("empty file should fail");

        assert_eq!(error_code(error), "no_extractable_text");
    }

    #[test]
    fn accepts_exactly_50_mib_and_rejects_one_byte_more() {
        const MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("boundary.pdf");
        let file = File::create(&path).expect("fixture should be created");
        file.set_len(MAX_INPUT_BYTES)
            .expect("sparse fixture should be resized");
        let metadata = fs::metadata(&path).expect("metadata should be available");
        assert_eq!(validate_input(&path, &metadata), Ok(DocumentKind::Pdf));

        file.set_len(MAX_INPUT_BYTES + 1)
            .expect("sparse fixture should be resized");
        let metadata = fs::metadata(&path).expect("metadata should be available");
        let error = validate_input(&path, &metadata).expect_err("oversize file should fail");

        assert_eq!(error_code(error), "file_too_large");
    }

    #[test]
    fn rejects_unsupported_ods_files() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("register.ods");
        fs::write(&path, b"fixture").expect("fixture should be written");
        let metadata = fs::metadata(&path).expect("metadata should be available");

        let error = validate_input(&path, &metadata).expect_err("ODS should be rejected");

        assert_eq!(error_code(error), "unsupported_format");
    }

    #[test]
    fn dispatches_every_supported_document_kind() {
        assert_eq!(
            extract_document(&fixture("three-pages.pdf"), DocumentKind::Pdf)
                .expect("PDF should extract"),
            "First page\n\nThird page"
        );
        assert!(
            extract_document(&fixture("body-and-table.docx"), DocumentKind::Docx)
                .expect("DOCX should extract")
                .contains("地址\t名称\t备注")
        );
        assert!(
            extract_document(&fixture("register.xls"), DocumentKind::Xls)
                .expect("XLS should extract")
                .starts_with("=== Sheet: Main ===")
        );
        assert!(
            extract_document(&fixture("register.xlsx"), DocumentKind::Xlsx)
                .expect("XLSX should extract")
                .contains("=== Sheet: 隐藏参数 ===")
        );
        assert!(
            extract_document(&fixture("register-utf8-bom.csv"), DocumentKind::Csv)
                .expect("CSV should extract")
                .starts_with("=== CSV ===")
        );
    }

    #[test]
    fn legacy_doc_returns_explicit_save_as_docx_guidance() {
        let error = extract_document(&fixture("corrupt.docx"), DocumentKind::LegacyDoc)
            .expect_err("legacy DOC should be rejected");

        assert_eq!(error_code(error.clone()), "unsupported_format");
        assert!(error_message(error).contains("另存为 DOCX"));
    }

    #[test]
    fn rejects_a_non_empty_csv_that_contains_no_extractable_cells() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("blank.csv");
        fs::write(&path, ",,\n,,\n").expect("blank CSV fixture should be written");

        let error = extract_document(&path, DocumentKind::Csv)
            .expect_err("CSV without cell text should fail");

        assert_eq!(error_code(error), "no_extractable_text");
    }

    #[test]
    fn returns_file_not_found_when_pdf_or_workbook_disappears_after_validation() {
        let directory = tempdir().expect("temporary directory should be created");

        for (filename, expected_kind) in [
            ("vanishing.pdf", DocumentKind::Pdf),
            ("vanishing.xls", DocumentKind::Xls),
            ("vanishing.xlsx", DocumentKind::Xlsx),
        ] {
            let path = directory.path().join(filename);
            fs::write(&path, b"synthetic non-empty file").expect("fixture should be written");
            let metadata = fs::metadata(&path).expect("metadata should be available");
            let kind = validate_input(&path, &metadata).expect("input should validate");
            assert_eq!(kind, expected_kind);
            fs::remove_file(&path).expect("validated fixture should be removed");

            let error = extract_document(&path, kind)
                .expect_err("a file removed after validation should fail");

            assert_eq!(error_code(error), "file_not_found");
        }
    }

    #[test]
    fn keeps_damaged_pdf_xls_and_xlsx_distinct_from_missing_files() {
        for (filename, kind) in [
            ("corrupt.pdf", DocumentKind::Pdf),
            ("corrupt.xls", DocumentKind::Xls),
            ("corrupt.xlsx", DocumentKind::Xlsx),
        ] {
            let error = extract_document(&fixture(filename), kind)
                .expect_err("a damaged document should fail");

            assert_eq!(error_code(error), "parse_failed");
        }
    }
}
