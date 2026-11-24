use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, significant_children, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xd692_dc8d;
const FIELDS: &[(&str, u32, usize)] = &[
    ("Radius", 0x6458_64f3, 8),
    ("MaxAttacksPerTarget", 0x23e6_50b8, 0xc),
    ("OverflowOptions", 0x2a89_f37f, 0x10),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &["id", "Radius", "MaxAttacksPerTarget", "OverflowOptions"],
        path,
    )?;

    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected StrafeAreaTargetChooser child"));
    }

    let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
    let max_attacks = node
        .attribute("MaxAttacksPerTarget")
        .unwrap_or("-1")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid MaxAttacksPerTarget"))?;
    let mut flags = 0;
    for flag in node
        .attribute("OverflowOptions")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        flags |= match flag {
            "CHOOSE_RANDOM_POSITION" => 1,
            "BREAK_OFF_ATTACK" => 2,
            _ => {
                return Err(invalid(
                    path,
                    format!("unknown OverflowOptions flag {flag}"),
                ));
            }
        };
    }

    let start = data.allocate(pointer, 0x18)?;
    let mut entries = vec![module_base(node, KIND, start, data, false)?];
    data.word(start + 8, radius.to_bits());
    data.word(start + 0xc, max_attacks as u32);
    data.word(start + 0x10, flags);
    entries.extend(fields(start, FIELDS));
    let filter = match filter {
        Some(filter) => Target::Table(encode_filter(filter, start + 0x14, data, reference_types)?),
        None => Target::Null,
    };
    entries.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
    Ok((KIND, Table::record(entries)))
}
