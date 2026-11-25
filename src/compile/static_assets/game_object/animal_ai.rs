use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table};
use super::super::{finite_number, sage_time};
use super::draws::{boolean, fields};
use crate::compile::invalid;
use roxmltree::Node;
use std::io;

pub(super) const KIND: u32 = 0xcd56_659b;
pub(super) const SIZE: usize = 0x74;
const INT: u32 = 0x23e6_50b8;
const TIME: u32 = 0x9986_69e8;
const REAL: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;
pub(super) const FIELDS: &[(&str, u32, usize)] = &[
    ("FleeRadius", INT, 0x4c),
    ("FleeDistance", INT, 0x50),
    ("WanderPercentage", INT, 0x54),
    ("MinDistance", INT, 0x58),
    ("MaxDistance", INT, 0x5c),
    ("Radius", INT, 0x60),
    ("FleeUpdateTimer", TIME, 0x64),
    ("WanderUpdateTimer", TIME, 0x68),
    ("AfraidOfCastles", BOOL, 0x70),
    ("InitialFleeBlindlyRadius", REAL, 0x6c),
];

pub(super) fn encode_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    inherited: Entry,
) -> io::Result<Table> {
    const DEFAULTS: &[&str] = &[
        "20", "100", "35", "15", "50", "25", "0.8s", "2s", "true", "50",
    ];
    let path = data.path;
    for (&(name, kind, offset), &default) in FIELDS.iter().zip(DEFAULTS) {
        let value = node.attribute(name).unwrap_or(default);
        let word = match kind {
            INT => value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            TIME => sage_time(value, name, path)?.to_bits(),
            REAL => finite_number(value, name, path)?.to_bits(),
            BOOL => {
                data.payload[start + offset] = boolean(node, name, true, path)?;
                continue;
            }
            _ => unreachable!(),
        };
        data.word(start + offset, word);
    }
    let mut entries = vec![inherited];
    entries.extend(fields(start, FIELDS));
    Ok(Table::record(entries))
}
