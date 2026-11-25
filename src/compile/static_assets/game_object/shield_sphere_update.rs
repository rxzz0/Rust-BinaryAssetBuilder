use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, required_element, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{module_base, optional_vector};
use super::draws::{boolean, fields, write_model_conditions};
use super::parking::{encode_filter, encode_inline_filter};
use super::secondary_damage::write_status_flags;
use super::spawn::damage_flags;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x6c11_b6f7;
const FILTER: u32 = 0xe46e_a3b2;
const FIELDS: [(&str, u32, usize); 12] = [
    ("RadiusMax", 0x6458_64f3, 8),
    ("RadiusMin", 0x6458_64f3, 0xc),
    ("DamageTypesNotToAbsorb", 0x07fa_aea4, 0x10),
    ("ScanFrequency", 0x9986_69e8, 0x18),
    ("Duration", 0x9986_69e8, 0x1c),
    ("MaxDamage", 0x6458_64f3, 0x20),
    ("ObjectStatus", 0xa7bd_811e, 0x24),
    ("ModelCondition", 0x67db_bfc9, 0x40),
    ("InitiallyActive", 0x77e3_f4f7, 0x104),
    ("ShieldBoneName", 0x1a29_33aa, 0x7c),
    ("ShieldSizeMultiplier", 0x6458_64f3, 0x84),
    ("Options", 0xeabd_4c0e, 0x88),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let position = children.next_if(|child| child.has_tag_name((SAGE_NS, "PositionOffset")));
    let filter = required_element(&mut children, "ObjectFilter", path)?;
    let ignore =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "IgnoreInsideToInsideCheck")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ShieldSphereUpdate child"));
    }

    let start = data.allocate(pointer, 0x108)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, offset, default) in [
        ("RadiusMax", 8, "40"),
        ("RadiusMin", 0xc, "10"),
        ("MaxDamage", 0x20, "0"),
        ("ShieldSizeMultiplier", 0x84, "1"),
    ] {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    for (name, offset, default) in [("ScanFrequency", 0x18, "1s"), ("Duration", 0x1c, "10s")] {
        let value = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let damage = damage_flags(
        node.attribute("DamageTypesNotToAbsorb").unwrap_or_default(),
        data,
    )?;
    data.word(start + 0x10, damage as u32);
    data.word(start + 0x14, (damage >> 32) as u32);
    write_status_flags(
        node.attribute("ObjectStatus").unwrap_or_default(),
        start + 0x24,
        data,
    )?;
    write_model_conditions(
        node.attribute("ModelCondition").unwrap_or_default(),
        start + 0x40,
        data,
    )?;
    data.payload[start + 0x104] = boolean(node, "InitiallyActive", false, path)?;
    if let Some(bone) = node.attribute("ShieldBoneName") {
        data.string(start + 0x7c, bone)?;
    }
    let mut options = 0;
    for option in node
        .attribute("Options")
        .unwrap_or_default()
        .split_ascii_whitespace()
    {
        if option != "ALLOW_ALLIES_PROJECTTILE_GOTHROUGH" {
            return Err(invalid(
                path,
                format!("invalid ShieldSphereUpdate option {option}"),
            ));
        }
        options = 1;
    }
    data.word(start + 0x88, options);
    entries.extend(fields(start, &FIELDS));
    entries.push(Entry::named(
        "PositionOffset",
        0x2936_8077,
        optional_vector(position, start + 0x8c, data)?,
    ));
    let filter = encode_inline_filter(filter, start + 0x90, data, reference_types)?;
    entries.push(Entry::named("ObjectFilter", FILTER, Target::Table(filter)));
    let ignore = match ignore {
        Some(child) => Target::Table(encode_filter(child, start + 0x100, data, reference_types)?),
        None => Target::Null,
    };
    entries.push(Entry::named("IgnoreInsideToInsideCheck", FILTER, ignore));
    Ok((KIND, Table::record(entries)))
}
