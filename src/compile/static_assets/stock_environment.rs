use super::binary::BinaryData;
use super::fx_list::{boolean, random_variable, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, asset_reference, color4, finite_number, significant_children, validate_attributes,
    write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

pub(super) fn environment(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let [cloud] = children(node, ["CloudEffect"], path)?;
    let cloud = cloud.ok_or_else(|| invalid(path, "missing Environment CloudEffect"))?;
    validate_attributes(
        cloud,
        &[
            "CloudTexture",
            "DarkCloudTexture",
            "AlphaTexture",
            "PropagateSpeed",
            "Angle",
            "DarkeningRate",
            "LighteningRate",
            "CloudScrollSpeed",
            "DissipateTexture",
            "DissipateStartLevel",
            "DissipateSpeed",
            "DissipateRateScale",
            "LightningShadows",
            "JitterLightningLightIntensity",
            "JitterLightningLightPosition",
            "LightningChance",
            "LightningFrequency",
            "LightningShadowIntensity",
            "LightningFX",
        ],
        path,
    )?;
    let [
        darkening,
        darkening_rain,
        shadow_color,
        light_position1,
        light_position2,
        light_position3,
        intensity,
        duration,
    ] = children(
        cloud,
        [
            "DarkeningFactor",
            "DarkeningFactorRain",
            "LightningShadowColor",
            "LightningLightPosition1",
            "LightningLightPosition2",
            "LightningLightPosition3",
            "LightningIntensity",
            "LightningDuration",
        ],
        path,
    )?;
    let mut data = payload(path, 156);
    let mut result = Marshaled::default();
    for attribute in cloud.attributes() {
        let offset = match attribute.name() {
            "CloudTexture" => 4,
            "DarkCloudTexture" => 8,
            "AlphaTexture" => 12,
            "DissipateTexture" => 36,
            "LightningFX" => {
                let effect = asset_reference(attribute.value(), "FXList", path)?;
                result.reference_types.push(effect.type_id);
                data.word(64, effect.instance_id);
                continue;
            }
            _ => continue,
        };
        write_import(attribute.value(), "Texture", offset, &mut data, &mut result)?;
    }
    result.imports.sort_unstable();

    reals(
        cloud,
        0,
        &[
            ("PropagateSpeed", 16, "0"),
            ("CloudScrollSpeed", 32, "0"),
            ("DissipateStartLevel", 40, "0"),
            ("DissipateSpeed", 44, "0"),
            ("DissipateRateScale", 48, "0"),
            ("LightningChance", 52, "0"),
            ("LightningFrequency", 56, "0"),
            ("LightningShadowIntensity", 60, "0"),
        ],
        &mut data,
    )?;
    for (name, offset) in [("Angle", 20), ("DarkeningRate", 24), ("LighteningRate", 28)] {
        data.word(offset, int(cloud, name, "0", path)?);
    }
    for (color, offset) in [(darkening, 68), (darkening_rain, 80), (shadow_color, 92)] {
        if let Some(color) = color {
            validate_leaf(color, &["R", "G", "B"], path)?;
            for (i, name) in ["R", "G", "B"].into_iter().enumerate() {
                data.word(
                    offset + i * 4,
                    finite_number(required(color, name, path)?, name, path)?.to_bits(),
                );
            }
        }
    }
    for (coord, offset) in [
        (light_position1, 104),
        (light_position2, 112),
        (light_position3, 120),
    ] {
        if let Some(coord) = coord {
            vec2(coord, offset, &mut data)?;
        }
    }
    for (variable, offset) in [(intensity, 128), (duration, 140)] {
        if let Some(variable) = variable {
            random_variable(variable, offset, &mut data)?;
        }
    }
    for (i, name) in [
        "LightningShadows",
        "JitterLightningLightIntensity",
        "JitterLightningLightPosition",
    ]
    .into_iter()
    .enumerate()
    {
        data.payload[152 + i] = boolean(cloud, name, false, path)?;
    }
    Ok(data.finish(result))
}

pub(super) fn shadow_map(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(
        node,
        &[
            "id",
            "MapSize",
            "MaxViewDistance",
            "MinShadowedTerrainHeight",
        ],
        path,
    )?;
    let mut data = payload(path, 16);
    data.word(4, int(node, "MapSize", "1024", path)?);
    reals(
        node,
        0,
        &[
            ("MaxViewDistance", 8, "1000.0"),
            ("MinShadowedTerrainHeight", 12, "0"),
        ],
        &mut data,
    )?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn water_setting(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "TimeOfDay"], path)?;
    let [diffuse] = children(node, ["DiffuseColor"], path)?;
    let diffuse = diffuse.ok_or_else(|| invalid(path, "missing WaterSetting DiffuseColor"))?;
    let time = enum_value(
        required(node, "TimeOfDay", path)?,
        &[
            "INVALID",
            "MORNING",
            "AFTERNOON",
            "EVENING",
            "NIGHT",
            "INTERPOLATE",
        ],
        path,
    )?;
    let mut data = payload(path, 12);
    data.word(4, time);
    data.word(8, color4(diffuse, path)?);
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn water_transparency(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(
        node,
        &[
            "id",
            "TransparentWaterDepth",
            "TransparentWaterMinOpacity",
            "StandingWaterTexture",
        ],
        path,
    )?;
    let mut data = payload(path, 16);
    reals(
        node,
        0,
        &[
            ("TransparentWaterDepth", 4, "3.0"),
            ("TransparentWaterMinOpacity", 8, "1.0"),
        ],
        &mut data,
    )?;
    let mut result = Marshaled::default();
    if let Some(value) = node.attribute("StandingWaterTexture") {
        let texture = asset_reference(value, "Texture", path)?;
        data.word(12, texture.instance_id);
        result.reference_types.push(texture.type_id);
    }
    Ok(data.finish(result))
}

pub(super) fn weather(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let mut definitions = [None; 3];
    let mut entries = Vec::new();
    for child in significant_children(node) {
        let name = child.tag_name().name();
        match name {
            "WeatherData" if child.tag_name().namespace() == Some(SAGE_NS) => entries.push(child),
            _ => add_child(
                child,
                &["Rain", "Lightning", "Cloud"],
                &mut definitions,
                path,
            )?,
        }
    }
    let mut data = payload(path, 24);
    let mut result = Marshaled::default();
    let [rain, lightning, cloud] = definitions;
    if let Some(rain) = rain {
        let offset = data.allocate(4, 60)?;
        encode_rain(rain, offset, &mut data, &mut result)?;
    }
    if let Some(lightning) = lightning {
        let offset = data.allocate(8, 12)?;
        validate_attributes(lightning, &["LightningDuration", "LightningChance"], path)?;
        data.word(offset, int(lightning, "LightningDuration", "30", path)?);
        reals(
            lightning,
            offset,
            &[("LightningChance", 4, "0.01")],
            &mut data,
        )?;
        let [factor] = children(lightning, ["LightningFactor"], path)?;
        if let Some(factor) = factor {
            let offset = data.allocate(offset + 8, 12)?;
            random_variable(factor, offset, &mut data)?;
        }
    }
    if let Some(cloud) = cloud {
        let offset = data.allocate(12, 8)?;
        validate_attributes(cloud, &[], path)?;
        let [size, drift] = children(cloud, ["CloudTextureSize", "CloudOffsetPerSecond"], path)?;
        for (child, pointer) in [(size, offset), (drift, offset + 4)] {
            if let Some(child) = child {
                let offset = data.allocate(pointer, 8)?;
                vec2(child, offset, &mut data)?;
            }
        }
    }
    let offset = data.list(16, entries.len(), 12)?;
    for (index, entry) in entries.into_iter().enumerate() {
        validate_leaf(entry, &["id", "HasLightning", "Sound"], path)?;
        let offset = offset + index * 12;
        let kind = enum_value(
            entry.attribute("id").unwrap_or("NORMAL"),
            &[
                "NORMAL",
                "CLOUDY",
                "RAINY",
                "CLOUDYRAINY",
                "SUNNY",
                "SNOWY",
                "INVALID",
            ],
            path,
        )?;
        data.word(offset, kind);
        if let Some(value) = entry.attribute("Sound") {
            write_import(
                value,
                "BaseAudioEventInfo",
                offset + 4,
                &mut data,
                &mut result,
            )?;
        }
        data.payload[offset + 8] = boolean(entry, "HasLightning", false, path)?;
    }
    data.relocations.sort_unstable();
    Ok(data.finish(result))
}

fn encode_rain(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
    result: &mut Marshaled,
) -> io::Result<()> {
    const REALS: &[(&str, usize, &str)] = &[
        ("RainBoxWidth", 8, "200"),
        ("RainBoxLength", 12, "200"),
        ("RainBoxHeight", 16, "200"),
        ("MinWidth", 20, "0.5"),
        ("MaxWidth", 24, "1.5"),
        ("MinHeight", 28, "5"),
        ("MaxHeight", 32, "15"),
        ("MinSpeed", 36, "50"),
        ("MaxSpeed", 40, "150"),
        ("MinAlpha", 44, "0.1"),
        ("MaxAlpha", 48, "0.5"),
        ("WindStrength", 52, "1"),
    ];
    let allowed: Vec<_> = ["RainTexture", "IsRaining", "NumRaindropsPerBox"]
        .into_iter()
        .chain(REALS.iter().map(|&(name, _, _)| name))
        .collect();
    let path = data.path;
    validate_leaf(node, &allowed, path)?;
    if let Some(value) = node.attribute("RainTexture") {
        write_import(value, "Texture", offset, data, result)?;
    }
    data.word(offset + 4, int(node, "NumRaindropsPerBox", "16000", path)?);
    reals(node, offset, REALS, data)?;
    data.payload[offset + 56] = boolean(node, "IsRaining", false, path)?;
    Ok(())
}

fn payload(path: &Path, size: usize) -> BinaryData<'_> {
    BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    }
}

