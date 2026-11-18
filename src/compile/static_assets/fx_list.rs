mod variants;

use super::binary::BinaryData;
use super::game_object::{audio_reference, optional_vector, write_model_conditions};
use super::global_ui::{enum_value, filter_reference_types, object_filter};
use super::{
    Marshaled, asset_reference, finite_number, sage_angle, sage_time, significant_children,
    text_value, validate_attributes,
};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::{io, path::Path};

const CONDITIONS: &[&str] = &[
    "RequiredSecondaryModelConditions",
    "ExcludedSecondaryModelConditions",
    "RequiredSourceModelConditions",
    "ExcludedSourceModelConditions",
];
const BASE_FLAGS: &[&str] = &["StopIfPlayed", "OnlyIfOnLand", "PlayIfSourceIsStealthed"];
const PARTICLE_FLAGS: &[&str] = &[
    "OrientToObject",
    "Ricochet",
    "CreateAtGroundHeight",
    "UseTarget",
    "SetTargetMatrix",
    "OnlyIfOnWater",
];
const PARTICLE_ATTRIBUTES: &[&str] = &[
    "Particle",
    "Count",
    "CreateOverrideBone",
    "TargetOverrideBone",
    "TargetCoeff",
    "SystemLife",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let root_flags = ["PlayEvenIfShrouded", "Tailorable"];
    validate_attributes(
        node,
        &[
            "id",
            "PlayEvenIfShrouded",
            "Tailorable",
            "CullTracking",
            "CullTrackingMin",
            "CullTrackingMax",
        ],
        path,
    )?;
    let mut data = BinaryData {
        payload: vec![0; 0x1c],
        relocations: Vec::new(),
        path,
    };
    let mut out = Marshaled::default();
    data.word(
        4,
        sage_time(
            node.attribute("CullTracking").unwrap_or("0s"),
            "CullTracking",
            path,
        )?
        .to_bits(),
    );
    data.word(8, integer(node, "CullTrackingMin", 0, path)?);
    data.word(12, integer(node, "CullTrackingMax", 0, path)?);
    write_flags(node, &root_flags, 0x18, &mut data)?;
    let mut children = significant_children(node);
    if let Some(list) = children.next() {
        if !list.has_tag_name((SAGE_NS, "NuggetList")) || children.next().is_some() {
            return Err(invalid(path, "unexpected FXList child"));
        }
        validate_attributes(list, &[], path)?;
        let nuggets: Vec<_> = significant_children(list).collect();
        let first = data.list(0x10, nuggets.len(), 4)?;
        let mut offsets = Vec::with_capacity(nuggets.len());
        for (index, nugget) in nuggets.into_iter().enumerate() {
            let pointer = first + index * 4;
            let reloc = data.relocations.len();
            encode_nugget(nugget, pointer, &mut data, &mut out)?;
            offsets.push(data.relocations.remove(reloc));
        }
        if !offsets.is_empty() {
            data.relocations.splice(1..1, offsets);
        }
    }
    for offsets in [&mut data.relocations, &mut out.imports] {
        if !offsets.is_empty() {
            offsets.push(u32::MAX);
        }
    }
    out.payload = data.payload;
    out.relocations = data.relocations;
    Ok(out)
}

