use super::super::binary::BinaryData;
use super::super::tokenized::Entry;
use super::behaviors_basic::optional_vector;
use roxmltree::Node;
use std::io;

pub(super) const KIND: u32 = 0x34e5_d5b5;
pub(super) const SIZE: usize = 0x118;
pub(super) const BONE_OFFSET: &str = "EjectionBoneOffset";
pub(super) const FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    ("TimePerPassenger", 0x9986_69e8, 0xf0, "time", "1s"),
    ("Speed", 0x6458_64f3, 0xf4, "real", "10"),
    ("Radius", 0x6458_64f3, 0xf8, "real", "20"),
    ("Height", 0x6458_64f3, 0xfc, "real", "100"),
    ("ApexHeight", 0x6458_64f3, 0x100, "real", "0"),
    ("EjectionFX", 0x0d1c_8e83, 0x104, "ref", "FXList"),
    ("EjectionBoneName", 0x1a29_33aa, 0x108, "preencoded", ""),
    ("RunOffMapWhenDone", 0x77e3_f4f7, 0x114, "bool", "false"),
    ("IgnoreTargetLocation", 0x77e3_f4f7, 0x115, "bool", "false"),
];

pub(super) fn nested(
    node: Node<'_, '_>,
    bone_offset: Option<Node<'_, '_>>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Entry> {
    // EA allocates the derived bone fields before the base ability's strings and animation.
    if let Some(name) = node.attribute("EjectionBoneName") {
        data.string(start + 0x108, name)?;
    }
    let vector = optional_vector(bone_offset, start + 0x110, data)?;
    Ok(Entry::named(BONE_OFFSET, 0x2936_8077, vector))
}
