use super::{MessageInput, PassthroughError};

pub fn split_hex_messages(input: &str) -> Vec<MessageInput> {
    let compact: String = input
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let mut messages = Vec::new();

    for (index, raw_segment) in compact.split("&&").enumerate() {
        let mut message = MessageInput {
            index,
            raw_segment: raw_segment.to_owned(),
            cleaned_hex: None,
            bytes: None,
            error: None,
        };

        if raw_segment.is_empty() {
            message.error = Some(PassthroughError::new(
                "empty_segment",
                "分隔符产生了空报文片段。",
            ));
        } else if raw_segment.contains('&') {
            message.error = Some(PassthroughError::new(
                "invalid_separator",
                "只能使用双 && 分隔多条报文。",
            ));
        } else if !raw_segment
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        {
            message.error = Some(PassthroughError::new(
                "invalid_hex",
                "报文包含非 Hex 字符。",
            ));
        } else if raw_segment.len() % 2 != 0 {
            message.error = Some(PassthroughError::new(
                "odd_hex_length",
                "Hex 报文字符数必须为偶数。",
            ));
        } else {
            let cleaned_hex = raw_segment.to_ascii_uppercase();
            let bytes = decode_hex(&cleaned_hex);
            message.cleaned_hex = Some(cleaned_hex);
            message.bytes = Some(bytes);
        }

        messages.push(message);
    }

    messages
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]);
            let low = hex_nibble(pair[1]);
            (high << 4) | low
        })
        .collect()
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("input is validated before decoding"),
    }
}

#[cfg(test)]
mod tests {
    use super::split_hex_messages;

    #[test]
    fn splits_before_any_protocol_detection_and_accepts_mixed_frames() {
        let parsed =
            split_hex_messages(" 01 03 00 00 00 01 84 0A &&\nFE FE 68 11 22 16 && FE 68 33 44 16 ");
        assert_eq!(parsed.len(), 3);
        assert!(parsed.iter().all(|item| item.cleaned_hex.is_some()));
    }

    #[test]
    fn rejects_single_ampersand_without_losing_neighboring_segments() {
        let parsed = split_hex_messages("0103&0203&&0303");
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0].error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_separator")
        );
        assert_eq!(parsed[1].cleaned_hex.as_deref(), Some("0303"));
    }

    #[test]
    fn reports_empty_invalid_and_odd_segments_independently() {
        let parsed = split_hex_messages("&&01GG&&123");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].error.as_ref().unwrap().code, "empty_segment");
        assert_eq!(parsed[1].error.as_ref().unwrap().code, "invalid_hex");
        assert_eq!(parsed[2].error.as_ref().unwrap().code, "odd_hex_length");
    }

    #[test]
    fn removes_all_unicode_whitespace_before_splitting() {
        let parsed = split_hex_messages("01\u{2003}03\t&&\nFE 68");
        assert_eq!(parsed[0].cleaned_hex.as_deref(), Some("0103"));
        assert_eq!(parsed[1].cleaned_hex.as_deref(), Some("FE68"));
    }
}
