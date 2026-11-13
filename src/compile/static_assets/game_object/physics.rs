use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, finite_number, percentage, sage_time, validate_attributes};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::io;

const FIELDS: &[(&str, u32, usize, &str)] = &[
    ("TumbleRandomly", 0x77e3_f4f7, 0x60, "0"),
    ("MaxXRotationVelocity", 0x6458_64f3, 0x08, "360"),
    ("MaxYRotationVelocity", 0x6458_64f3, 0x0c, "360"),
    ("MaxZRotationVelocity", 0x6458_64f3, 0x10, "360"),
    ("AllowBouncing", 0x77e3_f4f7, 0x61, "0"),
    ("KillWhenRestingOnGround", 0x77e3_f4f7, 0x62, "0"),
    ("GravityMultiplier", 0x6458_64f3, 0x14, "1"),
    ("OrientToFlightPath", 0x77e3_f4f7, 0x63, "0"),
    ("ShockStunnedTimeLow", 0x9986_69e8, 0x18, "1s"),
    ("ShockStunnedTimeHigh", 0x9986_69e8, 0x1c, "1s"),
    ("ShockStandingTime", 0x9986_69e8, 0x20, "1.5s"),
    ("FirstHeight", 0x6458_64f3, 0x24, "1.3"),
    ("SecondHeight", 0x6458_64f3, 0x28, "1.3"),
    ("FirstPercentIndent", 0x6458_64f3, 0x2c, "33"),
    ("SecondPercentIndent", 0x6458_64f3, 0x30, "66"),
    ("BounceCount", 0x23e6_50b8, 0x34, "2"),
    ("BounceDistance", 0x6458_64f3, 0x38, "5"),
    ("BounceFirstHeight", 0x6458_64f3, 0x3c, "1.3"),
    ("BounceSecondHeight", 0x6458_64f3, 0x40, "1.3"),
    ("BounceFirstPercentIndent", 0x6458_64f3, 0x44, "33"),
    ("BounceSecondPercentIndent", 0x6458_64f3, 0x48, "66"),
    ("GroundHitFX", 0x0d1c_8e83, 0x4c, "0"),
    ("GroundBounceFX", 0x0d1c_8e83, 0x50, "0"),
    ("IgnoreTerrainHeight", 0x77e3_f4f7, 0x64, "0"),
    ("FirstPercentHeight", 0x6458_64f3, 0x54, "33"),
    ("SecondPercentHeight", 0x6458_64f3, 0x58, "66"),
    ("CurveFlattenMinDist", 0x6458_64f3, 0x5c, "0"),
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
        .map(|&(name, _, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected Physics content"));
    }
    let start = data.allocate(pointer, 0x68)?;
    const KIND: u32 = 0x79bf_edcb;
    let mut entries = vec![super::behaviors_basic::module_base(
        node, KIND, start, data, true,
    )?];
    for attr in node.attributes() {
        if let Some(&(_, 0x0d1c_8e83, field, _)) =
            FIELDS.iter().find(|&&(name, _, _, _)| name == attr.name())
        {
            let ordinal = super::passive_area::reference(
                attr.value(),
                "FXList",
                path,
                references,
                reference_types,
            )?;
            data.word(start + field, ordinal);
        }
    }
    for &(name, kind, field, default) in FIELDS {
        let offset = start + field;
        let value = node.attribute(name).unwrap_or(default);
        let word = match kind {
            0x77e3_f4f7 => {
                data.payload[offset] = super::draws::boolean(node, name, false, path)?;
                None
            }
            0x9986_69e8 => Some(sage_time(value, name, path)?.to_bits()),
            0x23e6_50b8 => Some(
                value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            ),
            0x0d1c_8e83 => None,
            _ if name.contains("Percent") => Some(percentage(value, name, path)?.to_bits()),
            _ => Some(finite_number(value, name, path)?.to_bits()),
        };
        if let Some(word) = word {
            data.word(offset, word);
        }
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    Ok((KIND, Table::record(entries)))
}
