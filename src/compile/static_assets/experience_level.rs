use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::game_object::write_model_conditions;
use super::{
    Marshaled, asset_reference, color4, finite_number, percentage, sage_time, significant_children,
    text_value, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path};

const INTEGER_FIELDS: [(&str, i32); 4] = [
    ("Rank", 0),
    ("RequiredExperience", 1),
    ("ExperienceAward", 0),
    ("ExperienceAwardOwnGuysDie", 0),
];
const FLAGS: [&str; 4] = [
    "InformUpdateModule",
    "SinglePlayerOnly",
    "MultiPlayerOnly",
    "ShowLevelUpTint",
];
const TIMES: [&str; 3] = [
    "LevelUpTintPreColorTime",
    "LevelUpTintPostColorTime",
    "LevelUpTintSustainColorTime",
];
const REALS: [&str; 2] = ["LevelUpTintFrequency", "LevelUpTintAmplitude"];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let mut attributes = vec!["id", "Prerequisite", "LevelUpOCL", "ModelConditionState"];
    attributes.extend(INTEGER_FIELDS.map(|(name, _)| name));
    attributes.extend(FLAGS);
    attributes.extend(TIMES);
    attributes.extend(REALS);
    validate_attributes(node, &attributes, path)?;

    let mut data = BinaryData {
        payload: vec![0; 0x6c],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    for (index, (name, default)) in INTEGER_FIELDS.into_iter().enumerate() {
        let value = node.attribute(name).map_or(Ok(default as u32), |raw| {
            let raw = raw.trim();
            let parsed = if let Some(hex) = raw.strip_prefix("0x") {
                u32::from_str_radix(hex, 16)
            } else {
                raw.parse::<i32>().map(|value| value as u32)
            };
            parsed.map_err(|_| invalid(path, format!("invalid {name} {raw:?}")))
        })?;
        data.word(4 + index * 4, value);
    }
    for (index, name) in TIMES.into_iter().enumerate() {
        let value = sage_time(node.attribute(name).unwrap_or("0s"), name, path)?;
        data.word(0x20 + index * 4, value.to_bits());
    }
    for (index, name) in REALS.into_iter().enumerate() {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(0x2c + index * 4, value.to_bits());
    }
    for (index, name) in FLAGS.into_iter().enumerate() {
        data.payload[0x68 + index] = boolean(node, name, false, path)?;
    }
    let prerequisite = node
        .attribute("Prerequisite")
        .filter(|value| !value.trim().is_empty())
        .map(|_| data.allocate(0x14, 4))
        .transpose()?;
    if let Some(value) = node.attribute("ModelConditionState") {
        let offset = data.allocate(0x1c, 0x3c)?;
        write_model_conditions(value, offset, &mut data)?;
    }
    for attr in node
        .attributes()
        .filter(|attr| !attr.value().trim().is_empty())
    {
        match attr.name() {
            "Prerequisite" => write_import(
                attr.value(),
                "ExperienceLevelTemplate",
                prerequisite.expect("nonempty prerequisite"),
                &mut data,
                &mut result,
            )?,
            "LevelUpOCL" => write_import(
                attr.value(),
                "ObjectCreationList",
                0x18,
                &mut data,
                &mut result,
            )?,
            _ => {}
        }
    }
    // Root import slots are emitted before the prerequisite's allocated slot.
    result.imports.sort_unstable();

    let mut tint = None;
    let mut decal = None;
    let mut upgrades = Vec::new();
    let mut effects = Vec::new();
    let mut targets = Vec::new();
    let mut modifiers = Vec::new();
    for child in significant_children(node) {
        match (child.tag_name().namespace(), child.tag_name().name()) {
            (Some(SAGE_NS), "LevelUpTintColor") if tint.is_none() => tint = Some(child),
            (Some(SAGE_NS), "Upgrade") => upgrades.push(child),
            (Some(SAGE_NS), "LevelUpFxList") => effects.push(child),
            (Some(SAGE_NS), "Target") => targets.push(child),
            (Some(SAGE_NS), "AttributeModifier") => modifiers.push(child),
            (Some(SAGE_NS), "Decal") if decal.is_none() => decal = Some(child),
            _ => {
                return Err(invalid(
                    path,
                    format!(
                        "unexpected ExperienceLevelTemplate child {}",
                        child.tag_name().name()
                    ),
                ));
            }
        }
    }
    let tint = tint.ok_or_else(|| invalid(path, "missing LevelUpTintColor"))?;
    let channels = ["r", "g", "b", "a"];
    validate_leaf(tint, &channels, path)?;
    for (index, channel) in channels.into_iter().enumerate() {
        let value = finite_number(tint.attribute(channel).unwrap_or("0"), channel, path)?;
        data.word(0x34 + index * 4, value.to_bits());
    }
    weak_list(&upgrades, 0x44, "UpgradeTemplate", &mut data, &mut result)?;
    let first = data.list(0x4c, effects.len(), 12)?;
    for (index, effect) in effects.into_iter().enumerate() {
        validate_leaf(effect, &["FxList", "BoneName"], path)?;
        let offset = first + index * 12;
        if let Some(value) = effect
            .attribute("FxList")
            .filter(|value| !value.trim().is_empty())
        {
            write_import(value, "FXList", offset, &mut data, &mut result)?;
        }
        if let Some(name) = effect.attribute("BoneName") {
            data.string(offset + 4, name)?;
        }
    }
    weak_list(&targets, 0x54, "GameObject", &mut data, &mut result)?;
    let first = data.list(0x5c, modifiers.len(), 4)?;
    for (index, modifier) in modifiers.into_iter().enumerate() {
        let value = text_value(modifier, path)?;
        write_import(
            value,
            "AttributeModifier",
            first + index * 4,
            &mut data,
            &mut result,
        )?;
    }
    if let Some(decal) = decal {
        let header = data.allocate(0x64, 8)?;
        radius_decal(decal, header, &mut data, &mut result)?;
    }
    data.relocations.sort_unstable();
    for offsets in [&mut data.relocations, &mut result.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    result.relocations = data.relocations;
    result.payload = data.payload;
    Ok(result)
}

fn weak_list(
    nodes: &[Node<'_, '_>],
    offset: usize,
    kind: &str,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    let first = data.list(offset, nodes.len(), 4)?;
    for (index, &node) in nodes.iter().enumerate() {
        let value = text_value(node, data.path)?;
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let reference = asset_reference(value, kind, data.path)?;
        data.word(first + index * 4, reference.instance_id);
        result.reference_types.push(reference.type_id);
    }
    Ok(())
}

pub(super) fn radius_decal(
    node: Node<'_, '_>,
    header: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    const STYLES: &[&str] = &[
        "NONE",
        "DECAL",
        "ALPHA_DECAL",
        "ADDITIVE_DECAL",
        "ALPHA_DECAL_DYNAMIC",
        "ADDITIVE_DECAL_DYNAMIC",
        "MERGE_DECAL",
    ];
    const NUMBERS: [(&str, &str); 12] = [
        ("OpacityMin", "1"),
        ("OpacityMax", "1"),
        ("OpacityPulsesPerSecond", "0"),
        ("RotationMin", "0"),
        ("RotationMax", "0"),
        ("RotationDegreesPerSecond", "0"),
        ("MinRadius", "1"),
        ("MaxRadius", "1"),
        ("RadiusPulsesPerSecond", "0"),
        ("PositionOffsetMin", "0"),
        ("PositionOffsetMax", "0"),
        ("PositionOffsetPulsesPerSecond", "0"),
    ];
    let path = data.path;
    validate_attributes(node, &[], path)?;
    let mut attributes = vec!["Texture", "Texture2", "Style"];
    attributes.extend(NUMBERS.map(|(name, _)| name));
    let items: Vec<_> = significant_children(node).collect();

    let first = data.list(header, items.len(), 0x40)?;
    for (index, component) in items.into_iter().enumerate() {
        if !component.has_tag_name((SAGE_NS, "Component")) {
            return Err(invalid(path, "unexpected RadiusDecalTemplate child"));
        }
        validate_attributes(component, &attributes, path)?;
        let start = first + index * 0x40;
        for attr in component
            .attributes()
            .filter(|attr| !attr.value().trim().is_empty())
        {
            let offset = match attr.name() {
                "Texture" => start,
                "Texture2" => start + 4,
                _ => continue,
            };
            write_import(attr.value(), "Texture", offset, data, result)?;
        }
        let style = super::ai_strategic::enum_index(
            component.attribute("Style").unwrap_or("NONE"),
            STYLES,
            "Style",
            path,
        )?;
        data.word(start + 8, style);
        for (index, (name, default)) in NUMBERS.into_iter().enumerate() {
            let value = component.attribute(name).unwrap_or(default);
            let value = if index < 2 {
                percentage(value, name, path)?
            } else {
                finite_number(value, name, path)?
            };
            data.word(start + 12 + index * 4, value.to_bits());
        }
        let mut children = significant_children(component);
        if let Some(color) = children.next() {
            if !color.has_tag_name((SAGE_NS, "Color")) || children.next().is_some() {
                return Err(invalid(path, "unexpected RadiusDecalComponent child"));
            }
            data.word(start + 0x3c, color4(color, path)?);
        }
    }
    Ok(())
}
