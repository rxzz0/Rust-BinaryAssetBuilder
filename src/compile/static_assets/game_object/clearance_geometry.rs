use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, significant_children, validate_attributes};
use super::behaviors_basic::optional_vector;
use super::children::encode_geometry_vector;
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    const DIMENSIONS: [&str; 4] = [
        "BoundingCircleRadius",
        "BoundingSphereRadius",
        "XWidth",
        "YDepth",
    ];
    let path = data.path;
    let allowed = [
        "IsSmall",
        DIMENSIONS[0],
        DIMENSIONS[1],
        DIMENSIONS[2],
        DIMENSIONS[3],
    ];
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x30)?;
    data.payload[start + 0x2c] = boolean(node, "IsSmall", false, path)?;
    let mut entries = vec![Entry::named(
        "IsSmall",
        0x77e3_f4f7,
        Target::Data(start + 0x2c),
    )];
    for (index, name) in DIMENSIONS.into_iter().enumerate() {
        let offset = start + index * 4;
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(offset, value.to_bits());
        entries.push(Entry::named(name, 0x6458_64f3, Target::Data(offset)));
    }
    let mut children = significant_children(node).peekable();
    let mut shapes = Vec::new();
    while let Some(shape) = children.next_if(|child| child.has_tag_name((SAGE_NS, "Shapes"))) {
        shapes.push(shape);
    }
    if shapes.len() > 128 {
        return Err(invalid(path, "ClearanceGeometry allows up to 128 Shapes"));
    }
    let count = start + 0x10;
    let first = data.list(count, shapes.len(), 0x20)?;
    let items = shapes
        .into_iter()
        .enumerate()
        .map(|(index, shape)| {
            encode_shape(shape, first + index * 0x20, data).map(|table| (0, Target::Table(table)))
        })
        .collect::<io::Result<Vec<_>>>()?;
    entries.push(Entry::named(
        "Shapes",
        0x9ce4_80a2,
        Target::Table(Table::array(count, items)),
    ));

    let anchor = if let Some(anchor) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "RotationAnchorOffset")))
    {
        let offset = data.allocate(start + 0x18, 8)?;
        encode_geometry_vector(anchor, offset, &["x", "y"], data)?;
        Target::Data(offset)
    } else {
        Target::Null
    };
    entries.push(Entry::named("RotationAnchorOffset", 0x4ae3_1f24, anchor));
    for (name, field) in [
        ("Center", 0x1c),
        ("HighestContactPoint", 0x20),
        ("InnermostContactPoint", 0x24),
    ] {
        let child = children.next_if(|child| child.has_tag_name((SAGE_NS, name)));
        let target = optional_vector(child, start + field, data)?;
        entries.push(Entry::named(name, 0x2936_8077, target));
    }
    let contact = if let Some(contact) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ContactPoints")))
    {
        let offset = data.allocate(start + 0x28, 0x14)?;
        Target::Table(super::children::encode_geometry_contact(
            contact, offset, data,
        )?)
    } else {
        Target::Null
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ClearanceGeometry child"));
    }
    entries.push(Entry::named("ContactPoints", 0x6b8e_e8ef, contact));
    Ok(Table::record(entries))
}

fn encode_shape(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "Type",
            "Height",
            "MajorRadius",
            "MinorRadius",
            "Name",
            "BActive",
        ],
        path,
    )?;
    if let Some(value) = node.attribute("Type") {
        let kind = super::super::global_ui::enum_value(
            value,
            &["SPHERE", "BOX", "CYLINDER", "RING"],
            path,
        )?;
        data.word(start, kind);
    }
    for (name, offset) in [("Height", 4), ("MajorRadius", 8), ("MinorRadius", 0xc)] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    if let Some(name) = node.attribute("Name") {
        data.string(start + 0x10, name)?;
    }
    data.payload[start + 0x1c] = boolean(node, "BActive", false, path)?;
    let mut children = significant_children(node).peekable();
    let offset = children.next_if(|child| child.has_tag_name((SAGE_NS, "Offset")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ClearanceGeometry Shapes child"));
    }
    let offset = optional_vector(offset, start + 0x18, data)?;
    let mut entries: Vec<_> = fields(
        start,
        &[
            ("Type", 0xb0d4_9164, 0),
            ("Height", 0x6458_64f3, 4),
            ("MajorRadius", 0x6458_64f3, 8),
            ("MinorRadius", 0x6458_64f3, 0xc),
            ("Name", 0x1a29_33aa, 0x10),
            ("BActive", 0x77e3_f4f7, 0x1c),
        ],
    )
    .collect();
    entries.push(Entry::named("Offset", 0x2936_8077, offset));
    Ok(Table::record(entries))
}
