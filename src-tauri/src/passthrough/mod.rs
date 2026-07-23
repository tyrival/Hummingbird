pub mod checksum;
pub mod cjt188;
pub mod commands;
pub mod dlt645;
pub mod envelope;
pub mod explanation;
pub mod input;
pub mod modbus;
pub mod register_map;

use crate::error::{AppError, ErrorCode};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageInput {
    pub index: usize,
    pub raw_segment: String,
    pub cleaned_hex: Option<String>,
    pub bytes: Option<Vec<u8>>,
    pub error: Option<PassthroughError>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProtocolKind {
    ModbusRtu,
    Dlt645,
    Cjt188,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    Request,
    Response,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FactSource {
    Code,
    Manual,
    AwtTemplate,
    AiExplanation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldValue {
    pub name: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub raw_hex: String,
    pub display_value: String,
    pub source: FactSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterValue {
    pub address: Option<u16>,
    pub identifier: Option<String>,
    pub raw_hex: String,
    pub source: FactSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedRegisterExplanation {
    pub address: Option<u16>,
    pub parameter_code: Option<String>,
    pub parameter_name: Option<String>,
    pub unit: Option<String>,
    pub raw_hex: String,
    pub converted_value: Option<String>,
    pub meaning: Option<String>,
    pub source: FactSource,
    pub warnings: Vec<ParseWarning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChecksumResult {
    pub kind: String,
    pub received: Option<String>,
    pub calculated: String,
    pub valid: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassthroughError {
    pub code: String,
    pub message: String,
}

impl PassthroughError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageParseResult {
    pub index: usize,
    pub role: MessageRole,
    pub raw_segment: String,
    pub cleaned_hex: Option<String>,
    pub protocol: ProtocolKind,
    pub summary: String,
    pub fields: Vec<FieldValue>,
    pub registers: Vec<RegisterValue>,
    pub explanations: Vec<AppliedRegisterExplanation>,
    pub checksum: Option<ChecksumResult>,
    pub warnings: Vec<ParseWarning>,
    pub error: Option<PassthroughError>,
}

pub fn parse_messages(input: &str) -> Vec<MessageParseResult> {
    input::split_hex_messages(input)
        .into_iter()
        .map(|message| {
            let Some(bytes) = message.bytes.as_deref() else {
                return MessageParseResult {
                    index: message.index,
                    role: MessageRole::Request,
                    raw_segment: message.raw_segment,
                    cleaned_hex: message.cleaned_hex,
                    protocol: ProtocolKind::Unknown,
                    summary: "这条报文输入无效，未进入协议解析。".to_owned(),
                    fields: Vec::new(),
                    registers: Vec::new(),
                    explanations: Vec::new(),
                    checksum: None,
                    warnings: Vec::new(),
                    error: message.error,
                };
            };

            if bytes.first() == Some(&0xFE) {
                let wakeup_count = bytes.iter().take_while(|byte| **byte == 0xFE).count();
                let protocol_frame = &bytes[wakeup_count..];
                let dlt = dlt645::parse_dlt645(protocol_frame, wakeup_count).ok();
                let cjt = cjt188::parse_cjt188(protocol_frame, wakeup_count).ok();
                if dlt.as_ref().is_some_and(|frame| frame.checksum_valid) ^ cjt.as_ref().is_some_and(|frame| frame.checksum_valid) {
                    if let Some(frame) = dlt.filter(|frame| frame.checksum_valid) {
                        return standard_frame_result(message, StandardFrameFacts {
                            protocol: ProtocolKind::Dlt645,
                            address: frame.address,
                            control_code: frame.control_code,
                            data: frame.data_raw,
                            received: frame.checksum_received,
                            calculated: frame.checksum_calculated,
                            wakeup_count,
                            address_start: 1,
                            control_start: 8,
                            data_start: 10,
                        });
                    }
                    if let Some(frame) = cjt.filter(|frame| frame.checksum_valid) {
                        return standard_frame_result(message, StandardFrameFacts {
                            protocol: ProtocolKind::Cjt188,
                            address: frame.address,
                            control_code: frame.control_code,
                            data: frame.data,
                            received: frame.checksum_received,
                            calculated: frame.checksum_calculated,
                            wakeup_count,
                            address_start: 2,
                            control_start: 9,
                            data_start: 12,
                        });
                    }
                }
                return MessageParseResult {
                    index: message.index,
                    role: MessageRole::Request,
                    raw_segment: message.raw_segment,
                    cleaned_hex: message.cleaned_hex,
                    protocol: ProtocolKind::Unknown,
                    summary: format!(
                        "检测到 {wakeup_count} 个 FE 唤醒字节，但完整帧结构或校验不足以唯一确认 645/188。"
                    ),
                    fields: vec![FieldValue {
                        name: "wakeupBytes".to_owned(),
                        byte_start: 0,
                        byte_end: wakeup_count,
                        raw_hex: "FE".repeat(wakeup_count),
                        display_value: wakeup_count.to_string(),
                        source: FactSource::Code,
                    }],
                    registers: Vec::new(),
                    explanations: Vec::new(),
                    checksum: None,
                    warnings: vec![ParseWarning {
                        code: "protocol_evidence_conflict".to_owned(),
                        message: "645/188 候选均不成立或同时成立，未强行归类。".to_owned(),
                    }],
                    error: None,
                };
            }

            let envelope = envelope::strip_platform_envelope(bytes).ok();
            let modbus_bytes = envelope
                .as_ref()
                .map_or(bytes, |platform| platform.inner.as_slice());
            let parsed = modbus::parse_modbus(modbus_bytes, envelope.as_ref());
            let summary = if parsed.function_kind == modbus::FunctionKind::Private {
                format!("从站 {} 使用私有功能码 0x{:02X}。", parsed.slave.unwrap_or_default(), parsed.function_code)
            } else {
                format!("从站 {} 执行 Modbus {} 操作。", parsed.slave.unwrap_or_default(), parsed.operation)
            };

            MessageParseResult {
                index: message.index,
                role: MessageRole::Request,
                raw_segment: message.raw_segment,
                cleaned_hex: message.cleaned_hex,
                protocol: ProtocolKind::ModbusRtu,
                summary,
                fields: parsed.fields,
                registers: parsed.registers,
                explanations: Vec::new(),
                checksum: parsed.checksum,
                warnings: parsed.warnings,
                error: None,
            }
        })
        .collect()
}

pub fn parse_message_pairs(
    request_hex: &str,
    response_hex: Option<&str>,
) -> Result<Vec<MessageParseResult>, AppError> {
    let mut requests = parse_messages(request_hex);
    requests
        .iter_mut()
        .for_each(|result| result.role = MessageRole::Request);
    let Some(response_hex) = response_hex.filter(|value| !value.trim().is_empty()) else {
        return Ok(requests);
    };
    let mut responses = parse_messages(response_hex);
    if responses.len() > requests.len() {
        return Err(AppError::new(ErrorCode::InvalidPassthroughInput));
    }
    for (request, response) in requests.iter().zip(responses.iter_mut()) {
        response.role = MessageRole::Response;
        apply_pair_context(request, response);
    }
    requests.extend(responses);
    Ok(requests)
}

fn field_hex<'a>(result: &'a MessageParseResult, name: &str) -> Option<&'a str> {
    result
        .fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| field.raw_hex.as_str())
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn modbus_frame(result: &MessageParseResult) -> Option<Vec<u8>> {
    let bytes = decode_hex(result.cleaned_hex.as_deref()?)?;
    Some(envelope::strip_platform_envelope(&bytes).map_or(bytes.clone(), |value| value.inner))
}

fn apply_pair_context(request: &MessageParseResult, response: &mut MessageParseResult) {
    if request.protocol != response.protocol {
        response.warnings.push(ParseWarning {
            code: "request_response_protocol_mismatch".to_owned(),
            message: "请求与回复协议不一致，未应用请求上下文。".to_owned(),
        });
        return;
    }
    if request.protocol != ProtocolKind::ModbusRtu {
        if field_hex(request, "address") != field_hex(response, "address") {
            response.warnings.push(ParseWarning {
                code: "request_response_address_mismatch".to_owned(),
                message: "请求与回复的仪表地址不一致。".to_owned(),
            });
        }
        let controls_match = field_hex(request, "controlCode")
            .and_then(|value| u8::from_str_radix(value, 16).ok())
            .zip(
                field_hex(response, "controlCode")
                    .and_then(|value| u8::from_str_radix(value, 16).ok()),
            )
            .is_some_and(|(request, response)| request & 0x1F == response & 0x1F);
        if !controls_match {
            response.warnings.push(ParseWarning {
                code: "request_response_control_mismatch".to_owned(),
                message: "请求与回复的控制码功能不一致。".to_owned(),
            });
        }
        let identifier_hex_len = if request.protocol == ProtocolKind::Dlt645 {
            8
        } else {
            4
        };
        let identifiers_match = field_hex(request, "data")
            .zip(field_hex(response, "data"))
            .is_some_and(|(request, response)| {
                request.len() >= identifier_hex_len
                    && response.len() >= identifier_hex_len
                    && request[..identifier_hex_len] == response[..identifier_hex_len]
            });
        if !identifiers_match {
            response.warnings.push(ParseWarning {
                code: "request_response_identifier_mismatch".to_owned(),
                message: "请求与回复的数据标识不一致。".to_owned(),
            });
        }
        return;
    }
    if field_hex(request, "slave") != field_hex(response, "slave")
        || field_hex(request, "function") != field_hex(response, "function")
    {
        response.warnings.push(ParseWarning {
            code: "request_response_mismatch".to_owned(),
            message: "请求与回复的从站地址或功能码不一致，未应用请求上下文。".to_owned(),
        });
        return;
    }
    let (Some(request_frame), Some(response_frame)) =
        (modbus_frame(request), modbus_frame(response))
    else {
        return;
    };
    let start = request_frame
        .get(2..4)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
    let quantity = request_frame
        .get(4..6)
        .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
    let byte_count = response_frame.get(2).copied().map(usize::from);
    let data = byte_count
        .and_then(|count| response_frame.get(3..3 + count))
        .map(<[u8]>::to_vec);
    let (Some(start), Some(quantity), Some(data)) = (start, quantity, data) else {
        return;
    };
    if data.len() != usize::from(quantity) * 2 {
        response.warnings.push(ParseWarning {
            code: "request_response_quantity_mismatch".to_owned(),
            message: "回复数据长度与请求寄存器数量不一致，未应用请求上下文。".to_owned(),
        });
        return;
    }
    response.registers = data
        .chunks_exact(2)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            Some(RegisterValue {
                address: start.checked_add(u16::try_from(offset).ok()?),
                identifier: None,
                raw_hex: format!("{:02X}{:02X}", bytes[0], bytes[1]),
                source: FactSource::Code,
            })
        })
        .collect();
    response
        .warnings
        .retain(|warning| warning.code != "response_without_request_context");
}

struct StandardFrameFacts {
    protocol: ProtocolKind,
    address: String,
    control_code: u8,
    data: Vec<u8>,
    received: u8,
    calculated: u8,
    wakeup_count: usize,
    address_start: usize,
    control_start: usize,
    data_start: usize,
}

fn standard_frame_result(message: MessageInput, facts: StandardFrameFacts) -> MessageParseResult {
    let StandardFrameFacts {
        protocol,
        address,
        control_code,
        data,
        received,
        calculated,
        wakeup_count,
        address_start,
        control_start,
        data_start,
    } = facts;
    let protocol_name = match protocol {
        ProtocolKind::Dlt645 => "DL/T 645",
        ProtocolKind::Cjt188 => "CJ/T 188",
        _ => "标准",
    };
    MessageParseResult {
        index: message.index,
        role: MessageRole::Request,
        raw_segment: message.raw_segment,
        cleaned_hex: message.cleaned_hex,
        protocol,
        summary: format!("{protocol_name} 报文，仪表地址 {address}，控制码 0x{control_code:02X}。"),
        fields: vec![
            FieldValue {
                name: "wakeupBytes".to_owned(),
                byte_start: 0,
                byte_end: wakeup_count,
                raw_hex: "FE".repeat(wakeup_count),
                display_value: wakeup_count.to_string(),
                source: FactSource::Code,
            },
            FieldValue {
                name: "address".to_owned(),
                byte_start: wakeup_count + address_start,
                byte_end: wakeup_count
                    + address_start
                    + if matches!(protocol, ProtocolKind::Dlt645) {
                        6
                    } else {
                        7
                    },
                raw_hex: address.clone(),
                display_value: address,
                source: FactSource::Code,
            },
            FieldValue {
                name: "controlCode".to_owned(),
                byte_start: wakeup_count + control_start,
                byte_end: wakeup_count + control_start + 1,
                raw_hex: format!("{control_code:02X}"),
                display_value: format!("0x{control_code:02X}"),
                source: FactSource::Code,
            },
            FieldValue {
                name: "data".to_owned(),
                byte_start: wakeup_count + data_start,
                byte_end: wakeup_count + data_start + data.len(),
                raw_hex: data.iter().map(|byte| format!("{byte:02X}")).collect(),
                display_value: format!("{} 字节", data.len()),
                source: FactSource::Code,
            },
        ],
        registers: Vec::new(),
        explanations: Vec::new(),
        checksum: Some(ChecksumResult {
            kind: "additive8".to_owned(),
            received: Some(format!("{received:02X}")),
            calculated: format!("{calculated:02X}"),
            valid: Some(received == calculated),
        }),
        warnings: Vec::new(),
        error: None,
    }
}

#[cfg(test)]
mod pair_tests;
