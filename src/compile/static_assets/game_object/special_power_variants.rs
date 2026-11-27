use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, sage_uint, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::boolean;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const REAL: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const UNSIGNED: u32 = 0x7631_77a4;
const STRINGS: u32 = 0x6d66_ae18;
const UPDATE: u32 = 0xb9b2_a9b3;

type Field = (&'static str, u32, usize);

pub(super) fn radar_layout(name: &str) -> Option<(u32, usize, &'static [Field])> {
    Some(match name {
        "RadarFreezeSpecialPower" => (
            0x52ee_caed,
            0x1d0,
            &[
                ("TargetFilter", STRINGS, 0x1c4),
                ("FreezeDuration", TIME, 0x1cc),
            ],
        ),
        "RadarJamSpecialPower" => (
            0xe6b3_52b9,
            0x1cc,
            &[("JamRadius", REAL, 0x1c4), ("JamDuration", TIME, 0x1c8)],
        ),
        "RadarSpySpecialPower" => (
            0x47df_9c1a,
            0x1d0,
            &[
                ("TargetFilter", STRINGS, 0x1c4),
                ("SpyDuration", TIME, 0x1cc),
            ],
        ),
        "RadarHackSpecialPower" => (
            0xbed5_aa3a,
            0x1d0,
            &[
                ("NumFalseReturns", UNSIGNED, 0x1c4),
                ("Radius", REAL, 0x1c8),
                ("HackDuration", TIME, 0x1cc),
            ],
        ),
        _ => return None,
    })
}

pub(super) fn radar_fields(
    node: Node<'_, '_>,
    start: usize,
    layout: &[Field],
    data: &mut BinaryData<'_>,
) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::with_capacity(layout.len());
    for &(name, kind, offset) in layout {
        let offset = start + offset;
        let value = node.attribute(name);
        let target = if kind == STRINGS {
            Target::Table(string_list(value.unwrap_or(""), offset, data)?)
        } else {
            let value = match kind {
                REAL => finite_number(value.unwrap_or("0"), name, data.path)?.to_bits(),
                TIME => sage_time(value.unwrap_or("0s"), name, data.path)?.to_bits(),
                UNSIGNED => sage_uint(value.unwrap_or("0"), name, data.path)?,
                _ => unreachable!("unknown radar field kind"),
            };
            data.word(offset, value);
            Target::Data(offset)
        };
        entries.push(Entry::named(name, kind, target));
    }
    Ok(entries)
}

pub(super) fn string_list(
    value: &str,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let names = value.split_ascii_whitespace();
    let first = data.list(count, names.clone().count(), 8)?;
    let mut items = Vec::new();
    for (index, name) in names.enumerate() {
        let offset = first + index * 8;
        data.string(offset, name)?;
        items.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, items))
}

pub(super) fn encode_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "SpecialPowerTemplate",
            "StartsPaused",
            "DisabledTypesToIgnore",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x18)?;
    let module = module_base(node, UPDATE, start, data, true)?;
    let template = super::passive_area::reference(
        node.attribute("SpecialPowerTemplate").unwrap_or(""),
        "SpecialPowerTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 8, template);
    data.payload[start + 0x14] = boolean(node, "StartsPaused", false, path)?;

    let mut disabled = 0;
    for flag in node
        .attribute("DisabledTypesToIgnore")
        .unwrap_or("HELD")
        .split_ascii_whitespace()
    {
        let index = super::production::DISABLED
            .iter()
            .position(|&name| name == flag)
            .ok_or_else(|| invalid(path, format!("invalid DisabledTypesToIgnore flag {flag:?}")))?;
        disabled |= 1 << index;
    }
    data.word(start + 0x0c, disabled);
    let mut children = significant_children(node);
    let sound = if let Some(child) = children.next() {
        if !child.has_tag_name((SAGE_NS, "InitiateSound")) || children.next().is_some() {
            return Err(invalid(
                path,
                "expected a single SpecialPowerUpdate InitiateSound",
            ));
        }
        encode_initiate_sound(child, start + 0x10, data, references, reference_types)?
    } else {
        Target::Null
    };
    Ok((
        UPDATE,
        Table::record(vec![
            module,
            Entry::named("SpecialPowerTemplate", 0xaee8_3888, Target::Data(start + 8)),
            Entry::named("StartsPaused", 0x77e3_f4f7, Target::Data(start + 0x14)),
            Entry::named(
                "DisabledTypesToIgnore",
                0x1965_70c9,
                Target::Data(start + 0x0c),
            ),
            Entry::named("InitiateSound", 0x1ae6_b6b7, sound),
        ]),
    ))
}

pub(super) fn encode_initiate_sound(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let path = data.path;
    validate_attributes(node, &["Sound", "EvaEvent"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected InitiateSound content"));
    }
    let start = data.allocate(pointer, 8)?;
    let sound = if let Some(value) = node.attribute("Sound") {
        let ordinal =
            super::client_behaviors::audio_reference(value, path, references, reference_types)?;
        let offset = data.allocate(start, 4)?;
        data.word(offset, ordinal);
        Target::Data(offset)
    } else {
        Target::Null
    };
    let event = super::passive_area::reference(
        node.attribute("EvaEvent").unwrap_or(""),
        "EvaEvent",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 4, event);
    Ok(Target::Table(Table::record(vec![
        Entry::named("Sound", 0x63d7_b468, sound),
        Entry::named("EvaEvent", 0xc4ec_43b1, Target::Data(start + 4)),
    ])))
}
