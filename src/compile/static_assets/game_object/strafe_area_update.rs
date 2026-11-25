use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, finite_number, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::passive_area::reference;
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x09be_107c;
const WEAPON_REF: u32 = 0xd046_dd22;
const REAL: u32 = 0x6458_64f3;
const FIELDS: [(&str, u32, usize); 10] = [
    ("Radius", REAL, 0x10),
    ("MinRadius", REAL, 0x14),
    ("PreattackDistance", REAL, 0x18),
    ("SweepFrequency", REAL, 0x1c),
    ("SweepAmplitude", REAL, 0x20),
    ("DivingFloor", REAL, 0x24),
    ("InitialSweepPhase", REAL, 0x28),
    ("TargetJitterOffset", REAL, 0x2c),
    ("ShotsToFirePerFrame", 0x7631_77a4, 0x30),
    ("Options", 0x6180_98c1, 0x34),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = ["id", "WeaponName", "SecondaryWeaponName"]
        .into_iter()
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected StrafeAreaUpdate content"));
    }

    let start = data.allocate(pointer, 0x38)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];

    let mut secondary = Target::Null;
    for attribute in node.attributes() {
        let offset = match attribute.name() {
            "WeaponName" => start + 8,
            "SecondaryWeaponName" => {
                let offset = data.allocate(start + 0xc, 4)?;
                secondary = Target::Data(offset);
                offset
            }
            _ => continue,
        };
        let ordinal = reference(
            attribute.value(),
            "WeaponTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(offset, ordinal);
    }
    entries.push(Entry::named(
        "WeaponName",
        WEAPON_REF,
        Target::Data(start + 8),
    ));
    entries.push(Entry::named("SecondaryWeaponName", WEAPON_REF, secondary));

    for &(name, _, offset) in &FIELDS[..8] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    let shots = node
        .attribute("ShotsToFirePerFrame")
        .unwrap_or("1")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid ShotsToFirePerFrame"))?;
    data.word(start + 0x30, shots);

    let mut options = 0;
    for option in node
        .attribute("Options")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        options |= match option {
            "FLEE_STRAIGHT_AHEAD" => 1,
            "USE_TARGET_CHOOSER" => 2,
            "DO_NOT_AVOID_TALLBUILDING" => 4,
            "CANNOT_IDLE" => 8,
            _ => {
                return Err(invalid(
                    path,
                    format!("invalid StrafeAreaUpdate option {option}"),
                ));
            }
        };
    }
    data.word(start + 0x34, options);
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}
