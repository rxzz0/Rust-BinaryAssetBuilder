use super::super::binary::BinaryData;
use super::super::fx_list::validate_leaf;
use super::super::tokenized::{Entry, FLOAT_TYPE, Table, Target};
use super::super::{
    asset_reference, finite_number, required_element, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::boolean;
use super::parking::encode_inline_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const BOOL: u32 = 0x77e3_f4f7;
const FILTER: u32 = 0xe46e_a3b2;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x8acd_4ad2;
    let path = data.path;
    validate_attributes(node, &["id", "AllowShrouded"], path)?;
    let mut children = significant_children(node).peekable();
    let mut hints = Vec::new();
    while let Some(hint) = children.next_if(|child| child.has_tag_name((SAGE_NS, "HintObject"))) {
        hints.push(hint);
    }
    let restriction =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "PlacementRestriction")));
    let offset = required_element(&mut children, "SuggestedOffset", path)?;
    if children.next().is_some() {
        return Err(invalid(
            path,
            "unexpected AssistedPlacementInfoBehavior child",
        ));
    }
    let axes = ["x", "y", "z"];
    validate_leaf(offset, &axes, path)?;

    let start = data.allocate(pointer, 0x24)?;
    let base = module_base(node, KIND, start, data, false)?;
    data.payload[start + 0x20] = boolean(node, "AllowShrouded", true, path)?;
    for (index, axis) in axes.into_iter().enumerate() {
        let value = finite_number(offset.attribute(axis).unwrap_or("0"), axis, path)?;
        data.word(start + 0x14 + index * 4, value.to_bits());
    }

    let count = start + 8;
    let first = data.list(count, hints.len(), 0x14)?;
    let mut items = Vec::with_capacity(hints.len());
    for (index, hint) in hints.into_iter().enumerate() {
        items.push((
            0,
            encode_hint(hint, first + index * 0x14, data, reference_types)?,
        ));
    }

    let restriction = if let Some(restriction) = restriction {
        validate_attributes(restriction, &["FilterRadius"], path)?;
        let offset = data.allocate(start + 0x10, 8)?;
        let radius = finite_number(
            restriction.attribute("FilterRadius").unwrap_or("0"),
            "FilterRadius",
            path,
        )?;
        data.word(offset, radius.to_bits());
        let filter = encode_filter(
            restriction,
            "RestrictedObjectFilter",
            offset + 4,
            data,
            reference_types,
        )?;
        Target::Table(Table::record(vec![
            Entry::named("FilterRadius", FLOAT_TYPE, Target::Data(offset)),
            Entry::named("RestrictedObjectFilter", FILTER, filter),
        ]))
    } else {
        Target::Null
    };

    Ok((
        KIND,
        Table::record(vec![
            base,
            Entry::named("AllowShrouded", BOOL, Target::Data(start + 0x20)),
            Entry::named(
                "HintObject",
                0xa2d2_dc27,
                Target::Table(Table::array(count, items)),
            ),
            Entry::named("PlacementRestriction", 0x27a2_0991, restriction),
            Entry::named("SuggestedOffset", 0x2936_8077, Target::Data(start + 0x14)),
        ]),
    ))
}

fn encode_hint(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "PlayerTemplate",
            "HintDisplayObject",
            "HintOpacity",
            "HintUsesShaders",
        ],
        path,
    )?;
    if node.attribute("HintDisplayObject").is_none() {
        return Err(invalid(path, "HintObject requires HintDisplayObject"));
    }

    let mut player = Target::Null;
    // EA hashes reference types in XML attribute order.
    for attr in node.attributes() {
        let kind = match attr.name() {
            "PlayerTemplate" => "PlayerTemplate",
            "HintDisplayObject" => "GameObject",
            _ => continue,
        };
        let value = if attr.value().is_empty() {
            0
        } else {
            let reference = asset_reference(attr.value(), kind, path)?;
            reference_types.push(reference.type_id);
            reference.instance_id
        };
        if kind == "PlayerTemplate" {
            let pointer = data.allocate(offset, 4)?;
            data.word(pointer, value);
            player = Target::Data(pointer);
        } else {
            data.word(offset + 4, value);
        }
    }

    let opacity = finite_number(
        node.attribute("HintOpacity").unwrap_or("1"),
        "HintOpacity",
        path,
    )?;
    data.word(offset + 8, opacity.to_bits());
    data.payload[offset + 0x10] = boolean(node, "HintUsesShaders", false, path)?;
    let filter = encode_filter(node, "ObjectFilter", offset + 0xc, data, reference_types)?;

    Ok(Target::Table(Table::record(vec![
        Entry::named("PlayerTemplate", 0x6fac_143b, player),
        Entry::named("HintDisplayObject", 0x148e_d75d, Target::Data(offset + 4)),
        Entry::named("HintOpacity", FLOAT_TYPE, Target::Data(offset + 8)),
        Entry::named("HintUsesShaders", BOOL, Target::Data(offset + 0x10)),
        Entry::named("ObjectFilter", FILTER, filter),
    ])))
}

fn encode_filter(
    node: Node<'_, '_>,
    name: &str,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    let mut children = significant_children(node);
    let Some(filter) = children.next() else {
        return Ok(Target::Null);
    };
    if !filter.has_tag_name((SAGE_NS, name)) || children.next().is_some() {
        return Err(invalid(
            data.path,
            format!("unexpected {} child", node.tag_name().name()),
        ));
    }
    let offset = data.allocate(pointer, 0x70)?;
    Ok(Target::Table(encode_inline_filter(
        filter,
        offset,
        data,
        reference_types,
    )?))
}
