use super::checksum::modbus_crc16;
use super::{ChecksumResult, FactSource, FieldValue, ParseWarning, ProtocolKind, RegisterValue};
use crate::passthrough::envelope::PlatformEnvelope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    Standard,
    Private,
    Exception,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegisterLayout {
    StartQuantityByteCount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolParse {
    pub protocol: ProtocolKind,
    pub platform_serial: Option<String>,
    pub slave: Option<u8>,
    pub function_code: u8,
    pub function_kind: FunctionKind,
    pub operation: String,
    pub start_address: Option<u16>,
    pub quantity: Option<u16>,
    pub byte_count: Option<u8>,
    pub data: Vec<u8>,
    pub exception_code: Option<u8>,
    pub register_layout: Option<RegisterLayout>,
    pub fields: Vec<FieldValue>,
    pub registers: Vec<RegisterValue>,
    pub checksum: Option<ChecksumResult>,
    pub warnings: Vec<ParseWarning>,
}

pub fn parse_modbus(frame: &[u8], envelope: Option<&PlatformEnvelope>) -> ProtocolParse {
    let slave = frame.first().copied();
    let raw_function = frame.get(1).copied().unwrap_or(0);
    let is_exception = raw_function & 0x80 != 0;
    let function_code = if is_exception {
        raw_function & 0x7F
    } else {
        raw_function
    };
    let standard = matches!(
        function_code,
        0x01 | 0x02 | 0x03 | 0x04 | 0x05 | 0x06 | 0x0F | 0x10 | 0x16 | 0x17
    );
    let function_kind = if is_exception {
        FunctionKind::Exception
    } else if standard {
        FunctionKind::Standard
    } else {
        FunctionKind::Private
    };
    let payload_end = frame.len().saturating_sub(2);
    let pdu = if payload_end > 2 {
        &frame[2..payload_end]
    } else {
        &[]
    };
    let received_crc = (frame.len() >= 2)
        .then(|| u16::from_le_bytes([frame[frame.len() - 2], frame[frame.len() - 1]]));
    let calculated_crc = (frame.len() >= 2).then(|| modbus_crc16(&frame[..frame.len() - 2]));
    let checksum = calculated_crc.map(|calculated| ChecksumResult {
        kind: "modbusCrc16".to_owned(),
        received: received_crc.map(|value| format!("{value:04X}")),
        calculated: format!("{calculated:04X}"),
        valid: received_crc.map(|received| received == calculated),
    });

    let mut parsed = ProtocolParse {
        protocol: ProtocolKind::ModbusRtu,
        platform_serial: envelope.map(|value| value.serial.clone()),
        slave,
        function_code: raw_function,
        function_kind,
        operation: operation_name(raw_function).to_owned(),
        start_address: None,
        quantity: None,
        byte_count: None,
        data: Vec::new(),
        exception_code: None,
        register_layout: None,
        fields: base_fields(frame, envelope),
        registers: Vec::new(),
        checksum,
        warnings: Vec::new(),
    };

    if frame.len() < 4 {
        parsed.warnings.push(warning(
            "short_modbus_frame",
            "Modbus 报文缺少地址、功能码或 CRC。",
        ));
        return parsed;
    }
    if parsed.checksum.as_ref().and_then(|value| value.valid) == Some(false) {
        parsed
            .warnings
            .push(warning("invalid_crc", "Modbus CRC 校验失败。"));
    }
    if is_exception {
        parsed.exception_code = pdu.first().copied();
        return parsed;
    }

    if standard {
        parse_standard_pdu(&mut parsed, function_code, pdu);
    } else {
        parse_private_pdu(&mut parsed, pdu);
    }
    append_protocol_fields(&mut parsed, frame, envelope);
    parsed
}

fn append_protocol_fields(
    parsed: &mut ProtocolParse,
    frame: &[u8],
    envelope: Option<&PlatformEnvelope>,
) {
    let offset = envelope.map_or(0, |value| value.inner_offset);
    let payload_end = frame.len().saturating_sub(2);
    let pdu = frame.get(2..payload_end).unwrap_or_default();
    if matches!(parsed.function_code, 0x03 | 0x04)
        && parsed.start_address.is_none()
        && parsed.byte_count.is_some()
    {
        if let Some(byte_count) = pdu.first() {
            parsed.fields.push(range_field(
                "byteCount",
                offset + 2,
                &pdu[..1],
                format!("数据字节数 {byte_count}"),
            ));
            if pdu.len() > 1 {
                parsed.fields.push(range_field(
                    "data",
                    offset + 3,
                    &pdu[1..],
                    "寄存器返回数据".to_owned(),
                ));
            }
        }
    } else if matches!(parsed.function_code, 0x03 | 0x04 | 0x0F | 0x10) && pdu.len() >= 4 {
        parsed.fields.push(range_field(
            "startAddress",
            offset + 2,
            &pdu[0..2],
            format!(
                "起始寄存器地址 0x{:04X}",
                u16::from_be_bytes([pdu[0], pdu[1]])
            ),
        ));
        parsed.fields.push(range_field(
            "quantity",
            offset + 4,
            &pdu[2..4],
            format!("寄存器数量 {}", u16::from_be_bytes([pdu[2], pdu[3]])),
        ));
        if let Some(byte_count) = pdu.get(4) {
            parsed.fields.push(range_field(
                "byteCount",
                offset + 6,
                &pdu[4..5],
                format!("数据字节数 {byte_count}"),
            ));
            if pdu.len() > 5 {
                parsed.fields.push(range_field(
                    "data",
                    offset + 7,
                    &pdu[5..],
                    "寄存器写入数据".to_owned(),
                ));
            }
        }
    }
    if frame.len() >= 2 {
        let crc_start = frame.len() - 2;
        let status = match parsed.checksum.as_ref().and_then(|checksum| checksum.valid) {
            Some(true) => "CRC16，校验通过",
            Some(false) => "CRC16，校验失败",
            None => "CRC16，无法校验",
        };
        parsed.fields.push(range_field(
            "crc",
            offset + crc_start,
            &frame[crc_start..],
            status.to_owned(),
        ));
    }
}

fn range_field(name: &str, start: usize, bytes: &[u8], display_value: String) -> FieldValue {
    FieldValue {
        name: name.to_owned(),
        byte_start: start,
        byte_end: start + bytes.len(),
        raw_hex: bytes.iter().map(|byte| format!("{byte:02X}")).collect(),
        display_value,
        source: FactSource::Code,
    }
}

fn parse_standard_pdu(parsed: &mut ProtocolParse, function: u8, pdu: &[u8]) {
    match function {
        0x01 | 0x02 if pdu.len() == 4 => set_range(parsed, pdu),
        0x03 | 0x04 if pdu.len() == 4 => {
            set_range(parsed, pdu);
            push_requested_registers(parsed);
        }
        0x01..=0x04
            if pdu
                .first()
                .is_some_and(|count| pdu.len() == usize::from(*count) + 1) =>
        {
            parsed.byte_count = pdu.first().copied();
            parsed.data = pdu[1..].to_vec();
            parsed.warnings.push(warning(
                "response_without_request_context",
                "读响应缺少请求上下文，无法确定寄存器起始地址。",
            ));
        }
        0x05 | 0x06 if pdu.len() == 4 => {
            parsed.start_address = read_u16(pdu, 0);
            parsed.quantity = Some(1);
            parsed.data = pdu[2..].to_vec();
            push_registers(parsed);
        }
        0x0F if pdu.len() == 4 => set_range(parsed, pdu),
        0x10 if pdu.len() == 4 => {
            set_range(parsed, pdu);
            push_requested_registers(parsed);
        }
        0x0F | 0x10 if pdu.len() >= 5 && usize::from(pdu[4]) + 5 == pdu.len() => {
            parsed.start_address = read_u16(pdu, 0);
            parsed.quantity = read_u16(pdu, 2);
            parsed.byte_count = Some(pdu[4]);
            parsed.data = pdu[5..].to_vec();
            parsed.register_layout = Some(RegisterLayout::StartQuantityByteCount);
            push_registers(parsed);
        }
        0x16 if pdu.len() == 6 => {
            parsed.start_address = read_u16(pdu, 0);
            parsed.quantity = Some(1);
            parsed.data = pdu[2..].to_vec();
        }
        0x17 if pdu.len() >= 9 && usize::from(pdu[8]) + 9 == pdu.len() => {
            parsed.start_address = read_u16(pdu, 0);
            parsed.quantity = read_u16(pdu, 2);
            parsed.byte_count = Some(pdu[8]);
            parsed.data = pdu[9..].to_vec();
        }
        _ => parsed.warnings.push(warning(
            "unrecognized_standard_pdu_shape",
            "功能码是标准 Modbus 功能码，但当前 PDU 形态无法唯一解析。",
        )),
    }
}

fn parse_private_pdu(parsed: &mut ProtocolParse, pdu: &[u8]) {
    if pdu.len() >= 5 {
        let quantity = read_u16(pdu, 2);
        let byte_count = pdu[4];
        if quantity.is_some()
            && usize::from(byte_count) + 5 == pdu.len()
            && usize::from(quantity.unwrap()) * 2 == usize::from(byte_count)
        {
            parsed.start_address = read_u16(pdu, 0);
            parsed.quantity = quantity;
            parsed.byte_count = Some(byte_count);
            parsed.data = pdu[5..].to_vec();
            parsed.register_layout = Some(RegisterLayout::StartQuantityByteCount);
            push_registers(parsed);
            return;
        }
    }
    parsed.data = pdu.to_vec();
    parsed.warnings.push(warning(
        "private_pdu_unknown_layout",
        "私有功能码 PDU 未匹配唯一的通用寄存器结构，已保留原始数据。",
    ));
}

fn set_range(parsed: &mut ProtocolParse, pdu: &[u8]) {
    parsed.start_address = read_u16(pdu, 0);
    parsed.quantity = read_u16(pdu, 2);
}

fn push_registers(parsed: &mut ProtocolParse) {
    let Some(start) = parsed.start_address else {
        return;
    };
    for (offset, bytes) in parsed.data.chunks_exact(2).enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        parsed.registers.push(RegisterValue {
            address: start.checked_add(offset),
            identifier: None,
            raw_hex: format!("{:02X}{:02X}", bytes[0], bytes[1]),
            source: FactSource::Code,
        });
    }
}

