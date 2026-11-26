use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, significant_children,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::parking::encode_inline_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xaab6_4617;
    const FIELDS: [(&str, u32, usize); 4] = [
        ("EnclosureHitFX", 0x0d1c_8e83, 0x8),
        ("ScalableDrawModuleId", 0xbdf0_7965, 0xc),
        ("ScaledBoneName", 0x1a29_33aa, 0x10),
        ("EnclosureRadius", 0x6458_64f3, 0x18),
    ];
    let path = data.path;
    let allowed = ["id", FIELDS[0].0, FIELDS[1].0, FIELDS[2].0, FIELDS[3].0];
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "Filter")))
        .ok_or_else(|| invalid(path, "EncloseBehavior requires Filter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected EncloseBehavior child"));
    }

    let start = data.allocate(pointer, 0x8c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    if let Some(value) = node
        .attribute("EnclosureHitFX")
        .filter(|value| !value.trim().is_empty())
    {
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let effect = asset_reference(value, "FXList", path)?;
        reference_types.push(effect.type_id);
        references.push(effect.into());
        data.word(start + 8, references.len() as u32);
    }
    if let Some(value) = node.attribute("ScalableDrawModuleId") {
        data.word(start + 0xc, hash_name(value));
    }
    if let Some(value) = node.attribute("ScaledBoneName") {
        data.string(start + 0x10, value)?;
    }
    let radius = node.attribute("EnclosureRadius").unwrap_or("300");
    data.word(
        start + 0x18,
        finite_number(radius, "EnclosureRadius", path)?.to_bits(),
    );
    entries.extend(fields(start, &FIELDS));
    let filter = encode_inline_filter(filter, start + 0x1c, data, reference_types)?;
    entries.push(Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)));
    Ok((KIND, Table::record(entries)))
}