fn encode_nugget(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let tag = node.tag_name().name();
    if node.tag_name().namespace() != Some(SAGE_NS) {
        return Err(invalid(data.path, "unexpected FXList nugget namespace"));
    }
    let variant = variants::layout(tag);
    let (kind, size, fields, flags) = match tag {
        "ParticleSystem" => (0x2ccc_1572, 0x94, PARTICLE_ATTRIBUTES, PARTICLE_FLAGS),
        "Sound" => (0x1e08_c8fb, 0x30, &["Value"][..], &[][..]),
        "RayEffect" => (0x8e39_bc26, 0x48, &["Thing"][..], &[][..]),
        "EvaEvent" => (
            0x907f_b364,
            0x38,
            &["EvaEventOwner", "EvaEventAlly", "EvaEventEnemy"][..],
            &[][..],
        ),
        "ForceFeedback" => (0x260d_1436, 0x30, &["Event"][..], &[][..]),
        _ => {
            let layout = variant
                .as_ref()
                .ok_or_else(|| invalid(data.path, format!("unsupported FXList nugget {tag}")))?;
            (layout.kind, layout.size, &[][..], &[][..])
        }
    };
    let mut allowed = Vec::from(CONDITIONS);
    allowed.extend_from_slice(BASE_FLAGS);
    allowed.push("Weather");
    allowed.extend_from_slice(fields);
    allowed.extend_from_slice(flags);
    allowed.extend(variant.iter().flat_map(variants::Layout::attributes));
    validate_attributes(node, &allowed, data.path)?;
    let start = data.allocate(pointer, size)?;
    data.word(start, kind);

    // EA allocates derived data before the inherited filters and conditions.
    let mut children = significant_children(node).peekable();
    let base_children: Vec<_> = std::iter::from_fn(|| {
        children.next_if(|child| {
            child.tag_name().namespace() == Some(SAGE_NS)
                && matches!(
                    child.tag_name().name(),
                    "SecondaryObjectFilter"
                        | "SourceObjectFilter"
                        | "SourceMustNotHaveBeenDisabledThisFrameByType"
                )
        })
    })
    .collect();
    match tag {
        "ParticleSystem" => particle(node, start, &mut children, data, out)?,
        "Sound" => {
            let index = audio_reference(
                node.attribute("Value").unwrap_or(""),
                data.path,
                &mut out.references,
                &mut out.reference_types,
            )?;
            if index != 0 {
                data.word(start + 0x2c, index);
                out.imports.push((start + 0x2c) as u32);
            }
        }
        "RayEffect" => {
            if let Some(value) = node
                .attribute("Thing")
                .filter(|value| !value.trim().is_empty())
            {
                let reference = asset_reference(value, "GameObject", data.path)?;
                data.word(start + 0x2c, reference.instance_id);
                out.reference_types.push(reference.type_id);
            }
            for (name, offset) in [("PrimaryOffset", 0x30), ("SecondaryOffset", 0x3c)] {
                if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                    vector(child, false, start + offset, data)?;
                }
            }
        }
        "EvaEvent" => {
            for (index, name) in fields.iter().enumerate() {
                import(
                    node.attribute(*name),
                    "EvaEvent",
                    start + 0x2c + index * 4,
                    data,
                    out,
                )?;
            }
        }
        "ForceFeedback" => import(
            node.attribute("Event"),
            "ForceFeedbackEvent",
            start + 0x2c,
            data,
            out,
        )?,
        _ => variant
            .unwrap()
            .encode(node, start, &mut children, data, out)?,
    }
    if let Some(child) = children.next() {
        return Err(invalid(
            data.path,
            format!("unexpected {tag} child {}", child.tag_name().name()),
        ));
    }
    encode_base(node, start, &base_children, data, out)
}

fn encode_base(
    node: Node<'_, '_>,
    start: usize,
    children: &[Node<'_, '_>],
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    let options = [
        "NORMAL",
        "CLOUDY",
        "RAINY",
        "CLOUDYRAINY",
        "SUNNY",
        "SNOWY",
        "INVALID",
    ];
    data.word(
        start + 0x14,
        enum_value(
            node.attribute("Weather").unwrap_or("INVALID"),
            &options,
            path,
        )?,
    );
    write_flags(node, BASE_FLAGS, start + 0x28, data)?;
    let reloc = data.relocations.len();
    for (&name, offset) in CONDITIONS.iter().zip([4, 8, 0xc, 0x10]) {
        if let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
        {
            let target = data.allocate(start + offset, 0x3c)?;
            write_model_conditions(value, target, data)?;
        }
    }
    let mut children = children.iter().copied().peekable();
    for (name, offset) in [
        ("SecondaryObjectFilter", 0x18),
        ("SourceObjectFilter", 0x1c),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            let target = data.allocate(start + offset, 0x70)?;
            object_filter(child, target, data)?;
            out.reference_types.extend(filter_reference_types(child));
        }
    }
    let mut disabled = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "SourceMustNotHaveBeenDisabledThisFrameByType")) {
            return Err(invalid(path, "unexpected FXNugget filter"));
        }
        disabled.push(enum_value(
            text_value(child, path)?,
            &[
                "DEFAULT",
                "USER_PARALYZED",
                "EMP",
                "HELD",
                "PARALYZED",
                "UNMANNED",
                "UNDERPOWERED",
                "FREEFALL",
                "TEMPORARILY_BUSY",
                "SCRIPT_DISABLED",
                "SCRIPT_UNDERPOWERED",
                "INFILTRATED",
            ],
            path,
        )?);
    }
    let first = data.list(start + 0x20, disabled.len(), 4)?;
    for (index, value) in disabled.into_iter().enumerate() {
        data.word(first + index * 4, value);
    }
    data.relocations[reloc..].sort_unstable();
    Ok(())
}

