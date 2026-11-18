use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, percentage, sage_time, sage_uint as unsigned, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::{fields, model_condition_index};
use super::passive_area::reference;
use super::secondary_damage::encode_status;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const UINT: u32 = 0x7631_77a4;
const PERCENTAGE: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const CONDITION: u32 = 0x1572_3e0b;
const EVA: u32 = 0xc4ec_43b1;
const AUDIO: u32 = 0x63d7_b468;
const STATUS: &str = "ObjectStatusValidation";

pub(super) fn miner(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x90f9_97ef;
    const CAPACITY: &str = "DeliveryCapacity";
    const DELIVERY_TIME: &str = "DeliveryTime";
    let path = data.path;
    validate_attributes(node, &["id", CAPACITY, DELIVERY_TIME], path)?;
    let mut children = significant_children(node).peekable();
    let status = children.next_if(|child| child.has_tag_name((SAGE_NS, STATUS)));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected OreMinerBehavior child"));
    }

    let start = data.allocate(pointer, 0x14)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    let create = base(0x30a3_6b8d, Table::record(vec![behavior]));
    data.word(
        start + 8,
        unsigned(node.attribute(CAPACITY).unwrap_or("1000"), CAPACITY, path)?,
    );
    data.word(
        start + 0xc,
        sage_time(
            node.attribute(DELIVERY_TIME).unwrap_or("1s"),
            DELIVERY_TIME,
            path,
        )?
        .to_bits(),
    );
    let status = match status {
        Some(child) => Target::Table(encode_status(child, start + 0x10, data)?),
        None => Target::Null,
    };
    let entries = vec![
        create,
        Entry::named(CAPACITY, UINT, Target::Data(start + 8)),
        Entry::named(DELIVERY_TIME, TIME, Target::Data(start + 0xc)),
        Entry::named(STATUS, 0x3e8f_938c, status),
    ];
    Ok((KIND, Table::record(entries)))
}

pub(super) fn deposit(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x8e22_0ee2;
    const LOW: &str = "LowNotificationPercentage";
    const LOW_EVA: &str = "LowNotificationEvaEventRef";
    const DEPLETED_EVA: &str = "DepletedNotificationEvaEventRef";
    const DEPLETED_AUDIO: &str = "DepletedNotificationAudioEventInfoRef";
    const FIELDS: [(&str, u32, usize); 16] = [
        ("MaximumGatheredValue", UINT, 0x08),
        ("DeliveryAmount", UINT, 0x0c),
        ("DeliveryAmountWhenEmpty", UINT, 0x10),
        ("CashMultiplier", PERCENTAGE, 0x14),
        ("MoveToExtractTime", TIME, 0x18),
        ("ExtractTime", TIME, 0x1c),
        ("MoveToDeliverTime", TIME, 0x20),
        ("DeliveryTime", TIME, 0x24),
        ("ExtractAnimation", CONDITION, 0x28),
        ("DeliveryAnimation", CONDITION, 0x2c),
        ("ReturnAnimation", CONDITION, 0x30),
        ("MoveBackAnimation", CONDITION, 0x34),
        (LOW, PERCENTAGE, 0x38),
        (LOW_EVA, EVA, 0x3c),
        (DEPLETED_EVA, EVA, 0x40),
        (DEPLETED_AUDIO, AUDIO, 0x44),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected OreNodeBehavior content"));
    }
    let start = data.allocate(pointer, 0x48)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    let defaults = ["20000", "1000", "0", "100%", "1s", "1s", "1s", "1s"];
    for ((name, kind, offset), default) in FIELDS.into_iter().zip(defaults) {
        let value = node.attribute(name).unwrap_or(default);
        let word = match kind {
            UINT => unsigned(value, name, path)?,
            PERCENTAGE => percentage(value, name, path)?.to_bits(),
            TIME => sage_time(value, name, path)?.to_bits(),
            _ => unreachable!("ore node default field"),
        };
        data.word(start + offset, word);
    }
    for (name, _, offset) in &FIELDS[8..12] {
        if let Some(flag) = node.attribute(*name) {
            let value = match flag {
                "INVALID" => u32::MAX,
                _ => model_condition_index(flag, path)? as u32,
            };
            data.word(start + offset, value);
        }
    }
    data.word(
        start + 0x38,
        percentage(required(node, LOW, path)?, LOW, path)?.to_bits(),
    );
    for name in [LOW_EVA, DEPLETED_EVA, DEPLETED_AUDIO] {
        if node.attribute(name).is_none() {
            return Err(invalid(path, format!("expected {name} attribute")));
        }
    }
    // EA assigns reference ordinals in XML attribute order.
    for attribute in node.attributes() {
        let value = attribute.value();
        let (offset, ordinal) = match attribute.name() {
            LOW_EVA | DEPLETED_EVA => {
                let offset = if attribute.name() == LOW_EVA {
                    0x3c
                } else {
                    0x40
                };
                (
                    offset,
                    reference(value, "EvaEvent", path, references, reference_types)?,
                )
            }
            DEPLETED_AUDIO => (
                0x44,
                super::audio_reference(value, path, references, reference_types)?,
            ),
            _ => continue,
        };
        data.word(start + offset, ordinal);
    }
    let mut entries = vec![behavior];
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}
