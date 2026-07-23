#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformEnvelope {
    pub serial: String,
    pub inner: Vec<u8>,
    pub inner_offset: usize,
}

pub type EnvelopeResult = Result<PlatformEnvelope, EnvelopeError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeError {
    pub code: &'static str,
}

pub fn strip_platform_envelope(frame: &[u8]) -> EnvelopeResult {
    if frame.first() != Some(&0x68) {
        return Err(EnvelopeError {
            code: "missing_platform_envelope",
        });
    }

    let second_marker = frame[1..]
        .iter()
        .position(|byte| *byte == 0x68)
        .map(|position| position + 1)
        .ok_or(EnvelopeError {
            code: "incomplete_platform_envelope",
        })?;
    let serial = &frame[1..second_marker];
    let inner_offset = second_marker + 1;

    if serial.is_empty()
        || !serial.len().is_multiple_of(2)
        || !serial.iter().all(u8::is_ascii_digit)
        || frame.len().saturating_sub(inner_offset) < 4
    {
        return Err(EnvelopeError {
            code: "invalid_platform_envelope",
        });
    }

    Ok(PlatformEnvelope {
        serial: String::from_utf8(serial.to_vec()).expect("ASCII digits are valid UTF-8"),
        inner: frame[inner_offset..].to_vec(),
        inner_offset,
    })
}

#[cfg(test)]
mod tests {
    use super::strip_platform_envelope;

    fn hex_bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn bytes_to_upper_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02X}")).collect()
    }

    #[test]
    fn strips_ascii_serial_envelope_before_modbus() {
        let frame = hex_bytes("683236303632353039333330303031680110E000000677CB");
        let envelope = strip_platform_envelope(&frame).unwrap();
        assert_eq!(envelope.serial, "26062509330001");
        assert_eq!(bytes_to_upper_hex(&envelope.inner), "0110E000000677CB");
    }

    #[test]
    fn rejects_non_digit_or_odd_length_serials() {
        assert!(strip_platform_envelope(&hex_bytes("6831324168010300000001840A")).is_err());
        assert!(strip_platform_envelope(&hex_bytes("6831323368010300000001840A")).is_err());
    }
}
