use crate::error::{AppError, ErrorCode};
use std::path::Path;

pub(super) fn extract(path: &Path) -> Result<String, AppError> {
    let pages = pdf_extract::extract_text_by_pages(path)
        .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;
    let text = pages
        .iter()
        .map(|page| page.trim())
        .filter(|page| !page.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if text.is_empty() {
        Err(AppError::new(ErrorCode::NoExtractableText))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use std::fs;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/documents")
            .join(name)
    }

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

    #[test]
    fn joins_non_empty_pdf_pages_with_one_blank_line() {
        let text = extract(&fixture("three-pages.pdf")).expect("PDF fixture should parse");

        assert_eq!(text, "First page\n\nThird page");
    }

    #[test]
    fn reports_no_extractable_text_for_a_page_without_searchable_text() {
        let path = fixture("image-only.pdf");
        let bytes = fs::read(&path).expect("image-only fixture should be readable");
        assert!(
            bytes
                .windows(b"/Subtype/Image".len())
                .any(|window| window == b"/Subtype/Image"),
            "fixture must contain a real PDF Image XObject"
        );

        let error = extract(&path).expect_err("image-only PDF should have no searchable text");

        assert_eq!(error_code(error.clone()), "no_extractable_text");
        let message = error_message(error);
        assert!(message.contains("扫描件或图片型 PDF"));
        assert!(message.contains("不支持 OCR"));
    }

    #[test]
    fn reports_parse_failed_for_an_invalid_pdf() {
        let error = extract(&fixture("corrupt.pdf")).expect_err("corrupt PDF package should fail");

        assert_eq!(error_code(error), "parse_failed");
    }
}
