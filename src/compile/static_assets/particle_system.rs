mod effects;
mod emission;

use super::ai_strategic::enum_index;
use super::binary::BinaryData;
use super::fx_list::{boolean, random_values, random_variable, validate_leaf};
use super::{
    Marshaled, asset_reference, finite_number, percentage, schema_enums, significant_children,
    validate_attributes, write_import,
};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, path::Path, sync::OnceLock};

const ENUM_NAMES: [&str; 4] = [
    "FXParticleSystem_Priority",
    "FXParticleSystem_ShaderType",
    "FXParticleSystem_Type",
    "FXParticleSystem_WindMotion",
];
const ROOT_FLAGS: [&str; 5] = [
    "IsOneShot",
    "IsGroundAligned",
    "IsEmitAboveGroundOnly",
    "IsParticleUpTowardsEmitter",
    "UseMaximumHeight",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let mut data = BinaryData {
        payload: vec![0; 0xb0],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    let mut attributes = vec![
        "id",
        "Priority",
        "Shader",
        "Type",
        "ParticleTexture",
        "Drawable",
        "SlaveSystem",
        "PerParticleAttachedSystem",
        "SystemLifetime",
        "SortLevel",
        "EmitterSound",
    ];
    attributes.extend(ROOT_FLAGS);
    validate_attributes(node, &attributes, path)?;

    for (index, (name, default)) in [
        ("Priority", "ULTRA_HIGH_ONLY"),
        ("Shader", "ADDITIVE"),
        ("Type", "PARTICLE"),
    ]
    .into_iter()
    .enumerate()
    {
        data.word(
            4 + index * 4,
            enum_index(
                node.attribute(name).unwrap_or(default),
                &enums()[index],
                name,
                path,
            )?,
        );
    }
    data.word(0x20, unsigned(node, "SystemLifetime", 0, path)?);
    data.word(0x24, unsigned(node, "SortLevel", 0, path)?);
    for (index, name) in ROOT_FLAGS.into_iter().enumerate() {
        data.payload[0xa8 + index] = boolean(node, name, false, path)?;
    }
    for attribute in node
        .attributes()
        .filter(|attr| !attr.value().trim().is_empty())
    {
        let value = attribute.value();
        match attribute.name() {
            "ParticleTexture" => write_import(value, "Texture", 0x10, &mut data, &mut out)?,
            "Drawable" => write_import(value, "BaseRenderAssetType", 0x14, &mut data, &mut out)?,
            "SlaveSystem" | "PerParticleAttachedSystem" => {
                let offset = if attribute.name() == "SlaveSystem" {
                    0x18
                } else {
                    0x1c
                };
                let reference = asset_reference(value, "FXParticleSystemTemplate", path)?;
                data.word(offset, reference.instance_id);
                out.reference_types.push(reference.type_id);
            }
            "EmitterSound" => {
                let offset = data.allocate(0x28, 4)?;
                write_import(value, "BaseAudioEventInfo", offset, &mut data, &mut out)?;
            }
            _ => {}
        }
    }

    let mut children = significant_children(node).peekable();
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "SlavePosOffset"))) {
        vector(child, 0x2c, &mut data)?;
    }
    for (index, name) in [
        "Lifetime",
        "Size",
        "StartSizeRate",
        "BurstDelay",
        "BurstCount",
        "InitialDelay",
    ]
    .into_iter()
    .enumerate()
    {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            random_variable(child, 0x38 + index * 12, &mut data)?;
        }
    }
    for (name, encode) in [
        ("Alphas", alphas as fn(_, &mut BinaryData<'_>) -> _),
        ("Colors", colors),
        ("Wind", wind),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            encode(child, &mut data)?;
        }
    }
    encode_modules(children, &mut data, &mut out)?;

    for offsets in [&mut data.relocations, &mut out.imports] {
        if !offsets.is_empty() {
            offsets.sort_unstable();
            offsets.push(u32::MAX);
        }
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}

fn enums() -> &'static [Vec<String>; 4] {
    static ENUMS: OnceLock<[Vec<String>; 4]> = OnceLock::new();
    ENUMS.get_or_init(|| {
        schema_enums(
            include_str!("../../../schemas/ra3/xsd/FXParticleSystem/FXParticleBase.xsd"),
            ENUM_NAMES,
        )
    })
}

