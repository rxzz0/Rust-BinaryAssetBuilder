use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, hash_name, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use super::parking::encode_filter;
use super::secondary_damage::encode_status;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode_self(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x3563_4018;
    const FIELDS: &[(&str, u32, usize)] = &[
        ("InvisibilityTemplate", 0x914c_a765, 8),
        ("UpdatePeriod", 0x9986_69e8, 0xc),
        ("RequiredNearbyObjectRange", 0x6458_64f3, 0x10),
    ];
    const VOICES: &[(&str, usize)] = &[
        ("NamedVoiceNameToUseAsVoiceMoveToStealthyArea", 0x14),
        (
            "NamedVoiceNameToUseAsVoiceEnterStateMoveToStealthyArea",
            0x18,
        ),
    ];
    const FILTER: &str = "RequiresNearbyObjectFilter";
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.iter().map(|&(name, ..)| name))
        .chain(VOICES.iter().map(|&(name, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, FILTER)))
        .ok_or_else(|| {
            invalid(
                path,
                "InvisibilityUpdate requires RequiresNearbyObjectFilter",
            )
        })?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected InvisibilityUpdate child"));
    }
    let start = data.allocate(pointer, 0x8c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let template = super::passive_area::reference(
        required(node, "InvisibilityTemplate", path)?,
        "InvisibilityTemplate",
        path,
        references,
        reference_types,
    )?;
    let period = sage_time(
        node.attribute("UpdatePeriod").unwrap_or("1s"),
        "UpdatePeriod",
        path,
    )?;
    let range = finite_number(
        node.attribute("RequiredNearbyObjectRange").unwrap_or("0"),
        "RequiredNearbyObjectRange",
        path,
    )?;
    data.word(start + 8, template);
    data.word(start + 0xc, period.to_bits());
    data.word(start + 0x10, range.to_bits());
    entries.extend(fields(start, FIELDS));
    for &(name, relative) in VOICES {
        let target = if let Some(value) = node.attribute(name) {
            let offset = data.allocate(start + relative, 4)?;
            data.word(offset, super::super::fast_hash(value.as_bytes()));
            Target::Data(offset)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, 0x58be_c727, target));
    }
    let filter = super::parking::encode_inline_filter(filter, start + 0x1c, data, reference_types)?;
    entries.push(Entry::named(FILTER, 0xe46e_a3b2, Target::Table(filter)));
    Ok((KIND, Table::record(entries)))
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xb1e5_bfda;
    const FIELDS: [(&str, u32, usize); 6] = [
        ("InvisibilityTemplate", 0x914c_a765, 8),
        ("AffectsSelf", 0x77e3_f4f7, 0x28),
        ("UpdatePeriod", 0x9986_69e8, 0xc),
        ("BroadcastRange", 0x6458_64f3, 0x10),
        ("ScalableDrawModuleId", 0xbdf0_7965, 0x14),
        ("ScaledBoneName", 0x1a29_33aa, 0x18),
    ];
    const FILTER: &str = "BroadcastObjectFilter";
    const STATUS: &str = "ObjectStatusValidation";
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;

    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, FILTER)));
    let status = children.next_if(|child| child.has_tag_name((SAGE_NS, STATUS)));
    if children.next().is_some() {
        return Err(invalid(
            path,
            "unexpected InvisibilityBroadcastUpdate child",
        ));
    }
    let start = data.allocate(pointer, 0x2c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let template = super::passive_area::reference(
        required(node, "InvisibilityTemplate", path)?,
        "InvisibilityTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 8, template);
    data.payload[start + 0x28] = boolean(node, "AffectsSelf", true, path)?;
    let period = sage_time(
        node.attribute("UpdatePeriod").unwrap_or("1s"),
        "UpdatePeriod",
        path,
    )?;
    data.word(start + 0xc, period.to_bits());
    let range = finite_number(
        node.attribute("BroadcastRange").unwrap_or("0"),
        "BroadcastRange",
        path,
    )?;
    data.word(start + 0x10, range.to_bits());
    if let Some(id) = node
        .attribute("ScalableDrawModuleId")
        .filter(|id| !id.is_empty())
    {
        data.word(start + 0x14, hash_name(id));
    }
    if let Some(bone) = node.attribute("ScaledBoneName") {
        data.string(start + 0x18, bone)?;
    }
    entries.extend(fields(start, &FIELDS));
    let filter = filter
        .map(|filter| encode_filter(filter, start + 0x20, data, reference_types).map(Target::Table))
        .transpose()?
        .unwrap_or(Target::Null);
    let status = status
        .map(|status| encode_status(status, start + 0x24, data).map(Target::Table))
        .transpose()?
        .unwrap_or(Target::Null);
    entries.push(Entry::named(FILTER, 0xe46e_a3b2, filter));
    entries.push(Entry::named(STATUS, 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}
