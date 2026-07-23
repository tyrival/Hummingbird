use super::checksum::additive_checksum;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dlt645Frame {
    pub wakeup_count: usize,
    pub address: String,
    pub control_code: u8,
    pub data_raw: Vec<u8>,
    pub data_decoded: Vec<u8>,
    pub checksum_received: u8,
    pub checksum_calculated: u8,
    pub checksum_valid: bool,
}

pub fn parse_dlt645(frame: &[u8], wakeup_count: usize) -> Result<Dlt645Frame, &'static str> {
    if frame.len() < 12 || frame[0] != 0x68 || frame[7] != 0x68 {
        return Err("invalid_dlt645_markers");
    }
    let data_length = usize::from(frame[9]);
    let expected_length = 12 + data_length;
    if frame.len() != expected_length {
        return Err("invalid_dlt645_length");
    }
    if frame[expected_length - 1] != 0x16 {
        return Err("invalid_dlt645_terminator");
    }
    let checksum_index = expected_length - 2;
    let checksum_calculated = additive_checksum(&frame[..checksum_index]);
    let checksum_received = frame[checksum_index];
    let address = frame[1..7]
        .iter()
        .rev()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let data_raw = frame[10..checksum_index].to_vec();
    let data_decoded = data_raw
        .iter()
        .map(|byte| byte.wrapping_sub(0x33))
        .collect();
    Ok(Dlt645Frame {
        wakeup_count,
        address,
        control_code: frame[8],
        data_raw,
        data_decoded,
        checksum_received,
        checksum_calculated,
        checksum_valid: checksum_received == checksum_calculated,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_dlt645;

    #[test]
    fn parses_a_standard_structure_fixture() {
        let frame = [
            0x68, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x68, 0x11, 0x04, 0x33, 0x34, 0x35, 0x36,
            0x1C, 0x16,
        ];
        let parsed = parse_dlt645(&frame, 2).unwrap();
        assert_eq!(parsed.address, "665544332211");
        assert_eq!(parsed.data_decoded, [0, 1, 2, 3]);
        assert!(parsed.checksum_valid);
    }
}
