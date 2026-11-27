use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, FLOAT_TYPE, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use super::passive_area::reference;
use super::secondary_damage::write_status_flags;
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
    const RESTRICT: u32 = 0x953c_7f5b;
    const AREA: u32 = 0x47e7_eac9;
    const ASSIST: u32 = 0xc82c_b0cc;
    const RESTRICTION: (&str, u32, usize) = ("RestrictionType", 0x2b7c_84d8, 8);
    let name = node.tag_name().name();
    let (kind, size, attributes): (_, _, &[_]) = match name {
        "RestrictSpecialPowerBehavior" => (
            RESTRICT,
            0x1c,
            &[
                RESTRICTION,
                ("DependentObject", 0x148e_d75d, 12),
                ("ConsiderSpecialPowerRadius", 0x77e3_f4f7, 24),
            ],
        ),
        "AreaRestrictSpecialPowerBehavior" => {
            (AREA, 0x14, &[RESTRICTION, ("AreaName", 0x1a29_33aa, 12)])
        }
        "SpecialPowerAssistResponseBehavior" => (
            ASSIST,
            0x28,
            &[
                ("SpecialPowerTemplate", 0xaee8_3888, 8),
                ("ObjectStatus", 0xa7bd_811e, 12),
            ],
        ),
        _ => return Err(invalid(data.path, format!("unexpected behavior {name}"))),
    };
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(attributes.iter().map(|&(field, ..)| field))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if kind != RESTRICT && has_content(node) {
        return Err(invalid(path, format!("unexpected {name} content")));
    }
    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, false)?];
    entries.extend(fields(start, attributes));
    if kind != ASSIST {
        let restriction = enum_value(
            node.attribute("RestrictionType").unwrap_or("UNRESTRICTED"),
            &["UNRESTRICTED", "1", "2"],
            path,
        )?;
        data.word(start + 8, restriction);
    }
    match kind {
        RESTRICT => {
            if let Some(value) = node
                .attribute("DependentObject")
                .filter(|value| !value.trim().is_empty())
            {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "GameObject", path)?;
                reference_types.push(id.type_id);
                data.word(start + 12, id.instance_id);
            }
            data.payload[start + 24] = boolean(node, "ConsiderSpecialPowerRadius", true, path)?;
            let radii = radius_list(node, start + 16, data)?;
            entries.push(Entry::named(
                "DependentObjectRadius",
                FLOAT_TYPE,
                Target::Table(radii),
            ));
        }
        AREA => {
            if let Some(area) = node.attribute("AreaName") {
                data.string(start + 12, area)?;
            }
        }
        ASSIST => {
            let power = reference(
                node.attribute("SpecialPowerTemplate").unwrap_or(""),
                "SpecialPowerTemplate",
                path,
                references,
                reference_types,
            )?;
            data.word(start + 8, power);
            write_status_flags(
                node.attribute("ObjectStatus").unwrap_or(""),
                start + 12,
                data,
            )?;
        }
        _ => unreachable!("power response behavior"),
    }
    Ok((kind, Table::record(entries)))
}

fn radius_list(node: Node<'_, '_>, count: usize, data: &mut BinaryData<'_>) -> io::Result<Table> {
    const NAME: &str = "DependentObjectRadius";
    let path = data.path;
    let mut radii = Vec::new();
    for child in significant_children(node) {
        if !child.has_tag_name((SAGE_NS, NAME)) || child.children().any(|item| item.is_element()) {
            return Err(invalid(path, format!("expected {NAME}")));
        }
        validate_attributes(child, &[], path)?;
        let value: String = child
            .children()
            .filter(Node::is_text)
            .filter_map(|item| item.text())
            .collect();
        radii.push(finite_number(value.trim(), NAME, path)?);
    }
    if radii.is_empty() {
        return Err(invalid(path, format!("expected {NAME}")));
    }
    let start = data.list(count, radii.len(), 4)?;
    let mut items = Vec::with_capacity(radii.len());
    for (index, radius) in radii.into_iter().enumerate() {
        let offset = start + index * 4;
        data.word(offset, radius.to_bits());
        items.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, items))
}
