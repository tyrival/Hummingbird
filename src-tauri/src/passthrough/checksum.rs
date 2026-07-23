pub fn modbus_crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn additive_checksum(bytes: &[u8]) -> u8 {
    bytes
        .iter()
        .fold(0_u8, |checksum, byte| checksum.wrapping_add(*byte))
}

#[cfg(test)]
mod tests {
    use super::{additive_checksum, modbus_crc16};

    fn hex_bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = std::str::from_utf8(pair).expect("fixture must be ASCII");
                u8::from_str_radix(text, 16).expect("fixture must be Hex")
            })
            .collect()
    }

    #[test]
    fn crc_matches_user_modbus_sample() {
        let bytes = hex_bytes("0420035400050A000D020301071A000000");
        assert_eq!(modbus_crc16(&bytes), 0x6849);
    }

    #[test]
    fn additive_checksum_keeps_the_low_eight_bits() {
        assert_eq!(additive_checksum(&[0xFE, 0x03, 0x04]), 0x05);
    }
}