fn alphas(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<()> {
    validate_attributes(node, &[], data.path)?;
    let frames: Vec<_> = significant_children(node).collect();
    if frames.len() > 8 {
        return Err(invalid(data.path, "too many particle alpha keyframes"));
    }
    let header = data.allocate(0x80, 8)?;
    let start = data.list(header, frames.len(), 20)?;
    for (index, frame) in frames.into_iter().enumerate() {
        if !frame.has_tag_name((SAGE_NS, "Alpha")) {
            return Err(invalid(data.path, "unexpected particle Alphas child"));
        }
        validate_leaf(
            frame,
            &["Type", "Low", "High", "RelativeAge", "Frame"],
            data.path,
        )?;
        let offset = start + index * 20;
        random_values(frame, offset, data)?;
        keyframe(frame, offset + 12, data)?;
    }
    Ok(())
}

fn colors(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<()> {
    validate_attributes(node, &["UseHouseColor", "HouseColorSaturation"], data.path)?;
    let header = data.allocate(0x84, 0x1c)?;
    real(node, "HouseColorSaturation", "1", header, data)?;
    data.payload[header + 0x18] = boolean(node, "UseHouseColor", false, data.path)?;
    let mut children = significant_children(node).peekable();
    let mut frames = Vec::new();
    while let Some(frame) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Color"))) {
        frames.push(frame);
    }
    if frames.len() > 8 {
        return Err(invalid(data.path, "too many particle color keyframes"));
    }
    let start = data.list(header + 4, frames.len(), 20)?;
    for (index, frame) in frames.into_iter().enumerate() {
        let offset = start + index * 20;
        validate_attributes(frame, &["RelativeAge", "Frame"], data.path)?;
        keyframe(frame, offset, data)?;
        let mut channels = significant_children(frame);
        let color = channels
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, "Color")))
            .filter(|_| channels.next().is_none())
            .ok_or_else(|| invalid(data.path, "particle color keyframe requires one Color"))?;
        validate_leaf(color, &["r", "g", "b"], data.path)?;
        for (index, name) in ["r", "g", "b"].into_iter().enumerate() {
            real(color, name, "0", offset + 8 + index * 4, data)?;
        }
    }
    if let Some(scale) = children.next_if(|child| child.has_tag_name((SAGE_NS, "ColorScale"))) {
        random_variable(scale, header + 12, data)?;
    }
    if children.next().is_some() {
        return Err(invalid(data.path, "unexpected particle Colors child"));
    }
    Ok(())
}

fn wind(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<()> {
    let fields = [
        ("Strength", "2"),
        ("FullStrengthDist", "75"),
        ("ZeroStrengthDist", "200"),
        ("AngleChangeMin", ".15"),
        ("AngleChangeMax", ".45"),
        ("PingPongStartAngleMin", "0"),
        ("PingPongStartAngleMax", "0.7853981633974483"),
        ("PingPongEndAngleMin", "5.4977871437821381"),
        ("PingPongEndAngleMax", "6.2831853071795864"),
        ("TurbulenceAmplitude", "0"),
        ("TurbulenceFrequency", "0"),
    ];
    let mut attributes = vec!["Motion"];
    attributes.extend(fields.iter().map(|&(name, _)| name));
    validate_leaf(node, &attributes, data.path)?;
    let start = data.allocate(0x88, 0x30)?;
    data.word(
        start,
        enum_index(
            node.attribute("Motion").unwrap_or("NOT_USED"),
            &enums()[3],
            "Motion",
            data.path,
        )?,
    );
    for (index, (name, default)) in fields.into_iter().enumerate() {
        real(node, name, default, start + 4 + index * 4, data)?;
    }
    Ok(())
}

fn keyframe(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    data.word(
        start,
        percentage(
            node.attribute("RelativeAge").unwrap_or("0"),
            "RelativeAge",
            data.path,
        )?
        .to_bits(),
    );
    data.word(start + 4, unsigned(node, "Frame", 1000, data.path)?);
    Ok(())
}

fn real(
    node: Node<'_, '_>,
    name: &str,
    default: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    data.word(
        offset,
        finite_number(node.attribute(name).unwrap_or(default), name, data.path)?.to_bits(),
    );
    Ok(())
}

fn unsigned(node: Node<'_, '_>, name: &str, default: u32, path: &Path) -> io::Result<u32> {
    node.attribute(name).map_or(Ok(default), |value| {
        value
            .trim()
            .parse()
            .map_err(|_| invalid(path, format!("invalid {name} {value:?}")))
    })
}

fn vector(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    super::fx_list::vector(node, false, start, data)
}

fn encode_modules<'a, 'input: 'a>(
    mut children: std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    for (name, pointer) in [
        ("Physics", 0x8c),
        ("Draw", 0x90),
        ("Volume", 0x94),
        ("Velocity", 0x98),
        ("Event", 0xa0),
        ("Update", 0xa4),
    ] {
        let Some(wrapper) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) else {
            if matches!(name, "Event" | "Update") {
                continue;
            }
            return Err(invalid(data.path, format!("missing particle {name}")));
        };
        validate_attributes(wrapper, &[], data.path)?;
        let mut variants = significant_children(wrapper);
        if name == "Event" {
            let events: Vec<_> = variants.collect();
            let start = data.list(pointer - 4, events.len(), 4)?;
            for (index, event) in events.into_iter().enumerate() {
                effects::event(event, start + index * 4, data, out)?;
            }
            continue;
        }
        let Some(variant) = variants.next() else {
            continue;
        };
        if variants.next().is_some() {
            return Err(invalid(
                data.path,
                format!("multiple particle {name} modules"),
            ));
        }
        match name {
            "Physics" => emission::physics(variant, pointer, data, out)?,
            "Volume" => emission::volume(variant, pointer, data, out)?,
            "Velocity" => emission::velocity(variant, pointer, data, out)?,
            "Draw" => effects::draw(variant, pointer, data, out)?,
            "Update" => effects::update(variant, pointer, data, out)?,
            _ => unreachable!(),
        }
    }
    if children.next().is_some() {
        return Err(invalid(
            data.path,
            "unexpected FXParticleSystemTemplate child",
        ));
    }
    Ok(())
}
