use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_int, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::boolean;
use super::parking::encode_inline_filter;
use super::passive_area::reference;
use super::recall::empty_filter;
use super::spawn::{id_list, weak_reference};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const ATTRIBUTE_MODIFIER_NAVAL: u32 = 0xa34c_ed5c;
    const COMBINED_INFO: u32 = 0x784e_c9b8;
    const EXPERIENCE_LEVEL: u32 = 0xc2eb_b54d;
    const RANDOM_MODEL_CONDITION: u32 = 0xddd8_1ee9;
    const SUPPLY_CENTER: u32 = 0x9776_876e;
    const INHERIT_UPGRADE: u32 = 0xc001_089b;

    let path = data.path;
    let (kind, size, allowed): (u32, usize, &[&str]) = match node.tag_name().name() {
        "AttributeModifierNaval" => (ATTRIBUTE_MODIFIER_NAVAL, 0x0c, &["id", "AttributeModifier"]),
        "CombinedInfoModule" => (COMBINED_INFO, 0x10, &["id", "DockingBone"]),
        "ExperienceLevelCreate" => (EXPERIENCE_LEVEL, 0x10, &["id", "Level", "MPOnly"]),
        "RandomModelCondition" => (RANDOM_MODEL_CONDITION, 0x10, &["id", "Conditions"]),
        "SupplyCenterCreate" => (SUPPLY_CENTER, 0x08, &["id"]),
        "InheritUpgradeCreate" => (INHERIT_UPGRADE, 0x84, &["id", "Radius"]),
        name => return Err(invalid(path, format!("unexpected create module {name}"))),
    };
    validate_attributes(node, allowed, path)?;
    if kind != INHERIT_UPGRADE && has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected {} content", node.tag_name().name()),
        ));
    }

    let start = data.allocate(pointer, size)?;
    let parent = module_base(node, kind, start, data, false)?;
    let mut fields = vec![base(0x30a3_6b8d, Table::record(vec![parent]))];
    match kind {
        ATTRIBUTE_MODIFIER_NAVAL => {
            let modifier = reference(
                node.attribute("AttributeModifier").unwrap_or(""),
                "AttributeModifier",
                path,
                references,
                reference_types,
            )?;
            data.word(start + 8, modifier);
            fields.push(Entry::named(
                "AttributeModifier",
                0x09dd_cff3,
                Target::Data(start + 8),
            ));
        }
        COMBINED_INFO => {
            if let Some(value) = node.attribute("DockingBone") {
                data.string(start + 8, value)?;
            }
            fields.push(Entry::named(
                "DockingBone",
                0x1a29_33aa,
                Target::Data(start + 8),
            ));
        }
        EXPERIENCE_LEVEL => {
            let level = sage_int(node.attribute("Level").unwrap_or("0"), "Level", path)?;
            data.word(start + 8, level);
            data.payload[start + 0xc] = boolean(node, "MPOnly", false, path)?;
            fields.extend([
                Entry::named("Level", 0x23e6_50b8, Target::Data(start + 8)),
                Entry::named("MPOnly", 0x77e3_f4f7, Target::Data(start + 0xc)),
            ]);
        }
        RANDOM_MODEL_CONDITION => {
            let count = start + 8;
            let conditions: Vec<_> = node
                .attribute("Conditions")
                .unwrap_or("")
                .split_ascii_whitespace()
                .collect();
            let first = data.list(count, conditions.len(), 8)?;
            let mut items = Vec::with_capacity(conditions.len());
            for (index, condition) in conditions.into_iter().enumerate() {
                let offset = first + 8 * index;
                data.string(offset, condition)?;
                items.push((0, Target::Data(offset)));
            }
            fields.push(Entry::named(
                "Conditions",
                0x6d66_ae18,
                Target::Table(Table::array(count, items)),
            ));
        }
        INHERIT_UPGRADE => {
            fields.extend(inherit_upgrade_fields(node, start, data, reference_types)?)
        }
        _ => {}
    }
    Ok((kind, Table::record(fields)))
}

fn inherit_upgrade_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<[Entry; 3]> {
    let path = data.path;
    let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
    data.word(start + 8, radius.to_bits());
    let mut children = significant_children(node).peekable();
    let filter = match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter"))) {
        Some(child) => encode_inline_filter(child, start + 0xc, data, reference_types)?,
        None => empty_filter(start + 0xc),
    };
    let mut upgrades = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "Upgrade")) {
            return Err(invalid(path, "unexpected InheritUpgradeCreate child"));
        }
        upgrades.push(weak_reference(
            child,
            "UpgradeTemplate",
            data,
            reference_types,
        )?);
    }
    let upgrades = id_list(data, start + 0x7c, upgrades)?;
    Ok([
        Entry::named("Radius", 0x6458_64f3, Target::Data(start + 8)),
        Entry::named("ObjectFilter", 0xe46e_a3b2, Target::Table(filter)),
        Entry::named("Upgrade", 0x26fa_cfc7, Target::Table(upgrades)),
    ])
}
