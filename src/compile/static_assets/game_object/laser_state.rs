use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::secondary_damage::encode_status;
use super::spawn::id_list;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xe7b6_af0d;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id", "LaserId", "OriginBoneName"], path)?;
    let start = data.allocate(pointer, 0x2c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let id = node
        .attribute("LaserId")
        .unwrap_or("0")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid LaserId"))?;
    data.word(start + 8, id as u32);
    if let Some(bone) = node.attribute("OriginBoneName") {
        data.string(start + 0xc, bone)?;
    }
    entries.extend(fields(
        start,
        &[
            ("LaserId", 0x23e6_50b8, 8),
            ("OriginBoneName", 0x1a29_33aa, 0xc),
        ],
    ));
    let mut children = significant_children(node).peekable();
    for (name, offset) in [
        ("LaserEndParticleSystem", 0x14),
        ("LaserStartParticleSystem", 0x1c),
    ] {
        let mut ids = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            let value = text_value(child, path)?;
            let plain = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(plain, "FXParticleSystemTemplate", path)?;
            references.push(id.into());
            reference_types.push(id.type_id);
            ids.push(references.len() as u32);
        }
        let table = id_list(data, start + offset, ids)?;
        entries.push(Entry::named(name, 0x43f1_ce36, Target::Table(table)));
    }
    let end =
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "EndOffset"))) {
            let axes = ["x", "y", "z"];
            validate_attributes(child, &axes, path)?;
            if has_content(child) {
                return Err(invalid(path, "unexpected EndOffset content"));
            }
            let offset = data.allocate(start + 0x24, 12)?;
            for (index, name) in axes.into_iter().enumerate() {
                let value = finite_number(child.attribute(name).unwrap_or("0"), name, path)?;
                data.word(offset + index * 4, value.to_bits());
            }
            Target::Data(offset)
        } else {
            Target::Null
        };
    let status =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectStatusValidation"))) {
            Some(child) => Target::Table(encode_status(child, start + 0x28, data)?),
            None => Target::Null,
        };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LaserState child"));
    }
    entries.extend([
        Entry::named("EndOffset", 0x2936_8077, end),
        Entry::named("ObjectStatusValidation", 0x3e8f_938c, status),
    ]);
    Ok((KIND, Table::record(entries)))
}
