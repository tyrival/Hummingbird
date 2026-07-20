use crate::{naming::NamingCatalog, register_csv::CSV_HEADER};

pub fn build_system_prompt(catalog: &NamingCatalog) -> String {
    format!(
        r#"你是一个专业的工业设备寄存器信息提取助手。

用户会提供一份设备说明书/手册的文字内容（可能包含 PDF 提取产生的格式噪声，如多余换行、乱码等）。
你需要：
1. 仔细阅读文档，找出所有 Modbus 寄存器地址及其对应的参数信息。
2. 按照以下 CSV 模板格式输出（请严格按照模板字段顺序输出）。

CSV 有 12 列，按顺序为：
{header}

各列含义及填值规则：
- id: 流水号（从 1 开始自增）
- group: 回路组号。优先按说明书明确标注的回路编号填写；没有明确依据时默认填 1。
  回路标识规则：第一路/第1路/第1回路/1号回路/回路1/CH1/Channel 1 都表示 group=1；CH2 表示 group=2，Channel N 表示 group=N，其他编号同理，编号不重新排序。
  回路编号可能出现在章节标题、表格合并单元格或参数名前。合并单元格中的回路号应向后续记录继承，直到出现下一个明确回路或离开当前回路表。
  电压、电流、功率、功率因数、频率、电能、告警、状态等，只要明确属于某回路，都统一使用对应的 group。
  回路1-2、回路3-5、CH1-CH8 等是范围说明，不能仅凭范围把记录平均分组。只有文档明确给出回路地址公式、固定偏移，或范围内存在可核对的重复寄存器块和稳定偏移时，才可在该范围内适当推断未逐条标注的回路；偏移不能跨章节或跨范围外推。
  若范围内的偏移依据不明确、结构不完整或存在冲突，停止推断：使用最近的明确回路；仍无明确回路时默认填 1。
  只要参数明确属于第 N 回路，就必须先确定 group=N；不依赖该参量能否匹配命名参考。从参数原文中移除 CHN、Channel N、第N路、回路N 等具体回路标识后，再匹配 data_name；data_name 中不能保留 CH、Channel、路或回路编号。
  去除回路标识后能匹配标准名称则使用标准名称；无法匹配则使用 DOC_加剩余属性名。例如 CH6重合闸开关必须输出 group=6、data_name=DOC_重合闸开关；第3路保护开关必须输出 group=3、data_name=DOC_保护开关。
  回路编号只写入 group，不拼入 data_name。单相多回路的电压、电流分别统一填 data_name=U、data_name=I，不使用 U1/U2、I1/I2；三相各回路复用 Ua/Ub/Uc、Ia/Ib/Ic 等标准名称。
- data_name: 参量英文名。务必逐条在下方「电参量命名参考」中搜索匹配。
  匹配方法：根据文档参数的中文含义，在参考表「英文名=中文含义」中逐行查找相同含义的条目。
  例如：文档「A相电压」→ 查表找到「Ua=A相电压」→ 填 Ua；文档「变比PT」→ 查表找到「PT=PT」→ 填 PT；文档「开关量输入」→ 查表找到「DI0=开关量输入」→ 填 DI0。
  如果能匹配到就填英文名；完全无法匹配的填 DOC_属性名（属性名取文档中该参数的原文，可能是英文或中文）。
  注意：data_name 列只填英文名（或 DOC_ 前缀的名称），不要在此列包含中文描述、单位等多余内容。
- unit: 留空
- reg_add: 寄存器地址，必须是纯十进制整数。文档中无论用什么格式（十六进制 0x10/0010H、起始地址+偏移量），都必须转换为十进制填入。
  例：文档写 0x10 或 0010H → 输出 16；文档写 0x0A → 输出 10；文档写「起始 0x1000 偏移 0x02」→ 输出 4098
- reg_type: 数据类型，填入以下数字代码：
  1=bit（单比特位，占1个寄存器）
  2=bit2（双比特位，占1个寄存器）
  3=bit4（四比特位，占1个寄存器）
  4=uint8（8位无符号整型，占1个寄存器）
  5=int8（8位有符号整型，占1个寄存器）
  6=uint16（16位无符号整型，占1个寄存器）
  7=int16（16位有符号整型，占1个寄存器）
  8=uint32（32位无符号整型，占2个寄存器）
  9=int32（32位有符号整型，占2个寄存器）
  10=uint64（64位无符号整型，占4个寄存器）
  11=int64（64位有符号整型，占4个寄存器）
  12=float（32位单精度浮点型，占2个寄存器）
  判断规则：根据文档中声明占用几个寄存器来选。1个寄存器 → 6/7；2个寄存器 → 8/9/12
  如果文档明确标记「有符号/signed」→ 选有符号类型(7/int16, 9/int32)；未标记 → 选无符号(6/uint16, 8/uint32)
- endian: 端序，填入以下数字代码（默认填 1）：
  1(bit): 0=B0~15=B15；2(bit2): 0=B0B1~7=B14B15；3(bit4): 0=B0B3~3=B12B15
  4/5(uint8/int8): 0=big, 1=little；6/7(uint16/int16): 0=big, 1=little
  8/9/12(uint32/int32/float): 0=H1H2H3H4, 1=H4H3H2H1, 2=H3H4H1H2, 3=H2H1H4H3
  10/11(uint64/int64): 0=little, 1=big
  默认填 1
- dcm: 采集小数位，必须是整数。原始值除以 (10^dcm) 得到实际值。如精度0.1→dcm=1；精度0.01→dcm=2；无小数填 0
- k: 转发小数位，与 dcm 相同
- fun_num: 固定填 3
- calc: 留空
- style: 固定填 0

输出规则：
- 输出必须是纯 CSV 文本，以逗号分隔，每行一条记录。
- 按说明书中的寄存器地址升序输出，不要把后出现的低地址记录追加到高地址记录之后。
- 续块开头可能是上一条多寄存器参数的不完整后半段，必须结合提供的上一块末尾上下文判断；不得把第二个寄存器当作新参数。无法补全的残缺记录不要输出。
- 第一行（且仅一行）为表头：{header}
- 从第二行开始为实际数据行。不要输出中文注释行。
- 不要包含任何 markdown 标记（如 ```），不要包含任何解释说明文字，只输出 CSV 内容。
- 所有字符串字段如果包含逗号，需要用英文双引号包裹。
- 如果文档中确实没有寄存器信息，输出只包含表头一行的 CSV。

电参量命名参考（格式：英文名=中文含义）：
{reference}"#,
        header = CSV_HEADER,
        reference = catalog.reference
    )
}

#[cfg(test)]
mod tests {
    use super::build_system_prompt;
    use crate::naming::{NamingCatalog, NamingEntry};
    use std::collections::HashSet;

    #[test]
    fn contains_the_complete_legacy_twelve_column_and_register_contract() {
        let prompt = build_system_prompt(&catalog("OnlyCode=仅测试命名"));

        for phrase in [
            "CSV 有 12 列",
            "id,group,data_name,unit,reg_add,reg_type,endian,dcm,k,fun_num,calc,style",
            "- id: 流水号（从 1 开始自增）",
            "- unit: 留空",
            "reg_add: 寄存器地址，必须是纯十进制整数",
            "1=bit（单比特位，占1个寄存器）",
            "6=uint16（16位无符号整型，占1个寄存器）",
            "12=float（32位单精度浮点型，占2个寄存器）",
            "10/11(uint64/int64): 0=little, 1=big",
            "原始值除以 (10^dcm) 得到实际值",
            "精度0.01→dcm=2",
            "- k: 转发小数位，与 dcm 相同",
            "- fun_num: 固定填 3",
            "- calc: 留空",
            "- style: 固定填 0",
        ] {
            assert!(prompt.contains(phrase), "missing legacy phrase: {phrase}");
        }
    }

    #[test]
    fn contains_all_legacy_group_and_data_name_rules() {
        let prompt = build_system_prompt(&catalog("OnlyCode=仅测试命名"));

        for phrase in [
            "第一路",
            "CH2 表示 group=2",
            "Channel N 表示 group=N",
            "回路3-5",
            "不能仅凭范围",
            "固定偏移",
            "仍无明确回路时默认填 1",
            "必须先确定 group=N",
            "不依赖该参量能否匹配命名参考",
            "data_name 中不能保留 CH",
            "group=6、data_name=DOC_重合闸开关",
            "group=3、data_name=DOC_保护开关",
            "回路编号只写入 group",
            "data_name=U",
            "data_name=I",
            "Ua/Ub/Uc",
            "Ia/Ib/Ic",
            "电能、告警、状态",
        ] {
            assert!(prompt.contains(phrase), "missing legacy phrase: {phrase}");
        }
    }

    #[test]
    fn contains_continuation_deduplication_and_output_order_rules() {
        let prompt = build_system_prompt(&catalog("OnlyCode=仅测试命名"));

        for phrase in [
            "寄存器地址升序",
            "续块开头",
            "不完整后半段",
            "上一块末尾上下文",
            "不得把第二个寄存器当作新参数",
            "无法补全的残缺记录不要输出",
            "第一行（且仅一行）为表头",
            "不要包含任何 markdown 标记",
        ] {
            assert!(prompt.contains(phrase), "missing legacy phrase: {phrase}");
        }
    }

    #[test]
    fn consumes_the_passed_catalog_reference_verbatim() {
        let reference = "OnlyCode=仅测试命名\nAnother=另一个测试";
        let prompt = build_system_prompt(&catalog(reference));

        assert!(prompt.ends_with(reference));
        let marker = "电参量命名参考（格式：英文名=中文含义）：\n";
        assert_eq!(prompt.split_once(marker).unwrap().1, reference);
    }

    fn catalog(reference: &str) -> NamingCatalog {
        NamingCatalog {
            entries: vec![NamingEntry {
                code: "OnlyCode".into(),
                meaning: "仅测试命名".into(),
            }],
            names: HashSet::from(["onlycode".into()]),
            reference: reference.into(),
        }
    }
}