fn push_requested_registers(parsed: &mut ProtocolParse) {
    let (Some(start), Some(quantity)) = (parsed.start_address, parsed.quantity) else {
        return;
    };
    if quantity == 0 || quantity > 125 {
        parsed.warnings.push(warning(
            "invalid_register_quantity",
            "Modbus 读寄存器数量必须在 1 到 125 之间。",
        ));
        return;
    }
    for offset in 0..quantity {
        let Some(address) = start.checked_add(offset) else {
            parsed.warnings.push(warning(
                "register_address_overflow",
                "寄存器地址超出 16 位范围，已停止展开。",
            ));
            break;
        };
        parsed.registers.push(RegisterValue {
            address: Some(address),
            identifier: None,
            raw_hex: String::new(),
            source: FactSource::Code,
        });
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn base_fields(frame: &[u8], envelope: Option<&PlatformEnvelope>) -> Vec<FieldValue> {
    let offset = envelope.map_or(0, |value| value.inner_offset);
    let mut fields = Vec::new();
    if let Some(value) = envelope {
        fields.push(FieldValue {
            name: "platformSerial".to_owned(),
            byte_start: 0,
            byte_end: value.inner_offset,
            raw_hex: format!(
                "68{}68",
                value
                    .serial
                    .as_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<String>()
            ),
            display_value: format!("仪表序列号 {}", value.serial),
            source: FactSource::Code,
        });
    }
    if let Some(slave) = frame.first() {
        fields.push(field(
            "slave",
            offset,
            *slave,
            format!("Modbus 地址 {slave}"),
        ));
    }
    if let Some(function) = frame.get(1) {
        fields.push(field(
            "function",
            offset + 1,
            *function,
            function_description(*function).to_owned(),
        ));
    }
    fields
}

fn field(name: &str, offset: usize, value: u8, display_value: String) -> FieldValue {
    FieldValue {
        name: name.to_owned(),
        byte_start: offset,
        byte_end: offset + 1,
        raw_hex: format!("{value:02X}"),
        display_value,
        source: FactSource::Code,
    }
}

fn function_description(function: u8) -> &'static str {
    match function {
        0x01 => "读线圈",
        0x02 => "读离散输入",
        0x03 => "读保持寄存器",
        0x04 => "读输入寄存器",
        0x05 => "写单个线圈",
        0x06 => "写单个寄存器",
        0x0F => "写多个线圈",
        0x10 => "写多个寄存器",
        0x16 => "屏蔽写寄存器",
        0x17 => "读写多个寄存器",
        _ => "私有命令",
    }
}

fn warning(code: &str, message: &str) -> ParseWarning {
    ParseWarning {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

fn operation_name(function: u8) -> &'static str {
    match function {
        0x01..=0x04 => "read",
        0x05 | 0x06 | 0x0F | 0x10 | 0x16 => "write",
        0x17 => "readWrite",
        value if value & 0x80 != 0 => "exception",
        _ => "private",
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_modbus, FunctionKind, RegisterLayout};
    use crate::passthrough::checksum::modbus_crc16;
    use crate::passthrough::envelope::strip_platform_envelope;

    fn hex_bytes(value: &str) -> Vec<u8> {
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    #[test]
    fn parses_unknown_private_function_without_inventing_a_standard_name() {
        let frame = hex_bytes("0420035400050A000D020301071A0000004968");
        let parsed = parse_modbus(&frame, None);
        assert_eq!(parsed.function_code, 0x20);
        assert_eq!(parsed.function_kind, FunctionKind::Private);
        assert_eq!(parsed.start_address, Some(0x0354));
        assert_eq!(parsed.quantity, Some(5));
        assert_eq!(parsed.byte_count, Some(10));
        assert_eq!(
            parsed.register_layout,
            Some(RegisterLayout::StartQuantityByteCount)
        );
        assert_eq!(parsed.registers.len(), 5);
        assert!(parsed.checksum.unwrap().valid.unwrap());
    }

    #[test]
    fn does_not_assign_addresses_to_a_read_response_without_request_context() {
        let frame = hex_bytes("010304000100022A32");
        let parsed = parse_modbus(&frame, None);
        assert_eq!(parsed.start_address, None);
        assert!(parsed.registers.is_empty());
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning.code == "response_without_request_context"));
    }

    #[test]
    fn expands_every_requested_holding_register_without_inventing_values() {
        let frame = hex_bytes("01030354000345B3");
        let parsed = parse_modbus(&frame, None);
        assert_eq!(parsed.start_address, Some(0x0354));
        assert_eq!(parsed.quantity, Some(3));
        assert_eq!(parsed.registers.len(), 3);
        assert_eq!(parsed.registers[0].address, Some(0x0354));
        assert_eq!(parsed.registers[2].address, Some(0x0356));
        assert!(parsed
            .registers
            .iter()
            .all(|register| register.raw_hex.is_empty()));
    }

    #[test]
    fn expands_registers_from_platform_wrapped_write_multiple_responses() {
        let cases = [
            (
                "683236303632353039333330303031680110E000000677CB",
                0xE000,
                6,
            ),
            (
                "683236303632353039333330303031680110E006000697CA",
                0xE006,
                6,
            ),
            (
                "683236303632353039333330303031680110E00C0009F7CC",
                0xE00C,
                9,
            ),
            (
                "683236303632353039333330303031680110E02A001517CE",
                0xE02A,
                21,
            ),
        ];

        for (raw, start, quantity) in cases {
            let envelope = strip_platform_envelope(&hex_bytes(raw)).unwrap();
            let parsed = parse_modbus(&envelope.inner, Some(&envelope));
            assert_eq!(
                parsed
                    .fields
                    .first()
                    .map(|field| field.display_value.as_str()),
                Some("仪表序列号 26062509330001")
            );
            assert_eq!(parsed.start_address, Some(start));
            assert_eq!(parsed.quantity, Some(quantity));
            assert_eq!(parsed.registers.len(), usize::from(quantity));
            assert_eq!(
                parsed.registers.first().and_then(|value| value.address),
                Some(start)
            );
            assert!(parsed
                .registers
                .iter()
                .all(|register| register.raw_hex.is_empty()));
        }
    }

    #[test]
    fn exposes_write_values_and_ordered_fields_for_write_multiple_request() {
        let mut frame = hex_bytes("0110E00000020400010002");
        frame.extend(modbus_crc16(&frame).to_le_bytes());
        let parsed = parse_modbus(&frame, None);

        assert_eq!(parsed.registers.len(), 2);
        assert_eq!(parsed.registers[0].raw_hex, "0001");
        assert_eq!(parsed.registers[1].raw_hex, "0002");
        assert_eq!(
            parsed
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "slave",
                "function",
                "startAddress",
                "quantity",
                "byteCount",
                "data",
                "crc"
            ]
        );
        assert_eq!(parsed.fields[1].display_value, "写多个寄存器");
        assert_eq!(
            parsed.fields.last().unwrap().display_value,
            "CRC16，校验通过"
        );
    }
}
