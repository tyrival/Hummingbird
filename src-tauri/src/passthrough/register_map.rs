use super::{FactSource, ParseWarning, RegisterValue};
use crate::error::{AppError, ErrorCode};
use crate::naming::NamingCatalog;
use crate::register_csv::{parse_register_address, CSV_HEADER};
use csv::ReaderBuilder;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct RegisterDefinition {
    pub data_name: String,
    pub meaning: Option<String>,
    pub unit: String,
    pub address: u16,
    pub register_type: u8,
    pub endian: u8,
    pub decimals: u32,
    pub scale: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RegisterMap {
    definitions: BTreeMap<u16, Vec<RegisterDefinition>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterExplanation {
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

impl RegisterMap {
    pub fn definition_count(&self) -> usize {
        self.definitions.values().map(Vec::len).sum()
    }

    pub fn contains_address(&self, address: u16) -> bool {
        self.definitions.contains_key(&address)
    }
    pub fn from_awt_csv(input: &str, catalog: &NamingCatalog) -> Result<Self, AppError> {
        let mut reader = ReaderBuilder::new()
            .flexible(true)
            .from_reader(input.trim_start_matches('\u{feff}').as_bytes());
        let headers = reader
            .headers()
            .map_err(|_| AppError::new(ErrorCode::InvalidPassthroughSource))?
            .clone();
        let expected = CSV_HEADER.split(',').collect::<Vec<_>>();
        if headers.len() < expected.len()
            || headers
                .iter()
                .take(expected.len())
                .map(str::trim)
                .ne(expected.iter().copied())
        {
            return Err(AppError::new(ErrorCode::InvalidPassthroughSource));
        }
        let index = |name: &str| headers.iter().position(|header| header.trim() == name);
        let mut definitions = BTreeMap::<u16, Vec<RegisterDefinition>>::new();
        for record in reader.records() {
            let record = record.map_err(|_| AppError::new(ErrorCode::InvalidPassthroughSource))?;
            if record.len() < expected.len() {
                return Err(AppError::new(ErrorCode::InvalidPassthroughSource));
            }
            let value = |name: &str| {
                index(name)
                    .and_then(|position| record.get(position))
                    .unwrap_or("")
                    .trim()
            };
            let address = parse_register_address(value("reg_add"))
                .and_then(|address| u16::try_from(address).ok())
                .ok_or_else(|| AppError::new(ErrorCode::InvalidPassthroughSource))?;
            let data_name = value("data_name").to_owned();
            let meaning = catalog
                .entries
                .iter()
                .find(|entry| entry.code == data_name)
                .map(|entry| entry.meaning.clone());
            let register_type = value("reg_type")
                .parse::<u8>()
                .ok()
                .filter(|value| (1..=12).contains(value))
                .ok_or_else(|| AppError::new(ErrorCode::InvalidPassthroughSource))?;
            let endian = value("endian")
                .parse::<u8>()
                .ok()
                .filter(|value| valid_endian(register_type, *value))
                .ok_or_else(|| AppError::new(ErrorCode::InvalidPassthroughSource))?;
            definitions
                .entry(address)
                .or_default()
                .push(RegisterDefinition {
                    data_name,
                    meaning,
                    unit: value("unit").to_owned(),
                    address,
                    register_type,
                    endian,
                    decimals: value("dcm").parse().unwrap_or(0),
                    scale: value("k").parse().unwrap_or(1.0),
                });
        }
        Ok(Self { definitions })
    }

    pub fn explain(&self, registers: &[RegisterValue]) -> Vec<RegisterExplanation> {
        registers
            .iter()
            .flat_map(|register| {
                let definitions = register
                    .address
                    .and_then(|address| self.definitions.get(&address));
                match definitions {
                    Some(definitions) => definitions
                        .iter()
                        .map(|definition| explanation(registers, register, definition))
                        .collect(),
                    None => vec![RegisterExplanation {
                        address: register.address,
                        parameter_code: None,
                        parameter_name: None,
                        unit: None,
                        raw_hex: register.raw_hex.clone(),
                        converted_value: None,
                        meaning: None,
                        source: FactSource::Code,
                        warnings: vec![ParseWarning {
                            code: "register_not_found".to_owned(),
                            message: "资料中未找到对应寄存器。".to_owned(),
                        }],
                    }],
                }
            })
            .collect()
    }
}

fn valid_endian(register_type: u8, endian: u8) -> bool {
    match register_type {
        1 => endian <= 15,
        2 => endian <= 7,
        3 => endian <= 3,
        4..=7 | 10 | 11 => endian <= 1,
        8 | 9 | 12 => endian <= 3,
        _ => false,
    }
}

fn explanation(
    registers: &[RegisterValue],
    register: &RegisterValue,
    definition: &RegisterDefinition,
) -> RegisterExplanation {
    let mut warnings = Vec::new();
    let raw_hex = collect_register_hex(
        registers,
        definition.address,
        register_count(definition.register_type),
    );
    let converted_value = raw_hex
        .as_deref()
        .filter(|raw| !raw.is_empty())
        .and_then(|raw| convert_value(raw, definition));
    if raw_hex.as_deref().is_some_and(|raw| !raw.is_empty()) && converted_value.is_none() {
        warnings.push(ParseWarning {
            code: "unsupported_register_conversion".to_owned(),
            message: format!(
                "无法按 reg_type={} endian={} 换算当前原始值。",
                definition.register_type, definition.endian
            ),
        });
    }
    if definition.meaning.is_none() {
        warnings.push(ParseWarning {
            code: "parameter_name_not_found".to_owned(),
            message: "内置参量目录中未找到说明。".to_owned(),
        });
    }
    RegisterExplanation {
        address: Some(definition.address),
        parameter_code: Some(definition.data_name.clone()),
        parameter_name: definition
            .meaning
            .clone()
            .or_else(|| definition.data_name.strip_prefix("DOC_").map(str::to_owned)),
        unit: (!definition.unit.is_empty()).then(|| definition.unit.clone()),
        raw_hex: raw_hex.unwrap_or_else(|| register.raw_hex.clone()),
        converted_value,
        meaning: match (definition.register_type, definition.endian) {
            (4 | 5, 0) => Some("高字节".to_owned()),
            (4 | 5, 1) => Some("低字节".to_owned()),
            _ => None,
        },
        source: FactSource::AwtTemplate,
        warnings,
    }
}

fn register_count(register_type: u8) -> usize {
    match register_type {
        8 | 9 | 12 => 2,
        10 | 11 => 4,
        _ => 1,
    }
}

fn collect_register_hex(registers: &[RegisterValue], start: u16, count: usize) -> Option<String> {
    (0..count)
        .map(|offset| {
            let address = start.checked_add(u16::try_from(offset).ok()?)?;
            registers
                .iter()
                .find(|register| register.address == Some(address))
                .map(|register| register.raw_hex.as_str())
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.concat())
}

fn convert_value(raw_hex: &str, definition: &RegisterDefinition) -> Option<String> {
    let bytes = decode_hex(raw_hex)?;
    let ordered = reorder_bytes(&bytes, definition.register_type, definition.endian)?;
    let value = match definition.register_type {
        1 => f64::from((u16::from_be_bytes([bytes[0], bytes[1]]) >> definition.endian) & 1),
        2 => {
            f64::from((u16::from_be_bytes([bytes[0], bytes[1]]) >> (definition.endian * 2)) & 0x03)
        }
        3 => {
            f64::from((u16::from_be_bytes([bytes[0], bytes[1]]) >> (definition.endian * 4)) & 0x0F)
        }
        4 => f64::from(ordered[0]),
        5 => f64::from(i8::from_ne_bytes([ordered[0]])),
        6 => f64::from(u16::from_be_bytes(ordered.try_into().ok()?)),
        7 => f64::from(i16::from_be_bytes(ordered.try_into().ok()?)),
        8 => f64::from(u32::from_be_bytes(ordered.try_into().ok()?)),
        9 => f64::from(i32::from_be_bytes(ordered.try_into().ok()?)),
        10 => u64::from_be_bytes(ordered.try_into().ok()?) as f64,
        11 => i64::from_be_bytes(ordered.try_into().ok()?) as f64,
        12 => f64::from(f32::from_be_bytes(ordered.try_into().ok()?)),
        _ => return None,
    } * definition.scale
        / 10_f64.powi(definition.decimals as i32);
    Some(format!(
        "{value:.precision$}",
        precision = definition.decimals as usize
    ))
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn reorder_bytes(bytes: &[u8], register_type: u8, endian: u8) -> Option<Vec<u8>> {
    let order: &[usize] = match (register_type, endian, bytes.len()) {
        (4 | 5, 0, 2) => &[0],
        (4 | 5, 1, 2) => &[1],
        (6 | 7, 0, 2) => &[1, 0],
        (6 | 7, 1, 2) => &[0, 1],
        (8 | 9 | 12, 0, 4) => &[0, 1, 2, 3],
        (8 | 9 | 12, 1, 4) => &[3, 2, 1, 0],
        (8 | 9 | 12, 2, 4) => &[2, 3, 0, 1],
        (8 | 9 | 12, 3, 4) => &[1, 0, 3, 2],
        (10 | 11, 0, 8) => &[7, 6, 5, 4, 3, 2, 1, 0],
        (10 | 11, 1, 8) => &[0, 1, 2, 3, 4, 5, 6, 7],
        (1..=3, _, 2) => &[0, 1],
        _ => return None,
    };
    Some(order.iter().map(|index| bytes[*index]).collect())
}

#[cfg(test)]
mod tests {
    use super::{reorder_bytes, RegisterMap};
    use crate::naming::{NamingCatalog, NamingEntry};
    use crate::passthrough::{FactSource, RegisterValue};
    use std::collections::HashSet;

    fn catalog() -> NamingCatalog {
        NamingCatalog {
            entries: vec![NamingEntry {
                code: "voltage_a".to_owned(),
                meaning: "A相电压".to_owned(),
            }],
            names: HashSet::from(["voltage_a".to_owned()]),
            reference: String::new(),
        }
    }

    #[test]
    fn maps_a_standard_twelve_column_awt_record() {
        let csv = "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style\n1,1,voltage_a,V,0x0354,6,1,1,1,3,,\n";
        let map = RegisterMap::from_awt_csv(csv, &catalog()).unwrap();
        let explanations = map.explain(&[RegisterValue {
            address: Some(0x0354),
            identifier: None,
            raw_hex: "08FC".to_owned(),
            source: FactSource::Code,
        }]);
        assert_eq!(explanations[0].parameter_name.as_deref(), Some("A相电压"));
        assert_eq!(explanations[0].converted_value.as_deref(), Some("230.0"));
    }

    #[test]
    fn rejects_a_non_awt_header() {
        let error = RegisterMap::from_awt_csv("address,name\n1,x\n", &catalog());
        assert!(error.is_err());
    }

    #[test]
    fn ignores_columns_after_the_twelve_column_awt_contract() {
        let csv = "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style,legacy_style,note\n1,1,voltage_a,V,20,6,0,1,1,3,,0,,ignored\n";
        let map = RegisterMap::from_awt_csv(csv, &catalog()).unwrap();
        assert!(map.contains_address(20));
    }

    #[test]
    fn rejects_reordered_columns_inside_the_awt_contract() {
        let csv = "group,id,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style\n1,1,voltage_a,V,20,6,0,1,1,3,,0\n";
        assert!(RegisterMap::from_awt_csv(csv, &catalog()).is_err());
    }

    #[test]
    fn reorders_each_multibyte_type_according_to_the_awt_endian_codes() {
        let two = [0x12, 0x34];
        assert_eq!(reorder_bytes(&two, 6, 0), Some(vec![0x34, 0x12]));
        assert_eq!(reorder_bytes(&two, 7, 1), Some(vec![0x12, 0x34]));

        let four = [0x11, 0x22, 0x33, 0x44];
        for register_type in [8, 9, 12] {
            assert_eq!(
                reorder_bytes(&four, register_type, 0),
                Some(vec![0x11, 0x22, 0x33, 0x44])
            );
            assert_eq!(
                reorder_bytes(&four, register_type, 1),
                Some(vec![0x44, 0x33, 0x22, 0x11])
            );
            assert_eq!(
                reorder_bytes(&four, register_type, 2),
                Some(vec![0x33, 0x44, 0x11, 0x22])
            );
            assert_eq!(
                reorder_bytes(&four, register_type, 3),
                Some(vec![0x22, 0x11, 0x44, 0x33])
            );
        }

        let eight = [1, 2, 3, 4, 5, 6, 7, 8];
        for register_type in [10, 11] {
            assert_eq!(
                reorder_bytes(&eight, register_type, 0),
                Some(vec![8, 7, 6, 5, 4, 3, 2, 1])
            );
            assert_eq!(
                reorder_bytes(&eight, register_type, 1),
                Some(eight.to_vec())
            );
        }
    }

    #[test]
    fn expands_adw300_packed_time_period_fields_at_the_same_address() {
        let csv = include_str!("../../../tests/fixtures/passthrough/adw300-time-period.csv");
        let map = RegisterMap::from_awt_csv(csv, &catalog()).unwrap();
        assert_eq!(map.definition_count(), 36);
        assert!(map.contains_address(0x017E));
        let explanations = map.explain(&[RegisterValue {
            address: Some(0x016D),
            identifier: None,
            raw_hex: "0100".to_owned(),
            source: FactSource::Code,
        }]);
        assert_eq!(explanations.len(), 2);
        assert_eq!(
            explanations[0].parameter_name.as_deref(),
            Some("第1时段费率号")
        );
        assert_eq!(explanations[0].converted_value.as_deref(), Some("1"));
        assert_eq!(explanations[0].meaning.as_deref(), Some("高字节"));
        assert_eq!(
            explanations[1].parameter_name.as_deref(),
            Some("第1时段起始分")
        );
        assert_eq!(explanations[1].converted_value.as_deref(), Some("0"));
        assert_eq!(explanations[1].meaning.as_deref(), Some("低字节"));
    }

    #[test]
    fn converts_modbus_voltage_words_without_swapping_their_bytes() {
        let csv = "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style,style\n1,1,DOC_A相电压,V,20,6,1,1,1,3,,0,\n2,1,DOC_B相电压,V,21,6,1,1,1,3,,0,\n3,1,DOC_C相电压,V,22,6,1,1,1,3,,0,\n";
        let map = RegisterMap::from_awt_csv(csv, &catalog()).unwrap();
        let explanations = map.explain(&[
            RegisterValue {
                address: Some(20),
                identifier: None,
                raw_hex: "089D".to_owned(),
                source: FactSource::Code,
            },
            RegisterValue {
                address: Some(21),
                identifier: None,
                raw_hex: "0896".to_owned(),
                source: FactSource::Code,
            },
            RegisterValue {
                address: Some(22),
                identifier: None,
                raw_hex: "08A2".to_owned(),
                source: FactSource::Code,
            },
        ]);
        assert_eq!(
            explanations
                .iter()
                .map(|item| item.converted_value.as_deref())
                .collect::<Vec<_>>(),
            [Some("220.5"), Some("219.8"), Some("221.0")]
        );
        assert!(explanations
            .iter()
            .all(|item| item.unit.as_deref() == Some("V")));
    }
}
