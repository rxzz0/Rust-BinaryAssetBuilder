use std::io;

use roxmltree::Node;

use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, invalid, required_element, significant_children, validate_attributes,
};
use super::behaviors_basic::upgrade_parent;
use super::draws::write_model_conditions;
use super::spawn::{UPGRADE, weak_reference};
use crate::compile::SAGE_NS;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x3176_b098;
    let path = data.path;
    let attributes: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &attributes, path)?;
    let start = data.allocate(pointer, 0x34)?;
    let mut children = significant_children(node).peekable();

    // EA writes the derived lists ahead of the upgrade base's variable-sized data.
    let mut available = Vec::new();
    let mut offsets = Vec::new();
    let mut seen_available = false;
    let mut seen_offset = false;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "AvailableSubStructure")) && !seen_offset {
            seen_available = true;
            available.push(child);
        } else if child.has_tag_name((SAGE_NS, "StructureOffset")) {
            seen_offset = true;
            offsets.push(child);
        } else if seen_available || seen_offset {
            return Err(invalid(path, "unexpected SubStructureBuild child"));
        }
    }
    // Reference types are hashed in XML order, independently of payload allocation.
    let mut available_types = Vec::new();
    let available = available_structures(available, start + 0x24, data, &mut available_types)?;
    let offsets = structure_offsets(offsets, start + 0x2c, data)?;
    let parent = upgrade_parent(node, KIND, start, data, reference_types, &mut children)?;
    reference_types.extend(available_types);
    if children.any(|child| {
        !child.has_tag_name((SAGE_NS, "AvailableSubStructure"))
            && !child.has_tag_name((SAGE_NS, "StructureOffset"))
    }) {
        return Err(invalid(path, "unexpected SubStructureBuild child"));
    }

    Ok((
        KIND,
        Table::record(vec![
            parent,
            Entry::named(
                "AvailableSubStructure",
                0xb387_3ac3,
                Target::Table(available),
            ),
            Entry::named("StructureOffset", 0x645f_aa23, Target::Table(offsets)),
        ]),
    ))
}

fn available_structures(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let first = data.list(count, nodes.len(), 8)?;
    let mut entries = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &[], path)?;
        let mut elements = significant_children(node);
        let upgrade = required_element(&mut elements, "UpgradeTrigger", path)?;
        let object = required_element(&mut elements, "ObjectTemplate", path)?;
        if elements.next().is_some() {
            return Err(invalid(path, "unexpected AvailableSubStructure child"));
        }
        let offset = first + index * 8;
        let upgrade_id = weak_reference(upgrade, "UpgradeTemplate", data, reference_types)?;
        let object_id = weak_reference(object, "GameObject", data, reference_types)?;
        data.word(offset, upgrade_id);
        data.word(offset + 4, object_id);
        entries.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("UpgradeTrigger", 0x26fa_cfc7, Target::Data(offset)),
                Entry::named("ObjectTemplate", 0x148e_d75d, Target::Data(offset + 4)),
            ])),
        ));
    }
    Ok(Table::array(count, entries))
}

fn structure_offsets(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    let first = data.list(count, nodes.len(), 0x48)?;
    let mut entries = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &["ModelCondition"], path)?;
        let start = first + index * 0x48;
        write_model_conditions(node.attribute("ModelCondition").unwrap_or(""), start, data)?;
        let mut elements = significant_children(node);
        let offset = required_element(&mut elements, "Offset", path)?;
        let axes = ["x", "y", "z"];
        validate_attributes(offset, &axes, path)?;
        if significant_children(offset).next().is_some() || elements.next().is_some() {
            return Err(invalid(path, "unexpected StructureOffset child"));
        }
        for (index, axis) in axes.into_iter().enumerate() {
            let value = finite_number(offset.attribute(axis).unwrap_or("0"), axis, path)?;
            data.word(start + 0x3c + index * 4, value.to_bits());
        }
        entries.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("ModelCondition", 0x67db_bfc9, Target::Data(start)),
                Entry::named("Offset", 0x2936_8077, Target::Data(start + 0x3c)),
            ])),
        ));
    }
    Ok(Table::array(count, entries))
}
