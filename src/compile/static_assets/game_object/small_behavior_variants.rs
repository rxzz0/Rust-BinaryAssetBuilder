use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, percentage, sage_uint, schema_enums, validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::passive_area::reference;
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::{io, sync::LazyLock};

type Field = (&'static str, u32, usize, Option<&'static str>);

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const DISTRIBUTED_MONEY: u32 = 0x3cfb_de26;
    const FIND_COVER: u32 = 0xbeb7_d7a3;
    const GATHER_SLAVES: u32 = 0x264e_c61a;
    const UINT: u32 = 0x7631_77a4;
    const REAL: u32 = 0x6458_64f3;
    const DEATH_TYPE: u32 = 0xc608_ed36;
    const MODIFIER_REF: u32 = 0x09dd_cff3;
    const GAME_OBJECT_REF: u32 = 0xbfab_9075;

    let path = data.path;
    let name = node.tag_name().name();
    let (kind, layout): (u32, &[Field]) = match name {
        "DistributedMoney" => (
            DISTRIBUTED_MONEY,
            &[
                ("Capacity", UINT, 8, Some("1")),
                ("OnDieSpawnPercentage", REAL, 12, Some("1.0")),
                (
                    "RedistributeMoneyOnDeathType",
                    DEATH_TYPE,
                    16,
                    Some("SUICIDED"),
                ),
            ],
        ),
        "FindCoverBehavior" => (
            FIND_COVER,
            &[
                ("AttributeModifierInCover", MODIFIER_REF, 8, None),
                ("CoverScanRange", REAL, 12, None),
            ],
        ),
        "GatherSlavesUpdate" => (
            GATHER_SLAVES,
            &[
                ("SlaveTemplate", GAME_OBJECT_REF, 8, Some("")),
                ("Radius", REAL, 12, Some("0")),
                ("Amount", UINT, 16, Some("0")),
            ],
        ),
        _ => return Err(invalid(path, format!("unexpected behavior {name}"))),
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(layout.iter().map(|&(field, ..)| field))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, format!("unexpected {name} content")));
    }
    let start = data.allocate(pointer, 8 + layout.len() * 4)?;
    let root = module_base(node, kind, start, data, kind != DISTRIBUTED_MONEY)?;
    let mut entries = vec![if kind == DISTRIBUTED_MONEY {
        base(0x30a3_6b8d, Table::record(vec![root]))
    } else {
        root
    }];
    for &(field, token, offset, default) in layout {
        let value = match default {
            Some(value) => node.attribute(field).unwrap_or(value),
            None => required(node, field, path)?,
        };
        let word = match token {
            UINT => sage_uint(value, field, path)?,
            REAL if kind == DISTRIBUTED_MONEY => percentage(value, field, path)?.to_bits(),
            REAL => finite_number(value, field, path)?.to_bits(),
            DEATH_TYPE => {
                static TYPES: LazyLock<[Vec<String>; 1]> = LazyLock::new(|| {
                    schema_enums(
                        include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
                        ["DeathType"],
                    )
                });
                TYPES[0]
                    .iter()
                    .position(|item| item == value)
                    .ok_or_else(|| invalid(path, format!("invalid {field} {value:?}")))?
                    as u32
            }
            MODIFIER_REF | GAME_OBJECT_REF => reference(
                value,
                if token == MODIFIER_REF {
                    "AttributeModifier"
                } else {
                    "GameObject"
                },
                path,
                references,
                reference_types,
            )?,
            _ => unreachable!("small behavior field codec"),
        };
        data.word(start + offset, word);
        entries.push(Entry::named(field, token, Target::Data(start + offset)));
    }
    Ok((kind, Table::record(entries)))
}
