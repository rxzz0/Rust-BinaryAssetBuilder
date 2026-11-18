use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, finite_number, hash_name, sage_int, significant_children, validate_attributes,
    write_import,
};
use crate::compile::{BASE_ASSET_TYPE, SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path};

const FIELDS: &[&str] = &[
    "Waypoint",
    "Structure",
    "Unit",
    "Prop",
    "Audio",
    "Unknown",
    "Road",
    "MapMetaData",
    "EnvironmentData",
    "WorldDict",
    "PostEffect",
    "MapAnalysis",
];
const OBJECT_FLAGS: &[&str] = &[
    "DRAWS_IN_MIRROR",
    "ROAD_POINT1",
    "ROAD_POINT2",
    "OBSOLETE_BRIDGE_POINT1",
    "OBSOLETE_BRIDGE_POINT2",
    "ROAD_CORNER_TIGHT",
    "ROAD_JOIN",
    "DONT_RENDER",
];
const PROPERTIES: &[&str] = &[
    "BoolProperty",
    "IntProperty",
    "RealProperty",
    "StringProperty",
    "UnicodeStringProperty",
    "AssetIdProperty",
    "AssetIdListProperty",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut fields: [Vec<_>; 12] = Default::default();
    let mut last = 0;
    for child in significant_children(node) {
        let index = FIELDS
            .iter()
            .position(|name| child.has_tag_name((SAGE_NS, *name)))
            .filter(|index| *index >= last)
            .ok_or_else(|| invalid(path, "unsupported or out-of-order GameMap element"))?;
        if matches!(index, 7..=9 | 11) && !fields[index].is_empty() {
            return Err(invalid(path, format!("duplicate {}", FIELDS[index])));
        }
        fields[index].push(child);
        last = index;
    }

    let mut data = BinaryData {
        payload: vec![0; 80],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (index, objects) in fields[..6].iter().enumerate() {
        let has_ownership = matches!(index, 1..=3);
        let stride = if has_ownership { 56 } else { 28 };
        let first = data.list(4 + index * 8, objects.len(), stride)?;
        for (index, &object) in objects.iter().enumerate() {
            map_object(
                object,
                first + index * stride,
                has_ownership,
                &mut data,
                &mut result,
            )?;
        }
    }
    let first_road = data.list(52, fields[6].len(), 4)?;
    for (index, &road) in fields[6].iter().enumerate() {
        validate_leaf(road, &["id"], path)?;
        reference(
            road.attribute("id"),
            "Road",
            first_road + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    // MapMetaData is marked pipeline-only in the SDK schema.
    if let Some(&environment) = fields[8].first() {
        validate_leaf(environment, &["Cloud", "Macro", "Environment"], path)?;
        let start = data.allocate(60, 12)?;
        for (index, name) in ["Cloud", "Macro", "Environment"].into_iter().enumerate() {
            reference(
                environment.attribute(name),
                "Texture",
                start + index * 4,
                &mut data,
                &mut result,
            )?;
        }
    }
    if let Some(&dictionary) = fields[9].first() {
        let start = data.allocate(64, 56)?;
        world_dictionary(dictionary, start, &mut data, &mut result)?;
    }
    let first_effect = data.list(68, fields[10].len(), 4)?;
    for (index, &effect) in fields[10].iter().enumerate() {
        validate_leaf(effect, &["Effect"], path)?;
        reference(
            effect.attribute("Effect"),
            "Texture",
            first_effect + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    if let Some(&analysis) = fields[11].first() {
        let start = data.allocate(76, 28)?;
        map_analysis(analysis, start, &mut data)?;
    }
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.sort_unstable();
            offsets.push(u32::MAX);
        }
    }
    result.payload = data.payload;
    result.relocations = data.relocations;
    Ok(result)
}

fn map_object(
    node: Node<'_, '_>,
    offset: usize,
    has_ownership: bool,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let fields = [
        "id",
        "ThingTemplate",
        "Flags",
        "Angle",
        "Faction",
        "Team",
        "Health",
        "EventList",
    ];
    validate_attributes(
        node,
        &fields[..if has_ownership { 8 } else { 4 }],
        data.path,
    )?;
    if let Some(name) = node.attribute("id") {
        data.word(offset, hash_name(name));
    }
    reference(
        node.attribute("ThingTemplate"),
        "GameObject",
        offset + 4,
        data,
        result,
    )?;
    let mut flags = 0;
    for flag in node
        .attribute("Flags")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        flags |= 1 << enum_value(flag, OBJECT_FLAGS, data.path)?;
    }
    data.word(offset + 8, flags);
    write_real(node, "Angle", offset + 12, data)?;
    let mut children = significant_children(node);
    let position = children
        .next()
        .filter(|node| node.has_tag_name((SAGE_NS, "Position")))
        .ok_or_else(|| invalid(data.path, "expected map object Position"))?;
    if children.next().is_some() {
        return Err(invalid(data.path, "expected a single map object Position"));
    }
    validate_leaf(position, &["x", "y", "z"], data.path)?;
    for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
        write_real(position, name, offset + 16 + index * 4, data)?;
    }
    if has_ownership {
        write_string(node, "Faction", offset + 28, data)?;
        write_string(node, "Team", offset + 36, data)?;
        write_real(node, "Health", offset + 44, data)?;
        write_string(node, "EventList", offset + 48, data)?;
    }
    Ok(())
}

fn world_dictionary(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    validate_attributes(node, &[], data.path)?;
    let mut properties: [Vec<_>; 7] = Default::default();
    for child in significant_children(node) {
        let index = PROPERTIES
            .iter()
            .position(|name| child.has_tag_name((SAGE_NS, *name)))
            .ok_or_else(|| invalid(data.path, "unsupported WorldDict property"))?;
        validate_leaf(child, &["Key", "Value"], data.path)?;
        properties[index].push(child);
    }
    for (kind, fields) in properties.into_iter().enumerate() {
        let stride = if matches!(kind, 3 | 4 | 6) { 16 } else { 12 };
        let first = data.list(offset + kind * 8, fields.len(), stride)?;
        for (index, field) in fields.into_iter().enumerate() {
            let start = first + index * stride;
            write_string(field, "Key", start, data)?;
            let Some(value) = field.attribute("Value") else {
                continue;
            };
            let offset = start + 8;
            match kind {
                0 => data.word(
                    offset,
                    u32::from(boolean(field, "Value", false, data.path)?),
                ),
                1 => data.word(offset, sage_int(value, "Value", data.path)?),
                2 => write_real(field, "Value", offset, data)?,
                3 | 4 => data.string(offset, value)?,
                5 => {
                    let value = value.trim();
                    let kind = value
                        .split_once(':')
                        .map_or("BaseAssetType", |(kind, _)| kind);
                    let start = result.reference_types.len();
                    reference(Some(value), kind, offset, data, result)?;
                    // The schema declares BaseAssetType even for a concrete qualified name.
                    result.reference_types[start..].fill(BASE_ASSET_TYPE);
                }
                6 => {
                    let values: Vec<_> = value.split_ascii_whitespace().collect();
                    let first = data.list(offset, values.len(), 8)?;
                    for (index, value) in values.into_iter().enumerate() {
                        data.string(first + index * 8, value)?;
                    }
                }
                _ => unreachable!("dictionary property index"),
            }
        }
    }
    Ok(())
}

fn map_analysis(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let fields = [
        "GarrisonableStructures",
        "TechStructures",
        "CellsExamined",
        "WaterCells",
        "WaterAreas",
    ];
    validate_attributes(node, &fields, data.path)?;
    for (index, field) in fields.into_iter().enumerate() {
        if let Some(value) = node.attribute(field) {
            let value = value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(data.path, format!("invalid {field}")))?;
            data.word(offset + index * 4, value as u32);
        }
    }
    let areas: Vec<_> = significant_children(node).collect();
    let start = data.list(offset + 20, areas.len(), 4)?;
    for (index, area) in areas.into_iter().enumerate() {
        if !area.has_tag_name((SAGE_NS, "WaterAreaType"))
            || area.children().any(|child| child.is_element())
        {
            return Err(invalid(data.path, "expected WaterAreaType"));
        }
        validate_attributes(area, &[], data.path)?;
        let value = enum_value(
            area.text().unwrap_or("").trim(),
            &["INTERIOR", "EXTERIOR", "EXTERIOR_PARTIAL"],
            data.path,
        )?;
        data.word(start + index * 4, value);
    }
    Ok(())
}

fn reference(
    value: Option<&str>,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        write_import(value, kind, offset, data, result)?;
    }
    Ok(())
}

fn write_real(
    node: Node<'_, '_>,
    field: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    if let Some(value) = node.attribute(field) {
        data.word(offset, finite_number(value, field, data.path)?.to_bits());
    }
    Ok(())
}

fn write_string(
    node: Node<'_, '_>,
    field: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    if let Some(text) = node.attribute(field) {
        data.string(offset, text)?;
    }
    Ok(())
}