fn children<'a, 'input, const N: usize>(
    parent: Node<'a, 'input>,
    names: [&str; N],
    path: &Path,
) -> io::Result<[Option<Node<'a, 'input>>; N]> {
    let mut children = [None; N];
    for child in significant_children(parent) {
        add_child(child, &names, &mut children, path)?;
    }
    Ok(children)
}

fn add_child<'a, 'input>(
    child: Node<'a, 'input>,
    names: &[&str],
    children: &mut [Option<Node<'a, 'input>>],
    path: &Path,
) -> io::Result<()> {
    let tag = child.tag_name();
    if let Some(index) = names
        .iter()
        .position(|&name| child.has_tag_name((SAGE_NS, name)))
        && children[index].is_none()
    {
        children[index] = Some(child);
        return Ok(());
    }
    Err(invalid(
        path,
        format!("unexpected or duplicate {} child", tag.name()),
    ))
}

fn vec2(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    validate_leaf(node, &["x", "y"], data.path)?;
    reals(node, offset, &[("x", 0, "0"), ("y", 4, "0")], data)
}

fn int(node: Node<'_, '_>, name: &str, default: &str, path: &Path) -> io::Result<u32> {
    node.attribute(name)
        .unwrap_or(default)
        .trim()
        .parse::<i32>()
        .map(|value| value as u32)
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

fn reals(
    node: Node<'_, '_>,
    base: usize,
    fields: &[(&str, usize, &str)],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for &(name, offset, default) in fields {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, data.path)?;
        data.word(base + offset, value.to_bits());
    }
    Ok(())
}
