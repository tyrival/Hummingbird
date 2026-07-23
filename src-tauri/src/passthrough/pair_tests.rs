use super::checksum::additive_checksum;
use super::{parse_message_pairs, MessageRole};

fn frame_hex(mut bytes_without_checksum_or_end: Vec<u8>) -> String {
    let checksum = additive_checksum(&bytes_without_checksum_or_end);
    bytes_without_checksum_or_end.push(checksum);
    bytes_without_checksum_or_end.push(0x16);
    format!(
        "FE{}",
        bytes_without_checksum_or_end
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    )
}

#[test]
fn modbus_read_response_inherits_the_request_register_range() {
    let results = parse_message_pairs(
        "0103016D001255E6",
        Some("010324010000020000010800010002010C00020001120001000201160002000100000000000000ED53"),
    )
    .unwrap();

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].role, MessageRole::Request);
    assert_eq!(results[1].role, MessageRole::Response);
    assert_eq!(
        results[1].registers.first().and_then(|item| item.address),
        Some(0x016D)
    );
    assert_eq!(
        results[1].registers.last().and_then(|item| item.address),
        Some(0x017E)
    );
    assert!(!results[1]
        .warnings
        .iter()
        .any(|item| item.code == "response_without_request_context"));
    assert!(!results[1]
        .fields
        .iter()
        .any(|item| item.name == "startAddress" && item.raw_hex == "2401"));
}

#[test]
fn request_without_response_remains_parseable() {
    let results = parse_message_pairs("0103016D001255E6", None).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].role, MessageRole::Request);
}

#[test]
fn dlt645_and_cjt188_pairs_validate_address_control_and_identifier() {
    let dlt_request = frame_hex(vec![
        0x68, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x68, 0x11, 0x04, 0x33, 0x34, 0x35, 0x36,
    ]);
    let dlt_response = frame_hex(vec![
        0x68, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x68, 0x91, 0x06, 0x33, 0x34, 0x35, 0x36, 0x01,
        0x02,
    ]);
    let dlt = parse_message_pairs(&dlt_request, Some(&dlt_response)).unwrap();
    assert!(!dlt[1]
        .warnings
        .iter()
        .any(|warning| warning.code.starts_with("request_response_")));

    let cjt_request = frame_hex(vec![
        0x68, 0x10, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x01, 0x00, 0x02, 0x90, 0x1F,
    ]);
    let cjt_response = frame_hex(vec![
        0x68, 0x10, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x81, 0x00, 0x04, 0x90, 0x1F, 0x01,
        0x02,
    ]);
    let cjt = parse_message_pairs(&cjt_request, Some(&cjt_response)).unwrap();
    assert!(!cjt[1]
        .warnings
        .iter()
        .any(|warning| warning.code.starts_with("request_response_")));
}
