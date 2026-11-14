use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, required_element, significant_children, text_value, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::passive_area::reference;
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
    const KIND: u32 = 0x9d88_bc91;
    let path = data.path;
    validate_attributes(node, &["id", "Flags"], path)?;
    let mut flags = 0;
    for flag in node
        .attribute("Flags")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        flags |= match flag {
            "NOTIFY_WEAPON_SET_UPDATE" => 1,
            "NOTIFY_SUBOBJECT_UPGRADE_MUX" => 2,
            _ => {
                return Err(invalid(
                    path,
                    format!("unknown SecondaryObjectListenerModule flag {flag}"),
                ));
            }
        };
    }
    let start = data.allocate(pointer, 0x18)?;
    let parent = module_base(node, KIND, start, data, false)?;
    data.word(start + 8, flags);
    let mut children = significant_children(node).peekable();
    let validation = match children.next_if(|child| child.has_tag_name((SAGE_NS, "SelfValidation")))
    {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0xc,
            data,
        )?),
        None => Target::Null,
    };
    let tuples: Vec<_> = children.collect();
    if tuples.is_empty() {
        return Err(invalid(
            path,
            "SecondaryObjectListenerModule requires ObjectEffectTuple",
        ));
    }
    let count = start + 0x10;
    let first = data.list(count, tuples.len(), 0x78)?;
    let mut items = Vec::with_capacity(tuples.len());
    for (index, tuple) in tuples.into_iter().enumerate() {
        if !tuple.has_tag_name((SAGE_NS, "ObjectEffectTuple")) {
            return Err(invalid(
                path,
                "unexpected SecondaryObjectListenerModule child",
            ));
        }
        validate_attributes(tuple, &[], path)?;
        let tuple_start = first + index * 0x78;
        let mut children = significant_children(tuple);
        let filter = required_element(&mut children, "ObjectFilter", path)?;
        let filter =
            super::parking::encode_inline_filter(filter, tuple_start, data, reference_types)?;
        let mut upgrades = Vec::new();
        for child in children {
            if !child.has_tag_name((SAGE_NS, "UpgradeSelf")) {
                return Err(invalid(path, "unexpected ObjectEffectTuple child"));
            }
            upgrades.push(reference(
                text_value(child, path)?,
                "UpgradeTemplate",
                path,
                references,
                reference_types,
            )?);
        }
        let upgrades = super::spawn::id_list(data, tuple_start + 0x70, upgrades)?;
        items.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("ObjectFilter", 0xe46e_a3b2, Target::Table(filter)),
                Entry::named("UpgradeSelf", 0x4aba_1f29, Target::Table(upgrades)),
            ])),
        ));
    }
    Ok((
        KIND,
        Table::record(vec![
            parent,
            Entry::named("Flags", 0x0cb0_a0b3, Target::Data(start + 8)),
            Entry::named("SelfValidation", 0x3e8f_938c, validation),
            Entry::named(
                "ObjectEffectTuple",
                0x7fa5_326f,
                Target::Table(Table::array(count, items)),
            ),
        ]),
    ))
}
