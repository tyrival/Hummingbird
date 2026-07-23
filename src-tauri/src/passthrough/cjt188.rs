use super::checksum::additive_checksum;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cjt188Frame {
    pub wakeup_count: usize,
    pub meter_type: u8,
    pub address: String,
    pub control_code: u8,
    pub sequence: u8,
    pub data: Vec<u8>,
    pub checksum_received: u8,
    pub checksum_calculated: u8,
    pub checksum_valid: bool,
}

pub fn parse_cjt188(frame: &[u8], wakeup_count: usize) -> Result<Cjt188Frame, &'static str> {
    if frame.len() < 14 || frame[0] != 0x68 {
        return Err("invalid_cjt188_marker");
    }
    let data_length = usize::from(frame[11]);
    let expected_length = 14 + data_length;
    if frame.len() != expected_length {
        return Err("invalid_cjt188_length");
    }
    if frame[expected_length - 1] != 0x16 {
        return Err("invalid_cjt188_terminator");
    }
    let checksum_index = expected_length - 2;
    let checksum_calculated = additive_checksum(&frame[..checksum_index]);
    let checksum_received = frame[checksum_index];
    let address = frame[2..9]
        .iter()
        .rev()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    Ok(Cjt188Frame {
        wakeup_count,
        meter_type: frame[1],
        address,
        control_code: frame[9],
        sequence: frame[10],
        data: frame[12..checksum_index].to_vec(),
        checksum_received,
        checksum_calculated,
        checksum_valid: checksum_received == checksum_calculated,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_cjt188;

    #[test]
    fn parses_a_standard_structure_fixture() {
        let frame = [
            0x68, 0x10, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x01, 0x00, 0x02, 0x90, 0x1F,
            0x06, 0x16,
        ];
        let parsed = parse_cjt188(&frame, 1).unwrap();
        assert_eq!(parsed.address, "77665544332211");
        assert_eq!(parsed.data, [0x90, 0x1F]);
        assert!(parsed.checksum_valid);
    }
}
