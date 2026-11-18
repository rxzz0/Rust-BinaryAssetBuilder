use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    finite_number, percentage, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::boolean;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const CONTACT: u32 = 0x92e3_42e4;
const ROOT_FIELDS: [(&str, u32, &str, &str); 5] = [
    ("Motion", 0xa39a_fb20, "DynamicsMotionType", "TRACKED"),
    (
        "Collision",
        0x496e_121d,
        "DynamicsCollisionType",
        "NONINTERCOLLIDING",
    ),
    (
        "IntuitiveCollision",
        0xce2c_8de9,
        "IntuitiveCollisionType",
        "COLLIDES_WITH_GROUND_ONLY",
    ),
    ("ContactTag", CONTACT, "DynamicsContactTag", "NONE"),
    (
        "DrawPriority",
        0x4173_7ffb,
        "DynamicsDrawPriority",
        "REMOVABLE",
    ),
];
const VOLUME_REALS: [(&str, &str); 7] = [
    ("Mass", "0"),
    ("Bounciness", "30%"),
    ("Spinniness", "2"),
    ("StaticFriction", ".6"),
    ("KineticFriction", ".5"),
    ("AngularDrag", ".1"),
    ("LinearDrag", ".1"),
];
const SHAPES: [(&str, u32, &[&str]); 5] = [
    ("Sphere", 0x9800_e795, &["Radius"]),
    ("Capsule", 0xf7f5_8ca7, &["Radius", "HalfLength"]),
    (
        "Box",
        0x520b_5f30,
        &["HalfSizeX", "HalfSizeY", "HalfSizeZ", "EdgeRadius"],
    ),
    (
        "Cylinder",
        0x3804_cb6a,
        &["Radius", "HalfLength", "EdgeRadius"],
    ),
    (
        "Triangle",
        0x1b53_14e2,
        &[
            "X1",
            "Y1",
            "Z1",
            "X2",
            "Y2",
            "Z2",
            "X3",
            "Y3",
            "Z3",
            "EdgeRadius",
        ],
    ),
];
const REAL: u32 = 0x6458_64f3;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x155a_87bb;
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(ROOT_FIELDS.map(|(name, _, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let volume = children.next();
    if volume.is_some_and(|child| !child.has_tag_name((SAGE_NS, "Volume")))
        || children.next().is_some()
    {
        return Err(invalid(path, "unexpected DynamicsUpdate child"));
    }
    let start = data.allocate(pointer, 0x20)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (index, (name, token, kind, default)) in ROOT_FIELDS.into_iter().enumerate() {
        let offset = start + 8 + 4 * index;
        data.word(offset, enum_index(node, name, kind, default, data)?);
        entries.push(Entry::named(name, token, Target::Data(offset)));
    }
    let volume = match volume {
        Some(node) => Target::Table(encode_volume(node, start + 0x1c, data)?),
        None => Target::Null,
    };
    entries.push(Entry::named("Volume", 0x9684_2fa5, volume));
    Ok((KIND, Table::record(entries)))
}

pub(super) const DRAW_KIND: u32 = 0x9be6_adbb;
pub(super) const DRAW_SIZE: usize = 0xfc;
pub(super) const DRAW_ATTRIBUTES: [&str; 7] = [
    "Collision",
    "IntuitiveCollision",
    "DrawPriority",
    "LODPriority",
    "Explodiness",
    "FlingPerturbation",
    "InitiallyActive",
];

pub(super) fn draw_fields(
    node: Node<'_, '_>,
    start: usize,
    bone_volumes: Option<Node<'_, '_>>,
    lifetime: Option<Node<'_, '_>>,
    data: &mut BinaryData<'_>,
    bone_name_relocations: &mut Vec<u32>,
) -> io::Result<Vec<Entry>> {
    let path = data.path;
    let mut entries = Vec::with_capacity(9);
    for (index, (name, token, kind, default)) in [
        ROOT_FIELDS[1],
        ROOT_FIELDS[2],
        (
            "DrawPriority",
            0x4173_7ffb,
            "DynamicsDrawPriority",
            "MUST_BE_DRAWN",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = start + 0xd8 + index * 4;
        data.word(offset, enum_index(node, name, kind, default, data)?);
        entries.push(Entry::named(name, token, Target::Data(offset)));
    }
    let lod = super::super::global_ui::enum_value(
        node.attribute("LODPriority").unwrap_or("VERY_LOW"),
        &["VERY_LOW", "LOW", "MEDIUM", "HIGH", "ULTRA_HIGH"],
        path,
    )?;
    data.word(start + 0xe4, lod);
    entries.push(Entry::named(
        "LODPriority",
        0xf3dc_9d72,
        Target::Data(start + 0xe4),
    ));
    for (name, offset) in [("Explodiness", 0xe8), ("FlingPerturbation", 0xec)] {
        let field = start + offset;
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(field, value.to_bits());
        entries.push(Entry::named(name, REAL, Target::Data(field)));
    }
    data.payload[start + 0xf8] = boolean(node, "InitiallyActive", true, path)?;
    entries.push(Entry::named(
        "InitiallyActive",
        0x77e3_f4f7,
        Target::Data(start + 0xf8),
    ));
    let volumes = bone_volumes
        .map(|node| encode_bone_volumes(node, start + 0xf0, data, bone_name_relocations))
        .transpose()?;
    entries.push(Entry::named(
        "BoneVolumes",
        0x2911_8235,
        volumes.map_or(Target::Null, Target::Table),
    ));
    let lifetime = if let Some(node) = lifetime {
        validate_attributes(node, &["Delay", "FadeTime"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected DynamicsDraw Lifetime child"));
        }
        let start = data.allocate(start + 0xf4, 8)?;
        let mut fields = Vec::with_capacity(2);
        for (index, name) in ["Delay", "FadeTime"].into_iter().enumerate() {
            let offset = start + index * 4;
            let value = sage_time(node.attribute(name).unwrap_or("0s"), name, path)?;
            data.word(offset, value.to_bits());
            fields.push(Entry::named(name, 0x9986_69e8, Target::Data(offset)));
        }
        Target::Table(Table::record(fields))
    } else {
        Target::Null
    };
    entries.push(Entry::named("Lifetime", 0x29ef_3b8a, lifetime));
    Ok(entries)
}

fn encode_bone_volumes(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    bone_name_relocations: &mut Vec<u32>,
) -> io::Result<Table> {
    const VOLUME_SIZE: usize = 0x54;
    const BONE_NAME: usize = 0x4c;
    let path = data.path;
    validate_attributes(node, &[], path)?;
    let count = data.allocate(pointer, 8)?;
    let children: Vec<_> = significant_children(node).collect();
    let start = data.list(count, children.len(), VOLUME_SIZE)?;
    let mut volumes = Vec::with_capacity(children.len());
    for (index, node) in children.into_iter().enumerate() {
        if !node.has_tag_name((SAGE_NS, "BoneVolume")) {
            return Err(invalid(path, "unexpected BoneVolumes child"));
        }
        let offset = start + index * VOLUME_SIZE;
        let bone_name = offset + BONE_NAME;
        if let Some(value) = node.attribute("BoneName") {
            data.string(bone_name, value)?;
            // The derived string pointer precedes the inherited shape pointers.
            bone_name_relocations.push((bone_name + 4) as u32);
        }
        let volume = encode_volume_at(node, offset, data, Some("BoneName"))?;
        let table = Table::record(vec![
            base(0x9684_2fa5, volume),
            Entry::named("BoneName", 0x1a29_33aa, Target::Data(bone_name)),
        ]);
        volumes.push((0, Target::Table(table)));
    }
    Ok(Table::record(vec![Entry::named(
        "BoneVolume",
        0xf8d0_f3e6,
        Target::Table(Table::array(count, volumes)),
    )]))
}

fn encode_volume(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let start = data.allocate(pointer, 0x4c)?;
    encode_volume_at(node, start, data, None)
}

fn encode_volume_at(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    extra_attribute: Option<&str>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = VOLUME_REALS
        .map(|(name, _)| name)
        .into_iter()
        .chain(["ReportContacts", "ContactTag"])
        .chain(extra_attribute)
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut entries = Vec::new();
    for (index, (name, default)) in VOLUME_REALS.into_iter().enumerate() {
        let value = node.attribute(name).unwrap_or(default);
        let number = if name == "Bounciness" {
            percentage(value, name, path)?
        } else {
            finite_number(value, name, path)?
        };
        let field = start + 4 * index;
        data.word(field, number.to_bits());
        entries.push(Entry::named(name, REAL, Target::Data(field)));
    }
    data.payload[start + 0x48] = boolean(node, "ReportContacts", false, path)?;
    data.word(
        start + 0x1c,
        enum_index(node, "ContactTag", "DynamicsContactTag", "NONE", data)?,
    );
    entries.extend([
        Entry::named("ReportContacts", 0x77e3_f4f7, Target::Data(start + 0x48)),
        Entry::named("ContactTag", CONTACT, Target::Data(start + 0x1c)),
    ]);
    let mut groups: [Vec<_>; 5] = Default::default();
    for child in significant_children(node) {
        let index = SHAPES
            .iter()
            .position(|(name, _, _)| child.has_tag_name((SAGE_NS, *name)))
            .ok_or_else(|| invalid(path, "unknown DynamicsUpdate volume shape"))?;
        groups[index].push(child);
    }
    for (index, ((name, kind, attributes), group)) in SHAPES.into_iter().zip(groups).enumerate() {
        let count = start + 0x20 + index * 8;
        let size = 8 + attributes.len() * 4;
        let first = data.list(count, group.len(), size)?;
        let mut items = Vec::with_capacity(group.len());
        for (index, shape) in group.into_iter().enumerate() {
            let target = encode_shape(shape, first + index * size, attributes, data)?;
            items.push((0, Target::Table(target)));
        }
        entries.push(Entry::named(
            name,
            kind,
            Target::Table(Table::array(count, items)),
        ));
    }
    Ok(Table::record(entries))
}

fn encode_shape(
    node: Node<'_, '_>,
    start: usize,
    attributes: &[&str],
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(node, attributes, path)?;
    let mut children = significant_children(node).peekable();
    let mut transforms = Vec::new();
    for (name, kind, offset, axes) in [
        ("Translation", 0x2936_8077, 0, &["x", "y", "z"][..]),
        ("Rotation", 0x8549_85c7, 4, &["x", "y", "z", "w"][..]),
    ] {
        let target =
            if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                validate_attributes(child, axes, path)?;
                if has_content(child) {
                    return Err(invalid(path, format!("unexpected {name} content")));
                }
                let offset = data.allocate(start + offset, 4 * axes.len())?;
                for (index, &axis) in axes.iter().enumerate() {
                    let value = finite_number(child.attribute(axis).unwrap_or("0"), axis, path)?;
                    data.word(offset + index * 4, value.to_bits());
                }
                Target::Data(offset)
            } else {
                Target::Null
            };
        transforms.push(Entry::named(name, kind, target));
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected dynamics shape child"));
    }
    let mut entries = vec![base(0x3a44_308a, Table::record(transforms))];
    for (index, &name) in attributes.iter().enumerate() {
        let field = start + 8 + index * 4;
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(field, value.to_bits());
        entries.push(Entry::named(name, REAL, Target::Data(field)));
    }
    Ok(Table::record(entries))
}

fn enum_index(
    node: Node<'_, '_>,
    name: &str,
    kind: &str,
    default: &str,
    data: &BinaryData<'_>,
) -> io::Result<u32> {
    static VALUES: OnceLock<[Vec<String>; 5]> = OnceLock::new();
    let values = VALUES.get_or_init(|| {
        ROOT_FIELDS.map(|(_, _, kind, _)| {
            super::ai_update::enum_values(
                include_str!("../../../../schemas/ra3/xsd/Includes/Dynamics.xsd"),
                kind,
            )
        })
    });
    let index = ROOT_FIELDS
        .iter()
        .position(|(_, _, field, _)| *field == kind)
        .unwrap();
    let value = node.attribute(name).unwrap_or(default);
    values[index]
        .iter()
        .position(|name| name == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(data.path, format!("invalid {name} {value}")))
}
