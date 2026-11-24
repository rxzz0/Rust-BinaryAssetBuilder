use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_inline_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x829f_7876;
const FIELDS: [(&str, u32, usize); 6] = [
    ("Flags", 0x25f8_61e4, 8),
    ("MaxRadius", 0x6458_64f3, 0xc),
    ("ArcDistance", 0x6458_64f3, 0x10),
    ("SweepFXList", 0x0d1c_8e83, 0x14),
    ("SweepWeapon", 0xd046_dd22, 0x18),
    ("FXTimeout", 0x9986_69e8, 0x1c),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;

    let mut children = significant_children(node);
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")))
        .ok_or_else(|| invalid(path, "CircularDamageUpdate requires an ObjectFilter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected CircularDamageUpdate child"));
    }

    let start = data.allocate(pointer, 0x90)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let mut flags = 0;
    for flag in node
        .attribute("Flags")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        if flag != "ONLY_ON_WATER" {
            return Err(invalid(
                path,
                format!("invalid CircularDamageUpdate flag {flag}"),
            ));
        }
        flags |= 1;
    }
    data.word(start + 8, flags);
    for (name, offset, default) in [("MaxRadius", 0xc, "100"), ("ArcDistance", 0x10, "10")] {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let timeout = sage_time(
        node.attribute("FXTimeout").unwrap_or("0.25s"),
        "FXTimeout",
        path,
    )?;
    data.word(start + 0x1c, timeout.to_bits());

    for attribute in node.attributes() {
        let (kind, offset) = match attribute.name() {
            "SweepFXList" => ("FXList", 0x14),
            "SweepWeapon" => ("WeaponTemplate", 0x18),
            _ => continue,
        };
        let reference = super::passive_area::reference(
            attribute.value(),
            kind,
            path,
            references,
            reference_types,
        )?;
        data.word(start + offset, reference);
    }

    entries.extend(fields(start, &FIELDS));
    entries.push(Entry::named(
        "ObjectFilter",
        0xe46e_a3b2,
        Target::Table(encode_inline_filter(
            filter,
            start + 0x20,
            data,
            reference_types,
        )?),
    ));
    Ok((KIND, Table::record(entries)))
}
