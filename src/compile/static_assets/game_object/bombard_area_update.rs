use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, required_element, sage_angle, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use super::passive_area::reference;
use super::secondary_damage::encode_status;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x4950_fd10;
const FIELDS: [(&str, u32, usize); 4] = [
    ("UsePlayerStore", 0x77e3_f4f7, 0x28),
    ("IncomingAngle", 0x4e16_ea4d, 8),
    ("WeaponName", 0xd046_dd22, 0xc),
    ("MaxShots", 0x23e6_50b8, 0x10),
];

pub(super) fn encode(
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
            "UsePlayerStore",
            "IncomingAngle",
            "WeaponName",
            "MaxShots",
        ],
        path,
    )?;
    let mut children = significant_children(node).peekable();
    let status_node =
        children.next_if(|node| node.has_tag_name((SAGE_NS, "ObjectStatusValidation")));
    let delay = required_element(&mut children, "TimeDelayRange", path)?;
    let spread = required_element(&mut children, "RadiusSpreadRange", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected BombardAreaUpdate child"));
    }

    let start = data.allocate(pointer, 0x2c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.payload[start + 0x28] = boolean(node, "UsePlayerStore", false, path)?;
    let angle = sage_angle(
        node.attribute("IncomingAngle").unwrap_or("0d"),
        "IncomingAngle",
        path,
    )?;
    data.word(start + 8, angle.to_bits());
    let weapon = reference(
        node.attribute("WeaponName").unwrap_or_default(),
        "WeaponTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 0xc, weapon);
    let shots = node
        .attribute("MaxShots")
        .unwrap_or("1")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid MaxShots"))?;
    data.word(start + 0x10, shots as u32);
    entries.extend(fields(start, &FIELDS));

    let status = match status_node {
        Some(child) => Target::Table(encode_status(child, start + 0x14, data)?),
        None => Target::Null,
    };
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    for (name, kind, field_kind, offset, child) in [
        (
            "TimeDelayRange",
            0x4b7b_364f,
            0x9986_69e8,
            start + 0x18,
            delay,
        ),
        (
            "RadiusSpreadRange",
            0x5e48_7bb1,
            0x6458_64f3,
            start + 0x20,
            spread,
        ),
    ] {
        validate_attributes(child, &["Low", "High"], path)?;
        if has_content(child) {
            return Err(invalid(path, format!("unexpected {name} content")));
        }
        for (bound, index) in [("Low", 0), ("High", 4)] {
            let value = required(child, bound, path)?;
            let value = if name == "TimeDelayRange" {
                sage_time(value, bound, path)?
            } else {
                finite_number(value, bound, path)?
            };
            data.word(offset + index, value.to_bits());
        }
        let range = fields(offset, &[("Low", field_kind, 0), ("High", field_kind, 4)]).collect();
        entries.push(Entry::named(
            name,
            kind,
            Target::Table(Table::record(range)),
        ));
    }
    Ok((KIND, Table::record(entries)))
}
