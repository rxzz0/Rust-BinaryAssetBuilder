use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, finite_number, significant_children, validate_attributes};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x1e5c_747a;
const FIELDS: [(&str, u32, usize); 7] = [
    ("StartDistanceOffset", 0x6458_64f3, 8),
    ("Radius", 0x6458_64f3, 0xc),
    ("Length", 0x6458_64f3, 0x10),
    ("SweepFXList", 0x0d1c_8e83, 0x14),
    ("SweepWeapon", 0xd046_dd22, 0x18),
    ("Speed", 0x7a76_c60e, 0x1c),
    ("FollowTerrain", 0x77e3_f4f7, 0x24),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed = ["id"]
        .into_iter()
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect::<Vec<_>>();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x28)?;
    let mut entries = vec![super::behaviors_basic::module_base(
        node, KIND, start, data, true,
    )?];
    for (name, offset, default) in [
        ("StartDistanceOffset", 8, "0"),
        ("Radius", 0xc, "10"),
        ("Length", 0x10, "0"),
    ] {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let speed = finite_number(node.attribute("Speed").unwrap_or("1"), "Speed", path)? * 0.2;
    data.word(start + 0x1c, speed.to_bits());
    data.payload[start + 0x24] = boolean(node, "FollowTerrain", true, path)?;
    for attr in node.attributes() {
        let (kind, offset) = match attr.name() {
            "SweepFXList" => ("FXList", 0x14),
            "SweepWeapon" => ("WeaponTemplate", 0x18),
            _ => continue,
        };
        let reference =
            super::passive_area::reference(attr.value(), kind, path, references, reference_types)?;
        data.word(start + offset, reference);
    }
    entries.extend(fields(start, &FIELDS));
    let mut children = significant_children(node);
    let status = match children.next() {
        Some(child) if child.has_tag_name((SAGE_NS, "ObjectStatusValidation")) => Target::Table(
            super::secondary_damage::encode_status(child, start + 0x20, data)?,
        ),
        Some(_) => {
            return Err(invalid(
                path,
                "expected LinearDamageUpdate ObjectStatusValidation",
            ));
        }
        None => Target::Null,
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LinearDamageUpdate child"));
    }
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}
