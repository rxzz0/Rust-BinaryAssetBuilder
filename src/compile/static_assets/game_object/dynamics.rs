use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, percentage, significant_children, validate_attributes};
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

fn encode_volume(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = VOLUME_REALS
        .map(|(name, _)| name)
        .into_iter()
        .chain(["ReportContacts", "ContactTag"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x4c)?;
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
