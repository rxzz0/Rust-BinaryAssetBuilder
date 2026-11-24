use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Target};
use super::parking::encode_filter;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;

pub(super) const KIND: u32 = 0x9b27_de30;
pub(super) const SIZE: usize = 0x104;
pub(super) const FILTER_POINTER: usize = 0xfc;
pub(super) const FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    ("DominateRadius", 0x6458_64f3, 0xf0, "real", "0"),
    ("TriggerFX", 0x0d1c_8e83, 0xf4, "ref", "FXList"),
    ("DominatedFX", 0x0d1c_8e83, 0xf8, "ref", "FXList"),
    ("PermanentlyConvert", 0x77e3_f4f7, 0x100, "bool", "false"),
];

pub(super) fn filter<'a, 'input: 'a>(
    mut children: impl Iterator<Item = Node<'a, 'input>>,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Entry> {
    const FILTER: &str = "AttributeModifierAffects";
    let target = match children.next() {
        Some(child) if child.has_tag_name((SAGE_NS, FILTER)) && children.next().is_none() => {
            Target::Table(encode_filter(
                child,
                start + FILTER_POINTER,
                data,
                reference_types,
            )?)
        }
        None => Target::Null,
        _ => {
            return Err(invalid(
                data.path,
                "unexpected DominateEnemySpecialPower child",
            ));
        }
    };
    Ok(Entry::named(FILTER, 0xe46e_a3b2, target))
}
