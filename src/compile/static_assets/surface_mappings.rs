use super::ai_strategic::enum_index;
use super::binary::BinaryData;
use super::fx_list::validate_leaf;
use super::game_object::write_model_conditions;
use super::locomotor::terrain_class;
use super::{
    Marshaled, finite_number, percentage, significant_children, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn terrain_particles(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ACCELERATION: &str = "AccelerationMultiplier";
    const REQUIRED_FLAGS: &str = "RequiredModelConditions";
    const FORBIDDEN_FLAGS: &str = "ForbiddenModelConditions";
    validate_attributes(node, &["id", ACCELERATION], path)?;
    let mut data = BinaryData {
        payload: vec![0; 0x10],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let acceleration = percentage(
        node.attribute(ACCELERATION).unwrap_or("100%"),
        ACCELERATION,
        path,
    )?;
    data.word(4, acceleration.to_bits());
    let first = data.list(8, significant_children(node).count(), 0x80)?;
    for (index, item) in significant_children(node).enumerate() {
        if !item.has_tag_name((SAGE_NS, "Mapping")) {
            return Err(invalid(path, "expected Mapping in TerrainParticleMapping"));
        }
        validate_leaf(
            item,
            &[
                "TerrainClass",
                "ParticleSystem",
                REQUIRED_FLAGS,
                FORBIDDEN_FLAGS,
            ],
            path,
        )?;
        let offset = first + index * 0x80;
        let terrain = terrain_class(
            item.attribute("TerrainClass").unwrap_or("UNSPECIFIED"),
            path,
        )?;
        data.word(offset, terrain);
        if let Some(name) = item
            .attribute("ParticleSystem")
            .filter(|name| !name.trim().is_empty())
        {
            write_import(
                name,
                "FXParticleSystemTemplate",
                offset + 4,
                &mut data,
                &mut result,
            )?;
        }
        for (name, offset) in [
            (REQUIRED_FLAGS, offset + 8),
            (FORBIDDEN_FLAGS, offset + 0x44),
        ] {
            if let Some(flags) = item.attribute(name) {
                write_model_conditions(flags, offset, &mut data)?;
            }
        }
    }
    Ok(finish(data, result))
}

pub(super) fn dynamics_contacts(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    // DynamicsContactTag values in their SDK declaration order.
    const TAGS: [&str; 8] = [
        "NONE", "GROUND", "WATER", "VEHICLE", "PROP", "WALL", "TREE", "DEBRIS",
    ];
    validate_attributes(node, &["id"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 0xc],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    let first = data.list(4, significant_children(node).count(), 0x10)?;
    for (index, item) in significant_children(node).enumerate() {
        if !item.has_tag_name((SAGE_NS, "Mapping")) {
            return Err(invalid(
                path,
                "expected Mapping in DynamicsContactTagMapping",
            ));
        }
        validate_leaf(item, &["ContactA", "ContactB", "MinForce", "FX"], path)?;
        let offset = first + index * 0x10;
        for (name, offset) in [("ContactA", offset), ("ContactB", offset + 4)] {
            let value = enum_index(item.attribute(name).unwrap_or("NONE"), &TAGS, name, path)?;
            data.word(offset, value);
        }
        let force = finite_number(
            item.attribute("MinForce").unwrap_or("1.2"),
            "MinForce",
            path,
        )?;
        data.word(offset + 8, force.to_bits());
        if let Some(name) = item.attribute("FX").filter(|name| !name.trim().is_empty()) {
            write_import(name, "FXList", offset + 0xc, &mut data, &mut result)?;
        }
    }
    Ok(finish(data, result))
}

fn finish(mut data: BinaryData<'_>, mut result: Marshaled) -> Marshaled {
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    result
}
