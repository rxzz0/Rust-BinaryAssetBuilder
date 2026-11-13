use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, hash_name, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::boolean;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) const KIND: u32 = 0x2fdd_ec4d;
const BOOL: u32 = 0x77e3_f4f7;
const REAL: u32 = 0x6458_64f3;
const INT: u32 = 0x23e6_50b8;
const TIME: u32 = 0x9986_69e8;
const FIELDS: &[(&str, u32, usize, &str)] = &[
    ("UseDistortionShader", BOOL, 0x6c, "false"),
    ("Texture1_UTile", REAL, 0x08, "1"),
    ("Texture1_VTile", REAL, 0x0c, "1"),
    ("Texture2_UTile", REAL, 0x10, "1"),
    ("Texture2_VTile", REAL, 0x14, "1"),
    ("Texture1_UScrollRate", REAL, 0x18, "0"),
    ("Texture1_VScrollRate", REAL, 0x1c, "0"),
    ("Texture2_UScrollRate", REAL, 0x20, "0"),
    ("Texture2_VScrollRate", REAL, 0x24, "0"),
    ("Texture1_NumFrames", INT, 0x28, "1"),
    ("Texture1_FrameRate", REAL, 0x2c, "30"),
    ("Texture2_NumFrames", INT, 0x30, "1"),
    ("Texture2_FrameRate", REAL, 0x34, "30"),
    ("LaserWidth", REAL, 0x38, "5"),
    ("FadeInTime", TIME, 0x3c, "0s"),
    ("FadeOutTime", TIME, 0x40, "0s"),
    ("WeaponSlotID", INT, 0x44, "-1"),
    ("LaserStateID", INT, 0x48, "0"),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
    float_names: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.iter().map(|&(name, _, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let shader = children
        .next_if(|child| child.has_tag_name((SAGE_NS, "FXShader")))
        .ok_or_else(|| invalid(path, "LaserDraw requires FXShader"))?;
    let status = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectStatusValidation")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LaserDraw child"));
    }

    let start = data.allocate(pointer, 0x70)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut entries = vec![base(0x17f6_1728, inherited(0xd75d_93a2, module))];
    for &(name, kind, offset, default) in FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        if kind == BOOL {
            data.payload[start + offset] = boolean(node, name, default == "true", path)?;
        } else {
            let word = match kind {
                REAL => finite_number(value, name, path)?.to_bits(),
                TIME => sage_time(value, name, path)?.to_bits(),
                INT => value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?
                    as u32,
                _ => unreachable!("LaserDraw field type"),
            };
            data.word(start + offset, word);
        }
        entries.push(Entry::named(name, kind, Target::Data(start + offset)));
    }
    let shader = super::material::encode_inline(
        shader,
        start + 0x4c,
        data,
        references,
        reference_types,
        float_names,
    )?;
    let status = match status {
        Some(node) => Target::Table(super::secondary_damage::encode_status(
            node,
            start + 0x68,
            data,
        )?),
        None => Target::Null,
    };
    entries.extend([
        Entry::named("FXShader", 0xc7ea_3cf4, Target::Table(shader)),
        Entry::named("ObjectStatusValidation", 0x3e8f_938c, status),
    ]);
    Ok(Table::record(entries))
}
