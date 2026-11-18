use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::{
    Marshaled, asset_reference, encode_words, finite_number, sage_int, sage_time,
    significant_children, validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid, required};
use crate::hash::fast_hash;
use roxmltree::Node;
use std::{io, path::Path};

pub(in crate::compile) const BASE: &str = "BaseSubsystemSettings";
pub(in crate::compile) const TYPES: &[&str] = &[
    "BreezeInfo",
    "ModelSettings",
    "LightingSettings",
    "SkirmishSettings",
    "TerrainVisualSettings",
];

pub(super) fn network(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const SLACK: &str = "RunAheadSlack";
    validate_leaf(node, &["id", SLACK], path)?;
    let slack = sage_int(node.attribute(SLACK).unwrap_or("30"), SLACK, path)?;
    Ok(Marshaled {
        payload: encode_words([0, slack]),
        ..Marshaled::default()
    })
}

pub(super) fn skirmish(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "LoadAfter", "RandomCrateSpawner"], path)?;
    let (mut data, mut result) = subsystem(node, 20, path)?;
    let speeds: Vec<_> = significant_children(node).collect();
    let start = data.list(12, speeds.len(), 4)?;
    for (index, speed) in speeds.into_iter().enumerate() {
        if !speed.has_tag_name((SAGE_NS, "HandicapBuildSpeed")) {
            return Err(invalid(path, "expected a HandicapBuildSpeed"));
        }
        validate_leaf(speed, &["Speed"], path)?;
        let value = finite_number(speed.attribute("Speed").unwrap_or("0"), "Speed", path)?;
        data.word(start + index * 4, value.to_bits());
    }
    // Weak dependency types are ordered alongside the strong dependency in XML order.
    if let Some(crate_spawner) = node.attribute("RandomCrateSpawner") {
        let id = asset_reference(crate_spawner, "GameObject", path)?;
        data.word(8, id.instance_id);
        let index = usize::from(
            node.attributes()
                .find(|attribute| matches!(attribute.name(), "LoadAfter" | "RandomCrateSpawner"))
                .is_some_and(|attribute| attribute.name() == "LoadAfter"),
        );
        result
            .reference_types
            .insert(index, fast_hash(b"GameObject"));
    }
    Ok(data.finish(result))
}

pub(super) fn breeze(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const PERIOD: &str = "BreezePeriod";
    const DEFAULTS: [(&str, &str); 4] = [
        ("Direction", "1.0472"),
        ("Intensity", "0.027488125"),
        ("Lean", "0.0157075"),
        ("Randomness", "0.2"),
    ];
    let names: Vec<_> = ["id", "LoadAfter", PERIOD]
        .into_iter()
        .chain(DEFAULTS.map(|(name, _)| name))
        .collect();
    validate_leaf(node, &names, path)?;
    let (mut data, result) = subsystem(node, 28, path)?;
    for (index, (name, default)) in DEFAULTS.into_iter().enumerate() {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(8 + index * 4, value.to_bits());
    }
    data.word(
        24,
        sage_time(node.attribute(PERIOD).unwrap_or("10s"), PERIOD, path)?.to_bits(),
    );
    Ok(data.finish(result))
}

pub(super) fn model(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const NAMES: &[&str] = &[
        "id",
        "LoadAfter",
        "AnimationSharingCap",
        "AnimationSharingFrameTolerance",
        "AnimationSharingDrasticThreshold",
        "ForceModelsToFollowTimeOfDay",
        "ForceModelsToFollowWeather",
    ];
    validate_leaf(node, NAMES, path)?;
    let (mut data, result) = subsystem(node, 24, path)?;
    data.word(
        8,
        sage_int(required(node, NAMES[2], path)?, NAMES[2], path)?,
    );
    for (index, &name) in NAMES[3..5].iter().enumerate() {
        data.word(
            12 + index * 4,
            finite_number(required(node, name, path)?, name, path)?.to_bits(),
        );
    }
    for (index, &name) in NAMES[5..].iter().enumerate() {
        required(node, name, path)?;
        data.payload[20 + index] = boolean(node, name, false, path)?;
    }
    Ok(data.finish(result))
}

pub(super) fn terrain(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "LoadAfter"], path)?;
    let (mut data, result) = subsystem(node, 28, path)?;
    let mut children = significant_children(node);
    for (name, offset, axes) in [
        ("WaterPosition", 8, &["x", "y", "z"][..]),
        ("WaterExtents", 20, &["x", "y"][..]),
    ] {
        let vector = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, name)))
            .ok_or_else(|| invalid(path, format!("expected {name}")))?;
        validate_leaf(vector, axes, path)?;
        for (index, &axis) in axes.iter().enumerate() {
            let value = finite_number(vector.attribute(axis).unwrap_or("0"), axis, path)?;
            data.word(offset + index * 4, value.to_bits());
        }
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TerrainVisualSettings child"));
    }
    Ok(data.finish(result))
}

pub(super) fn subsystem<'a>(
    node: Node<'_, '_>,
    size: usize,
    path: &'a Path,
) -> io::Result<(BinaryData<'a>, Marshaled)> {
    let mut data = BinaryData {
        payload: vec![0; size],
        relocations: Vec::new(),
        path,
    };
    let mut result = Marshaled::default();
    if let Some(value) = node.attribute("LoadAfter") {
        write_import(value, BASE, 4, &mut data, &mut result)?;
    }
    Ok((data, result))
}
