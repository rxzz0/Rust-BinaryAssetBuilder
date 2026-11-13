use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, required_element, significant_children,
    validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::{boolean, fields};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0xc8e1_225f;
const REAL: u32 = 0x6458_64f3;
const INT: u32 = 0x23e6_50b8;
const BOOL: u32 = 0x77e3_f4f7;
const DAMAGE: u32 = 0x4648_6ff1;
const DEATH: u32 = 0xc608_ed36;
const PARTICLE: u32 = 0x43f1_ce36;
const STRING: u32 = 0x1a29_33aa;
const UINT: u32 = 0x7631_77a4;

const FIELDS: &[(&str, u32, usize)] = &[
    ("LeashRange", INT, 0x8),
    ("GuardMaxRange", INT, 0xc),
    ("GuardWanderRange", INT, 0x10),
    ("AttackRange", INT, 0x14),
    ("AttackWanderRange", INT, 0x18),
    ("ScoutRange", INT, 0x1c),
    ("ScoutWanderRange", INT, 0x20),
    ("DistToTargetToGrantRangeBonus", INT, 0x24),
    ("RepairRange", INT, 0x28),
    ("RepairMinAltitude", REAL, 0x2c),
    ("RepairMaxAltitude", REAL, 0x30),
    ("RepairWhenHealthBelowPercentage", INT, 0x34),
    ("RepairRatePerSecond", REAL, 0x38),
    ("MinReadyFrames", INT, 0x3c),
    ("MaxReadyFrames", INT, 0x40),
    ("MinWeldFrames", INT, 0x44),
    ("MaxWeldFrames", INT, 0x48),
    ("WeldingSysId", PARTICLE, 0x4c),
    ("WeldingFXBone", STRING, 0x50),
    ("StayOnSameLayerAsMaster", BOOL, 0x6c),
    ("DieOnMastersDeath", BOOL, 0x6d),
    ("DieOnMastersDeathDamageType", DAMAGE, 0x58),
    ("DieOnMastersDeathType", DEATH, 0x5c),
    ("FadeOutRange", INT, 0x60),
    ("FadeTime", UINT, 0x64),
    ("MarkUnselectable", BOOL, 0x6e),
    ("UseSlaverAsControlForEvaObjectSightedEvents", BOOL, 0x6f),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = FIELDS
        .iter()
        .map(|&(name, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x70)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for &(name, token, relative) in FIELDS {
        let offset = start + relative;
        let value = node.attribute(name);
        let word = match token {
            BOOL => {
                data.payload[offset] = boolean(node, name, false, path)?;
                continue;
            }
            STRING => {
                if let Some(value) = value {
                    data.string(offset, value)?;
                }
                continue;
            }
            PARTICLE => {
                let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
                    continue;
                };
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "FXParticleSystemTemplate", path)?;
                reference_types.push(id.type_id);
                references.push(id.into());
                references.len() as u32
            }
            REAL => finite_number(value.unwrap_or("0"), name, path)?.to_bits(),
            INT => value
                .unwrap_or("0")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            UINT => value
                .unwrap_or("0")
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?,
            DAMAGE | DEATH => {
                static DAMAGE_TYPES: OnceLock<Vec<String>> = OnceLock::new();
                static DEATH_TYPES: OnceLock<Vec<String>> = OnceLock::new();
                let (values, default) = if token == DAMAGE {
                    (
                        DAMAGE_TYPES.get_or_init(|| {
                            super::ai_update::enum_values(
                                include_str!("../../../../schemas/ra3/xsd/Includes/DamageDef.xsd"),
                                "DamageType",
                            )
                        }),
                        "UNRESISTABLE",
                    )
                } else {
                    (
                        DEATH_TYPES.get_or_init(|| {
                            super::ai_update::enum_values(
                                include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
                                "DeathType",
                            )
                        }),
                        "NORMAL",
                    )
                };
                values
                    .iter()
                    .position(|candidate| candidate == value.unwrap_or(default))
                    .ok_or_else(|| invalid(path, format!("invalid {name}")))? as u32
            }
            _ => unreachable!(),
        };
        data.word(offset, word);
    }
    entries.extend(fields(start, FIELDS));
    let mut children = significant_children(node).peekable();
    let guard = if children.peek().is_some() {
        let pos = required_element(&mut children, "GuardPosOffset", path)?;
        validate_attributes(pos, &["x", "y", "z"], path)?;
        if has_content(pos) || children.next().is_some() {
            return Err(invalid(path, "unexpected SlavedUpdate content"));
        }
        let start = data.allocate(start + 0x68, 12)?;
        for (index, axis) in ["x", "y", "z"].into_iter().enumerate() {
            let value = finite_number(pos.attribute(axis).unwrap_or("0"), axis, path)?;
            data.word(start + index * 4, value.to_bits());
        }
        Target::Data(start)
    } else {
        Target::Null
    };
    entries.push(Entry::named("GuardPosOffset", 0x2936_8077, guard));
    Ok((KIND, Table::record(entries)))
}
