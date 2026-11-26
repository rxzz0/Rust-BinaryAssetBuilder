use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, sage_time, significant_children, validate_attributes};
use super::behaviors_basic::{death_flags, module_base};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x59d1_2990;
    const TIME: u32 = 0x9986_69e8;
    const REAL: u32 = 0x6458_64f3;
    const BOOL: u32 = 0x77e3_f4f7;
    const FIELDS: [(&str, u32, usize); 10] = [
        ("LightDamageHitReactionLifeTimer", TIME, 8),
        ("MediumDamageHitReactionLifeTimer", TIME, 12),
        ("HeavyDamageHitReactionLifeTimer", TIME, 16),
        ("LightDamageHitReactionThreshold", REAL, 20),
        ("MediumDamageHitReactionThreshold", REAL, 24),
        ("HeavyDamageHitReactionThreshold", REAL, 28),
        ("RotateToSourceWhenDamaged", BOOL, 44),
        ("RotateToSourceIgnoreIdleCheck", BOOL, 45),
        ("GoIdleWhenDamaged", BOOL, 46),
        ("RequiredDeathTypes", 0x034d_f1a1, 32),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let status = children.next();
    if status.is_some_and(|child| !child.has_tag_name((SAGE_NS, "ObjectStatusValidation")))
        || children.next().is_some()
    {
        return Err(invalid(path, "unexpected HitReactionBehavior content"));
    }

    let start = data.allocate(pointer, 0x30)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, kind, offset) in FIELDS {
        match kind {
            TIME => {
                let value = sage_time(node.attribute(name).unwrap_or("0s"), name, path)?;
                data.word(start + offset, value.to_bits());
            }
            REAL => {
                let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
                data.word(start + offset, value.to_bits());
            }
            BOOL => data.payload[start + offset] = boolean(node, name, false, path)?,
            _ => {
                let flags = death_flags(node.attribute(name).unwrap_or(""), path)?;
                data.word(start + offset, flags as u32);
                data.word(start + offset + 4, (flags >> 32) as u32);
            }
        }
    }
    entries.extend(fields(start, &FIELDS));
    let status = match status {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 40,
            data,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}
