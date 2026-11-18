use super::binary::BinaryData;
use super::fx_list::{boolean, validate_leaf};
use super::global_ui::enum_value;
use super::{
    Marshaled, encode_words, finite_number, sage_int, sage_uint, significant_children,
    validate_attributes,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::{io, path::Path};

const GAME_LEVELS: &[&str] = &[
    "UNKNOWN",
    "VERY_LOW",
    "LOW",
    "MEDIUM",
    "HIGH",
    "ULTRA_HIGH",
    "CUSTOM",
];
const MODEL_LEVELS: &[&str] = &["LOW", "MEDIUM", "HIGH"];
const DETAIL_LEVELS: &[&str] = &["VERY_LOW", "LOW", "MEDIUM", "HIGH", "ULTRA_HIGH"];
const SHADOW_LEVELS: &[&str] = &["OFF", "LOW", "MEDIUM", "HIGH", "ULTRA_HIGH"];
const SHADER_LEVELS: &[&str] = &["LOW", "MEDIUM", "HIGH", "ULTRA_HIGH"];
const DECAL_LEVELS: &[&str] = &["OFF", "LOW", "HIGH"];
const PRIORITIES: &[&str] = &[
    "INVALID_PRIORITY",
    "ULTRA_HIGH_ONLY",
    "HIGH_OR_ABOVE",
    "MEDIUM_OR_ABOVE",
    "LOW_OR_ABOVE",
    "VERY_LOW_OR_ABOVE",
    "ALWAYS_RENDER",
];

fn level(value: &str, path: &Path) -> io::Result<u32> {
    Ok(enum_value(value, GAME_LEVELS, path)?.wrapping_sub(1))
}

pub(super) fn hardware(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id"], path)?;
    let devices: Vec<_> = significant_children(node).collect();
    let mut data = BinaryData {
        payload: vec![0; 12],
        relocations: Vec::new(),
        path,
    };
    let start = data.list(4, devices.len(), 24)?;
    for (index, device) in devices.into_iter().enumerate() {
        if !device.has_tag_name((SAGE_NS, "Device")) {
            return Err(invalid(path, "expected a GraphicsHardware Device"));
        }
        validate_leaf(
            device,
            &["VendorId", "DeviceId", "Name", "Level", "MacLevel"],
            path,
        )?;
        let offset = start + index * 24;
        for (name, field) in [("VendorId", 0), ("DeviceId", 4)] {
            data.word(
                offset + field,
                sage_uint(required(device, name, path)?, name, path)?,
            );
        }
        data.string(offset + 8, required(device, "Name", path)?)?;
        data.word(offset + 16, level(required(device, "Level", path)?, path)?);
        if let Some(mac) = device.attribute("MacLevel") {
            data.optional_word(offset + 20, level(mac, path)?)?;
        }
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn preset(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_attributes(node, &["id", "Level", "SystemMemory", "Xres", "YRes"], path)?;
    let mut data = BinaryData {
        payload: vec![0; 28],
        relocations: Vec::new(),
        path,
    };
    data.word(4, level(node.attribute("Level").unwrap_or("MEDIUM"), path)?);
    for (index, (name, default)) in [("SystemMemory", "500"), ("Xres", "1024"), ("YRes", "768")]
        .into_iter()
        .enumerate()
    {
        data.word(
            8 + index * 4,
            sage_int(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    if significant_children(node).next().is_none() {
        return Err(invalid(path, "expected a GameLODPreset Processor"));
    }
    write_processors(node, 20, &mut data)?;
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn dynamic_lod(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    validate_leaf(
        node,
        &[
            "id",
            "Level",
            "MinimumFPS",
            "ParticleSkipMask",
            "DebrisSkipMask",
            "SlowDeathScale",
        ],
        path,
    )?;
    let mut words = vec![0, level(node.attribute("Level").unwrap_or("MEDIUM"), path)?];
    for (name, default) in [
        ("MinimumFPS", "10"),
        ("ParticleSkipMask", "1"),
        ("DebrisSkipMask", "0"),
    ] {
        words.push(sage_int(
            node.attribute(name).unwrap_or(default),
            name,
            path,
        )?);
    }
    words.push(
        finite_number(
            node.attribute("SlowDeathScale").unwrap_or("1.0"),
            "SlowDeathScale",
            path,
        )?
        .to_bits(),
    );
    Ok(Marshaled {
        payload: encode_words(words),
        ..Marshaled::default()
    })
}

pub(super) fn static_lod(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    const ENUMS: &[(&str, &str, &[&str], usize)] = &[
        ("ModelLOD", "HIGH", MODEL_LEVELS, 8),
        ("EffectsLOD", "MEDIUM", DETAIL_LEVELS, 12),
        ("ShadowLOD", "MEDIUM", SHADOW_LEVELS, 20),
        ("TerrainLOD", "MEDIUM", SHADER_LEVELS, 24),
        ("WaterLOD", "MEDIUM", SHADER_LEVELS, 28),
        ("AnimationLOD", "HIGH", DETAIL_LEVELS, 48),
        ("ShaderLOD", "MEDIUM", SHADER_LEVELS, 52),
        ("DecalLOD", "HIGH", DECAL_LEVELS, 56),
        ("MinParticlePriority", "MEDIUM_OR_ABOVE", PRIORITIES, 60),
        ("MinParticleSkipPriority", "ALWAYS_RENDER", PRIORITIES, 64),
    ];
    const COUNTS: &[(&str, &str, usize)] = &[
        ("MaxParticleCount", "1500", 16),
        ("MaxTankTrackEdges", "100", 32),
        ("MaxTankTrackOpaqueEdges", "25", 36),
        ("MaxTankTrackFadeDelay", "30000", 40),
        ("TextureReductionFactor", "0", 44),
        ("AntiAliasingQuality", "0", 68),
    ];
    const FLAGS: &[(&str, bool)] = &[
        ("UseShadowDecals", true),
        ("UseShadowMapping", false),
        ("UseTerrainNormalMap", false),
        ("ShowProps", true),
        ("EnableVSync", true),
    ];
    let names: Vec<_> = ["id", "Level"]
        .into_iter()
        .chain(ENUMS.iter().map(|field| field.0))
        .chain(COUNTS.iter().map(|field| field.0))
        .chain(FLAGS.iter().map(|field| field.0))
        .collect();
    validate_leaf(node, &names, path)?;
    let mut data = BinaryData {
        payload: vec![0; 80],
        relocations: Vec::new(),
        path,
    };
    data.word(4, level(node.attribute("Level").unwrap_or("MEDIUM"), path)?);
    for &(name, default, options, offset) in ENUMS {
        data.word(
            offset,
            enum_value(node.attribute(name).unwrap_or(default), options, path)?,
        );
    }
    for &(name, default, offset) in COUNTS {
        data.word(
            offset,
            sage_int(node.attribute(name).unwrap_or(default), name, path)?,
        );
    }
    for (index, &(name, default)) in FLAGS.iter().enumerate() {
        data.payload[72 + index] = boolean(node, name, default, path)?;
    }
    Ok(data.finish(Marshaled::default()))
}

pub(super) fn write_processors(
    node: Node<'_, '_>,
    field: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let path = data.path;
    let processors: Vec<_> = significant_children(node).collect();
    let start = data.list(field, processors.len(), 8)?;
    for (index, processor) in processors.into_iter().enumerate() {
        if !processor.has_tag_name((SAGE_NS, "Processor")) {
            return Err(invalid(path, "expected a quality-level Processor"));
        }
        validate_leaf(processor, &["Type", "MinMHz"], path)?;
        let kind = enum_value(
            required(processor, "Type", path)?,
            &["UNKNOWN", "PENTIUM_4", "CORE_DUO", "ATHLON", "ATHLON_64"],
            path,
        )?;
        let offset = start + index * 8;
        data.word(offset, kind);
        data.word(
            offset + 4,
            sage_int(required(processor, "MinMHz", path)?, "MinMHz", path)?,
        );
    }
    Ok(())
}
