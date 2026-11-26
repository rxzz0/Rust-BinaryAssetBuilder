use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_time, sage_uint, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_filter;
use super::passive_area::reference;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const FIELDS: [(&str, u32, usize); 7] = [
    ("MaxCrates", 0x7631_77a4, 8),
    ("CreateFrequency", 0x9986_69e8, 0xc),
    ("CrateLifetime", 0x9986_69e8, 0x10),
    ("MapExclusionMargin", 0x6458_64f3, 0x14),
    ("BlockingUnitRange", 0x6458_64f3, 0x18),
    ("BlockingStartPositionRange", 0x6458_64f3, 0x1c),
    ("Flags", 0xb6d1_fd71, 0x20),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x4a32_bf8e;
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x30)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let max = sage_uint(
        node.attribute("MaxCrates").unwrap_or("20"),
        "MaxCrates",
        path,
    )?;
    data.word(start + 8, max);
    for (name, offset, default) in [
        ("CreateFrequency", 0xc, "30s"),
        ("CrateLifetime", 0x10, "120s"),
    ] {
        let value = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    for (name, offset, default) in [
        ("MapExclusionMargin", 0x14, "50"),
        ("BlockingUnitRange", 0x18, "30"),
        ("BlockingStartPositionRange", 0x1c, "300"),
    ] {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let mut flags = 0;
    for flag in node
        .attribute("Flags")
        .unwrap_or("DELETE_EXPIRED_CRATES")
        .split_ascii_whitespace()
    {
        flags |= match flag {
            "NONE" => 1,
            "DELETE_EXPIRED_CRATES" => 2,
            _ => {
                return Err(invalid(
                    path,
                    format!("invalid SpawnCrateUpdate flag {flag}"),
                ));
            }
        };
    }
    data.word(start + 0x20, flags);
    entries.extend(fields(start, &FIELDS));

    let mut children = significant_children(node).peekable();
    let mut crates = Vec::new();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "CrateList"))) {
        validate_attributes(child, &[], path)?;
        let value = text_value(child, path)?;
        crates.push(reference(
            value,
            "GameObject",
            path,
            references,
            reference_types,
        )?);
    }
    let list = start + 0x24;
    let first = data.list(list, crates.len(), 4)?;
    let items = crates.into_iter().enumerate().map(|(index, id)| {
        let offset = first + index * 4;
        data.word(offset, id);
        (0, Target::Data(offset))
    });
    entries.push(Entry::named(
        "CrateList",
        0xbfab_9075,
        Target::Table(Table::array(list, items)),
    ));
    let blocking = match children.next() {
        Some(child) if child.has_tag_name((SAGE_NS, "BlockingUnits")) => {
            Target::Table(encode_filter(child, start + 0x2c, data, reference_types)?)
        }
        None => Target::Null,
        Some(_) => return Err(invalid(path, "unexpected SpawnCrateUpdate child")),
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SpawnCrateUpdate child"));
    }
    entries.push(Entry::named("BlockingUnits", 0xe46e_a3b2, blocking));
    Ok((KIND, Table::record(entries)))
}
