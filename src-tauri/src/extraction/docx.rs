use crate::error::{AppError, ErrorCode};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub(super) fn extract(path: &Path) -> Result<String, AppError> {
    let file = File::open(path).map_err(|_| AppError::new(ErrorCode::FileNotFound))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;
    let mut document_xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?
        .read_to_string(&mut document_xml)
        .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;

    parse_document_xml(&document_xml)
}

fn parse_document_xml(document_xml: &str) -> Result<String, AppError> {
    let mut reader = Reader::from_str(document_xml);
    let mut blocks = Vec::new();
    let mut table_depth = 0_usize;
    let mut in_text = false;
    let mut paragraph = String::new();
    let mut cell = String::new();
    let mut row = Vec::new();

    loop {
        let event = reader
            .read_event()
            .map_err(|error| AppError::internal(ErrorCode::ParseFailed, error.to_string()))?;

        match event {
            Event::Start(element) => match local_name(element.name().as_ref()) {
                b"tbl" => table_depth += 1,
                b"tr" if table_depth == 1 => row.clear(),
                b"tc" if table_depth == 1 => cell.clear(),
                b"p" => paragraph.clear(),
                b"t" => in_text = true,
                _ => {}
            },
            Event::Empty(element) => match local_name(element.name().as_ref()) {
                b"tab" if in_text || !paragraph.is_empty() => paragraph.push('\t'),
                b"br" if in_text || !paragraph.is_empty() => paragraph.push('\n'),
                _ => {}
            },
            Event::Text(text) if in_text => {
                let decoded = text.decode().map_err(|error| {
                    AppError::internal(ErrorCode::ParseFailed, error.to_string())
                })?;
                let unescaped = quick_xml::escape::unescape(decoded.as_ref()).map_err(|error| {
                    AppError::internal(ErrorCode::ParseFailed, error.to_string())
                })?;
                paragraph.push_str(&unescaped);
            }
            Event::CData(text) if in_text => {
                let decoded = text.decode().map_err(|error| {
                    AppError::internal(ErrorCode::ParseFailed, error.to_string())
                })?;
                paragraph.push_str(&decoded);
            }
            Event::End(element) => match local_name(element.name().as_ref()) {
                b"t" => in_text = false,
                b"p" => {
                    let text = paragraph.trim();
                    if !text.is_empty() {
                        if table_depth > 0 {
                            if !cell.is_empty() {
                                cell.push('\n');
                            }
                            cell.push_str(text);
                        } else {
                            blocks.push(text.to_owned());
                        }
                    }
                    paragraph.clear();
                }
                b"tc" if table_depth == 1 => row.push(cell.trim().to_owned()),
                b"tr" if table_depth == 1 => {
                    while row.last().is_some_and(String::is_empty) {
                        row.pop();
                    }
                    if row.iter().any(|value| !value.is_empty()) {
                        blocks.push(row.join("\t"));
                    }
                }
                b"tbl" => table_depth = table_depth.saturating_sub(1),
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(blocks.join("\n"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::extract;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::{AesMode, ZipWriter};

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

    #[test]
    fn preserves_body_paragraph_and_table_order_with_internal_empty_cells() {
        let text = extract(&fixture("body-and-table.docx")).expect("DOCX fixture should parse");

        assert_eq!(text, "第一段\n地址\t名称\t备注\n1\t\t电压\n第二段");
    }

    #[test]
    fn rejects_a_corrupt_docx_package_as_parse_failed() {
        let error = extract(&fixture("corrupt.docx")).expect_err("corrupt DOCX should fail");

        assert_eq!(error_code(error), "parse_failed");
    }

    #[test]
    fn rejects_an_encrypted_docx_package_as_parse_failed() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("encrypted.docx");
        let file = File::create(&path).expect("encrypted fixture should be created");
        let mut archive = ZipWriter::new(file);
        let options = SimpleFileOptions::default().with_aes_encryption(AesMode::Aes128, "password");
        archive
            .start_file("word/document.xml", options)
            .expect("encrypted entry should start");
        archive
            .write_all(b"<w:document/>")
            .expect("encrypted XML should be written");
        archive.finish().expect("encrypted fixture should finish");

        let error = extract(&path).expect_err("encrypted DOCX should fail without a password");

        assert_eq!(error_code(error), "parse_failed");
    }
}