fn particle<'a, 'input: 'a>(
    node: Node<'a, 'input>,
    start: usize,
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    let path = data.path;
    import(
        node.attribute("Particle"),
        "FXParticleSystemTemplate",
        start + 0x2c,
        data,
        out,
    )?;
    data.word(start + 0x30, integer(node, "Count", 1, path)?);
    for (name, offset) in [("CreateOverrideBone", 0x34), ("TargetOverrideBone", 0x3c)] {
        if let Some(value) = node.attribute(name).filter(|value| !value.is_empty()) {
            data.string(start + offset, value)?;
        }
    }
    data.word(
        start + 0x44,
        finite_number(
            node.attribute("TargetCoeff").unwrap_or("1"),
            "TargetCoeff",
            path,
        )?
        .to_bits(),
    );
    data.word(
        start + 0x48,
        sage_time(
            node.attribute("SystemLife").unwrap_or("-1s"),
            "SystemLife",
            path,
        )?
        .to_bits(),
    );
    for (name, offset) in [("Rotate", 0x4c), ("Offset", 0x58)] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            vector(child, name == "Rotate", start + offset, data)?;
        }
    }
    optional_vector(
        children.next_if(|child| child.has_tag_name((SAGE_NS, "TargetOffset"))),
        start + 0x64,
        data,
    )?;
    for (name, offset, default) in [
        ("Radius", 0x68, 0f32),
        ("Height", 0x74, 0f32),
        ("InitialDelay", 0x80, -1f32),
    ] {
        if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            random_variable(child, start + offset, data)?;
        } else {
            data.word(start + offset + 4, default.to_bits());
            data.word(start + offset + 8, default.to_bits());
        }
    }
    write_flags(node, PARTICLE_FLAGS, start + 0x8c, data)
}

pub(super) fn vector(
    node: Node<'_, '_>,
    angles: bool,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let axes = if angles {
        ["X", "Y", "Z"]
    } else {
        ["x", "y", "z"]
    };
    validate_leaf(node, &axes, data.path)?;
    for (index, name) in axes.into_iter().enumerate() {
        let raw = node
            .attribute(name)
            .unwrap_or(if angles { "0d" } else { "0" });
        let value = if angles {
            sage_angle(raw, name, data.path)?
        } else {
            finite_number(raw, name, data.path)?
        };
        data.word(start + index * 4, value.to_bits());
    }
    Ok(())
}

pub(super) fn random_variable(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let path = data.path;
    validate_leaf(node, &["Type", "Low", "High"], path)?;
    random_values(node, start, data)
}

pub(super) fn random_values(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let path = data.path;
    let distribution = enum_value(
        node.attribute("Type").unwrap_or("UNIFORM"),
        &[
            "CONSTANT",
            "UNIFORM",
            "GAUSSIAN",
            "TRIANGULAR",
            "LOW_BIAS",
            "HIGH_BIAS",
        ],
        path,
    )?;
    data.word(start, distribution);
    for (name, offset) in [("Low", 4), ("High", 8)] {
        data.word(
            start + offset,
            finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
        );
    }
    Ok(())
}

pub(super) fn validate_leaf(node: Node<'_, '_>, fields: &[&str], path: &Path) -> io::Result<()> {
    validate_attributes(node, fields, path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected {} content", node.tag_name().name()),
        ));
    }
    Ok(())
}

fn integer(node: Node<'_, '_>, name: &str, default: i32, path: &Path) -> io::Result<u32> {
    node.attribute(name)
        .map_or(Ok(default), |value| {
            value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))
        })
        .map(|value| value as u32)
}

fn write_flags(
    node: Node<'_, '_>,
    names: &[&str],
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    for (index, &name) in names.iter().enumerate() {
        data.payload[start + index] = boolean(node, name, false, data.path)?;
    }
    Ok(())
}

pub(super) fn boolean(
    node: Node<'_, '_>,
    name: &str,
    default: bool,
    path: &Path,
) -> io::Result<u8> {
    match node.attribute(name).map(str::trim) {
        None => Ok(u8::from(default)),
        Some("true" | "True" | "1") => Ok(1),
        Some("false" | "False" | "0") => Ok(0),
        Some(value) => Err(invalid(path, format!("invalid {name} {value:?}"))),
    }
}

fn import(
    raw: Option<&str>,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    out: &mut Marshaled,
) -> io::Result<()> {
    if let Some(value) = raw.filter(|value| !value.trim().is_empty()) {
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let reference = asset_reference(value, kind, data.path)?;
        out.references.push(reference.into());
        out.reference_types.push(reference.type_id);
        out.imports.push(offset as u32);
        data.word(offset, out.references.len() as u32);
    }
    Ok(())
}
