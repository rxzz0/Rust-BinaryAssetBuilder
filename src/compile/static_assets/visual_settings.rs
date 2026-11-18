use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf, vector};
use super::global_ui::enum_value;
use super::subsystem_settings::subsystem;
use super::{
    Marshaled, asset_reference, color4, finite_number, sage_int, sage_uint, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use crate::hash::fast_hash;
use roxmltree::Node;
use std::{io, path::Path};

pub(super) fn lighting(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const SCALE: &str = "OccludedLuminanceScale";
    validate_attributes(node, &["id", "LoadAfter", "TimeOfDay", SCALE], path)?;
    let (mut data, result) = subsystem(node, 48, path)?;
    data.word(8, time_of_day(node, path)?);
    data.word(
        12,
        finite_number(node.attribute(SCALE).unwrap_or("0.5"), SCALE, path)?.to_bits(),
    );
    let mut children = significant_children(node);
    let multiplier = child(&mut children, "LightingMultiplierIfNoClouds", path)?;
    write_rgb(multiplier, 16, &mut data)?;
    let current = child(&mut children, "CurrentLightingSettings", path)?;
    write_lighting_time(current, 28, &mut data)?;
    let terrain = child(&mut children, "TerrainLighting", path)?;
    no_children_left(children, node, path)?;
    validate_attributes(terrain, &[], path)?;
    let start = data.list(40, 6, 12)?;
    let mut times = significant_children(terrain);
    for offset in (start..start + 72).step_by(12) {
        let time = child(&mut times, "LightingTimeGroup", path)?;
        write_lighting_time(time, offset, &mut data)?;
    }
    no_children_left(times, terrain, path)?;
    Ok(data.finish(result))
}

fn write_lighting_time(
    node: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let path = data.path;
    validate_attributes(node, &["TimeOfDay"], path)?;
    data.word(offset, time_of_day(node, path)?);
    let mut lights = significant_children(node);
    let start = data.list(offset + 4, 3, 36)?;
    for offset in (start..start + 108).step_by(36) {
        let light = child(&mut lights, "Lighting", path)?;
        validate_attributes(light, &[], path)?;
        let mut values = significant_children(light);
        let ambient = child(&mut values, "Ambient", path)?;
        write_rgb(ambient, offset, data)?;
        let diffuse = child(&mut values, "Diffuse", path)?;
        write_rgb(diffuse, offset + 12, data)?;
        let position = child(&mut values, "LightPos", path)?;
        vector(position, false, offset + 24, data)?;
        no_children_left(values, light, path)?;
    }
    no_children_left(lights, node, path)
}

fn time_of_day(node: Node<'_, '_>, path: &Path) -> io::Result<u32> {
    enum_value(
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
    )
}

fn write_rgb(node: Node<'_, '_>, offset: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let channels = ["R", "G", "B"];
    let path = data.path;
    validate_leaf(node, &channels, path)?;
    for (index, channel) in channels.into_iter().enumerate() {
        let value = finite_number(required(node, channel, path)?, channel, path)?;
        data.word(offset + index * 4, value.to_bits());
    }
    Ok(())
}

pub(super) fn damage(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(node, &["id", "GroundDamageFX"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 8],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(value) = node.attribute("GroundDamageFX") {
        let offset = data.allocate(4, 4)?;
        write_import(value, "DamageFX", offset, &mut data, &mut result)?;
    }
    Ok(data.finish(result))
}

pub(super) fn draw_group(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const INTS: [(&str, &str); 4] = [
        ("DropShadowOffsetX", "-1"),
        ("DropShadowOffsetY", "-1"),
        ("PixelOffetXInteger", "0"),
        ("PixelOffetYInteger", "0"),
    ];
    const REALS: [&str; 2] = ["PixelOffetXReal", "PixelOffetYReal"];
    const FLAGS: [(&str, bool); 3] = [
        ("IsBold", false),
        ("UsePlayerColor", true),
        ("PixelOffsetAsPercentage", false),
    ];
    let names: Vec<_> = ["id", "FontName", "FontSize"]
        .into_iter()
        .chain(INTS.map(|(name, _)| name))
        .chain(REALS)
        .chain(FLAGS.map(|(name, _)| name))
        .collect();
    validate_attributes(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 52],
        relocations: Vec::new(),
        path,
    };
    data.string(4, node.attribute("FontName").unwrap_or("Arial"))?;
    data.word(
        12,
        sage_uint(node.attribute("FontSize").unwrap_or("12"), "FontSize", path)?,
    );
    for (index, (name, default)) in INTS.into_iter().enumerate() {
        data.word(
            16 + index * 4,
            sage_int(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    for (index, name) in REALS.into_iter().enumerate() {
        data.word(
            32 + index * 4,
            finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
        );
    }
    for (index, (name, default)) in FLAGS.into_iter().enumerate() {
        data.payload[48 + index] = boolean(node, name, default, path)?;
    }
    let mut children = significant_children(node);
    for (name, offset) in [("TextColor", 40), ("TextDropShadowColor", 44)] {
        let color = child(&mut children, name, path)?;
        data.word(offset, color4(color, path)?);
    }
    no_children_left(children, node, path)?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn auto_particles(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const TYPES: [&str; 7] = [
        "FireSmall",
        "FireMedium",
        "FireLarge",
        "SmokeSmall",
        "SmokeMedium",
        "SmokeLarge",
        "Aflame",
    ];
    const SCALE: &str = "ParticleScale";
    const MAX: &str = "MaxParticleCount";
    validate_attributes(node, &["id", SCALE, MAX], path)?;
    let mut data = BinaryData {
        payload: vec![0; 124],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    data.word(
        4,
        finite_number(node.attribute(SCALE).unwrap_or("0"), SCALE, path)?.to_bits(),
    );
    data.word(
        8,
        sage_int(node.attribute(MAX).unwrap_or("2500"), MAX, path)?,
    );
    let mut children = significant_children(node);
    for (index, name) in TYPES.into_iter().enumerate() {
        let particle = child(&mut children, name, path)?;
        validate_leaf(
            particle,
            &["ParticlePrefix", "ParticleSystemId", "ParticleMax"],
            path,
        )?;
        let offset = 12 + index * 16;
        data.string(offset, required(particle, "ParticlePrefix", path)?)?;
        let target = required(particle, "ParticleSystemId", path)?;
        let id = asset_reference(target, "FXParticleSystemTemplate", path)?;
        data.word(offset + 8, id.instance_id);
        result
            .reference_types
            .push(fast_hash(b"FXParticleSystemTemplate"));
        let max = required(particle, "ParticleMax", path)?;
        data.word(offset + 12, sage_int(max, "ParticleMax", path)?);
    }
    no_children_left(children, node, path)?;
    Ok(data.finish(result))
}

fn child<'a, 'input>(
    children: &mut impl Iterator<Item = Node<'a, 'input>>,
    name: &str,
    path: &Path,
) -> io::Result<Node<'a, 'input>> {
    children
        .next()
        .filter(|node| node.has_tag_name((SAGE_NS, name)))
        .ok_or_else(|| invalid(path, format!("expected {name}")))
}

fn no_children_left(
    mut children: impl Iterator,
    parent: Node<'_, '_>,
    path: &Path,
) -> io::Result<()> {
    if children.next().is_some() {
        return Err(invalid(
            path,
            format!("unexpected {} child", parent.tag_name().name()),
        ));
    }
    Ok(())
}
