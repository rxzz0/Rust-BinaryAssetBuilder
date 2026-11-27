use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, FLOAT_TYPE, Table, Target};
use super::super::{AssetReference, finite_number, sage_int, sage_time, validate_attributes};
use super::behaviors_basic::{base, module_base};
use super::draws::boolean;
use super::passive_area::reference;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const CLUSTER: u32 = 0xe486_09fb;
const LASER: u32 = 0xca9e_d83d;
const WEAPON: u32 = 0xd046_dd22;
const FX: u32 = 0x0d1c_8e83;
const PARTICLE: u32 = 0x43f1_ce36;
const TIME: u32 = 0x9986_69e8;
const INTEGER: u32 = 0x23e6_50b8;
const BOOLEAN: u32 = 0x77e3_f4f7;
const STRING: u32 = 0x1a29_33aa;
const INITIAL_WEAPON: &str = "InitialWeaponName";

const CLUSTER_FIELDS: &[(&str, u32, usize)] = &[
    ("WeaponName", WEAPON, 8),
    (INITIAL_WEAPON, WEAPON, 12),
    ("Radius", FLOAT_TYPE, 16),
    ("NumBomblets", INTEGER, 20),
    ("MinDelay", TIME, 24),
    ("MaxDelay", TIME, 28),
    ("BombletFX", FX, 32),
];
const LASER_FIELDS: &[(&str, u32, usize)] = &[
    ("ParticleSystemId", PARTICLE, 8),
    ("ParentFireBoneName", STRING, 12),
    ("ParentFireBoneOnTurret", BOOLEAN, 28),
    ("TargetParticleSystemId", PARTICLE, 20),
    ("LaserLifetime", FLOAT_TYPE, 24),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let name = node.tag_name().name();
    let (kind, size, fields) = match name {
        "ClusterBombUpdate" => (CLUSTER, 0x24, CLUSTER_FIELDS),
        "LaserUpdate" => (LASER, 0x20, LASER_FIELDS),
        _ => return Err(invalid(path, format!("unexpected behavior {name}"))),
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(fields.iter().map(|&(field, ..)| field))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, format!("unexpected {name} content")));
    }
    let start = data.allocate(pointer, size)?;
    let parent = module_base(node, kind, start, data, kind == CLUSTER)?;
    let mut entries = vec![if kind == LASER {
        base(0x4cad_6edc, Table::record(vec![parent]))
    } else {
        parent
    }];

    let mut initial_weapon = None;
    for attr in node.attributes() {
        let Some(&(_, tag, offset)) = fields.iter().find(|field| field.0 == attr.name()) else {
            continue;
        };
        let target = match tag {
            WEAPON => "WeaponTemplate",
            FX => "FXList",
            PARTICLE => "FXParticleSystemTemplate",
            _ => continue,
        };
        let ordinal = reference(attr.value(), target, path, references, reference_types)?;
        let mut offset = start + offset;
        if attr.name() == INITIAL_WEAPON {
            offset = data.allocate(offset, 4)?;
            initial_weapon = Some(offset);
        }
        data.word(offset, ordinal);
    }
    for &(field, tag, offset) in fields {
        let value = node.attribute(field);
        let offset = start + offset;
        match tag {
            FLOAT_TYPE => data.word(
                offset,
                finite_number(value.unwrap_or("0"), field, path)?.to_bits(),
            ),
            INTEGER => data.word(offset, sage_int(value.unwrap_or("0"), field, path)?),
            TIME => data.word(
                offset,
                sage_time(value.unwrap_or("0s"), field, path)?.to_bits(),
            ),
            BOOLEAN => data.payload[offset] = boolean(node, field, false, path)?,
            STRING => {
                if let Some(value) = value {
                    data.string(offset, value)?;
                }
            }
            _ => {}
        }
        let target = if field == INITIAL_WEAPON {
            initial_weapon.map_or(Target::Null, Target::Data)
        } else {
            Target::Data(offset)
        };
        entries.push(Entry::named(field, tag, target));
    }
    Ok((kind, Table::record(entries)))
}
