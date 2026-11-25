use std::io;

use roxmltree::Node;

use super::super::binary::BinaryData;
use super::super::global_ui::kind_of_flags;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::client_behaviors::audio_reference;
use super::draws::{boolean, fields};
use super::parking::encode_filter;
use super::passive_area::reference;
use crate::compile::SAGE_NS;
use crate::compile::{has_content, invalid};

const KIND: u32 = 0x2027_962a;
const FIELDS: [(&str, u32, usize); 12] = [
    ("KindOf", 0x9afc_9193, 8),
    ("KindOfNot", 0x9afc_9193, 0x2c),
    ("IsForbidOwnerPlayer", 0x77e3_f4f7, 0x64),
    ("IsBuildingPickup", 0x77e3_f4f7, 0x65),
    ("IsHumanOnlyPickup", 0x77e3_f4f7, 0x66),
    ("IsHumanOnlyPickupInSinglePlayerCampaign", 0x77e3_f4f7, 0x67),
    ("ExecuteFX", 0x0d1c_8e83, 0x50),
    ("ExecutionAnimationTemplate", 0x148e_d75d, 0x54),
    ("ExecuteAnimationDisplayTimeInSeconds", 0x6458_64f3, 0x58),
    ("ExecuteAnimationZRisePerSecond", 0x6458_64f3, 0x5c),
    ("ExecuteAnimationFades", 0x77e3_f4f7, 0x68),
    ("ExecuteAudioEvent", 0x63d7_b468, 0x60),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    validate_crate_attributes(node, &[], data)?;

    if has_content(node) {
        return Err(invalid(data.path, "unexpected CrateCollide content"));
    }
    let start = data.allocate(pointer, 0x6c)?;
    let table = encode_base(node, KIND, start, data, references, reference_types)?;
    Ok((KIND, table))
}

fn encode_base(
    node: Node<'_, '_>,
    kind: u32,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let behavior = module_base(node, kind, start, data, false)?;
    for &(name, field_kind, offset) in &FIELDS {
        let value = node.attribute(name).unwrap_or("");
        match field_kind {
            0x9afc_9193 => {
                let flags = kind_of_flags(value, path)?;
                for (index, word) in flags.into_iter().enumerate() {
                    data.word(start + offset + index * 4, word);
                }
            }
            0x77e3_f4f7 => {
                let default = name == "IsHumanOnlyPickupInSinglePlayerCampaign";
                data.payload[start + offset] = boolean(node, name, default, path)?;
            }
            0x6458_64f3 => {
                let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
                data.word(start + offset, value.to_bits());
            }
            _ => {}
        }
    }

    for attribute in node.attributes() {
        let value = attribute.value();
        match attribute.name() {
            "ExecuteFX" => {
                let ordinal = reference(value, "FXList", path, references, reference_types)?;
                data.word(start + 0x50, ordinal);
            }
            "ExecutionAnimationTemplate" if !value.trim().is_empty() => {
                let name = value.split_once('\\').map_or(value, |(name, _)| name);
                let asset = asset_reference(name, "GameObject", path)?;
                data.word(start + 0x54, asset.instance_id);
                reference_types.push(asset.type_id);
            }
            "ExecuteAudioEvent" => {
                let ordinal = audio_reference(value, path, references, reference_types)?;
                data.word(start + 0x60, ordinal);
            }
            _ => {}
        }
    }
    if node.attribute("ExecuteAudioEvent").is_none() {
        let ordinal = audio_reference("CratePickup", path, references, reference_types)?;
        data.word(start + 0x60, ordinal);
    }

    let mut entries = vec![base(0x70e5_c103, Table::record(vec![behavior]))];
    entries.extend(fields(start, &FIELDS));
    Ok(Table::record(entries))
}

pub(super) fn encode_money(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const MONEY: u32 = 0xef29_3c44;
    validate_crate_attributes(node, &["MoneyProvided"], data)?;
    if has_content(node) {
        return Err(invalid(data.path, "unexpected MoneyCrateCollide content"));
    }
    let start = data.allocate(pointer, 0x70)?;
    let crate_table = encode_base(node, MONEY, start, data, references, reference_types)?;
    let amount = node
        .attribute("MoneyProvided")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(data.path, "invalid MoneyProvided"))?;
    data.word(start + 0x6c, amount);
    Ok((
        MONEY,
        Table::record(vec![
            base(KIND, crate_table),
            Entry::named("MoneyProvided", 0x7631_77a4, Target::Data(start + 0x6c)),
        ]),
    ))
}

fn validate_crate_attributes(
    node: Node<'_, '_>,
    extra: &[&str],
    data: &BinaryData<'_>,
) -> io::Result<()> {
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.iter().map(|&(name, ..)| name))
        .chain(extra.iter().copied())
        .collect();
    validate_attributes(node, &allowed, data.path)
}

pub(super) fn encode_heal(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const HEAL: u32 = 0x839a_e83c;
    validate_crate_attributes(node, &["Range"], data)?;
    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")));
    if children.next().is_some() {
        return Err(invalid(data.path, "unexpected HealCrateCollide child"));
    }
    let start = data.allocate(pointer, 0x74)?;
    let crate_table = encode_base(node, HEAL, start, data, references, reference_types)?;
    let range = finite_number(node.attribute("Range").unwrap_or("75"), "Range", data.path)?;
    data.word(start + 0x6c, range.to_bits());
    let filter = match filter {
        Some(child) => Target::Table(encode_filter(child, start + 0x70, data, reference_types)?),
        None => Target::Null,
    };
    Ok((
        HEAL,
        Table::record(vec![
            base(KIND, crate_table),
            Entry::named("Range", 0x6458_64f3, Target::Data(start + 0x6c)),
            Entry::named("ObjectFilter", 0xe46e_a3b2, filter),
        ]),
    ))
}

pub(super) fn encode_veterancy(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const VETERANCY: u32 = 0x573d_c203;
    const EXTRA_FIELDS: [(&str, u32, usize); 4] = [
        ("RangeOfEffect", 0x7631_77a4, 0x6c),
        ("AddsOwnerVeterancy", 0x77e3_f4f7, 0x74),
        ("IsPilot", 0x77e3_f4f7, 0x75),
        ("AffectsUpToLevel", 0x23e6_50b8, 0x70),
    ];
    let path = data.path;
    validate_crate_attributes(node, &EXTRA_FIELDS.map(|(name, ..)| name), data)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected VeterancyCrateCollide content"));
    }
    let start = data.allocate(pointer, 0x78)?;
    let crate_table = encode_base(node, VETERANCY, start, data, references, reference_types)?;
    let range = node
        .attribute("RangeOfEffect")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid RangeOfEffect"))?;
    let level = node
        .attribute("AffectsUpToLevel")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid AffectsUpToLevel"))?;
    data.word(start + 0x6c, range);
    data.word(start + 0x70, level as u32);
    data.payload[start + 0x74] = boolean(node, "AddsOwnerVeterancy", false, path)?;
    data.payload[start + 0x75] = boolean(node, "IsPilot", false, path)?;
    let mut entries = vec![base(KIND, crate_table)];
    entries.extend(fields(start, &EXTRA_FIELDS));
    Ok((VETERANCY, Table::record(entries)))
}
