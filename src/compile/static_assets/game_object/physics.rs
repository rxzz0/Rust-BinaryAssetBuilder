use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{hash_name, validate_attributes};
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    validate_attributes(node, &["id"], data.path)?;
    if has_content(node) {
        return Err(invalid(data.path, "unexpected Physics content"));
    }
    let id = required(node, "id", data.path)?;
    let offset = data.allocate(pointer, 0x68)?;
    const KIND: u32 = 0x79bf_edcb;
    data.word(offset, KIND);
    data.word(offset + 4, hash_name(id));
    for (field, value) in [
        (0x08, 360.0_f32.to_bits()),
        (0x0c, 360.0_f32.to_bits()),
        (0x10, 360.0_f32.to_bits()),
        (0x14, 1.0_f32.to_bits()),
        (0x18, 1.0_f32.to_bits()),
        (0x1c, 1.0_f32.to_bits()),
        (0x20, 1.5_f32.to_bits()),
        (0x24, 1.3_f32.to_bits()),
        (0x28, 1.3_f32.to_bits()),
        (0x2c, (33.0_f32 * 0.01).to_bits()),
        (0x30, (66.0_f32 * 0.01).to_bits()),
        (0x34, 2),
        (0x38, 5.0_f32.to_bits()),
        (0x3c, 1.3_f32.to_bits()),
        (0x40, 1.3_f32.to_bits()),
        (0x44, (33.0_f32 * 0.01).to_bits()),
        (0x48, (66.0_f32 * 0.01).to_bits()),
        (0x54, (33.0_f32 * 0.01).to_bits()),
        (0x58, (66.0_f32 * 0.01).to_bits()),
    ] {
        data.word(offset + field, value);
    }
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(offset)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(offset + 4)),
    ]);
    let mut base = module;
    for kind in [0xd75d_93a2, 0xa50d_729a, 0x2900_211d] {
        base = Table::record(vec![Entry::new(1, kind, Target::Table(base))]);
    }
    let mut entries = vec![Entry::new(1, 0xf100_2505, Target::Table(base))];
    for (name, kind, field) in [
        ("TumbleRandomly", 0x77e3_f4f7, 0x60),
        ("MaxXRotationVelocity", 0x6458_64f3, 0x08),
        ("MaxYRotationVelocity", 0x6458_64f3, 0x0c),
        ("MaxZRotationVelocity", 0x6458_64f3, 0x10),
        ("AllowBouncing", 0x77e3_f4f7, 0x61),
        ("KillWhenRestingOnGround", 0x77e3_f4f7, 0x62),
        ("GravityMultiplier", 0x6458_64f3, 0x14),
        ("OrientToFlightPath", 0x77e3_f4f7, 0x63),
        ("ShockStunnedTimeLow", 0x9986_69e8, 0x18),
        ("ShockStunnedTimeHigh", 0x9986_69e8, 0x1c),
        ("ShockStandingTime", 0x9986_69e8, 0x20),
        ("FirstHeight", 0x6458_64f3, 0x24),
        ("SecondHeight", 0x6458_64f3, 0x28),
        ("FirstPercentIndent", 0x6458_64f3, 0x2c),
        ("SecondPercentIndent", 0x6458_64f3, 0x30),
        ("BounceCount", 0x23e6_50b8, 0x34),
        ("BounceDistance", 0x6458_64f3, 0x38),
        ("BounceFirstHeight", 0x6458_64f3, 0x3c),
        ("BounceSecondHeight", 0x6458_64f3, 0x40),
        ("BounceFirstPercentIndent", 0x6458_64f3, 0x44),
        ("BounceSecondPercentIndent", 0x6458_64f3, 0x48),
        ("GroundHitFX", 0x0d1c_8e83, 0x4c),
        ("GroundBounceFX", 0x0d1c_8e83, 0x50),
        ("IgnoreTerrainHeight", 0x77e3_f4f7, 0x64),
        ("FirstPercentHeight", 0x6458_64f3, 0x54),
        ("SecondPercentHeight", 0x6458_64f3, 0x58),
        ("CurveFlattenMinDist", 0x6458_64f3, 0x5c),
    ] {
        entries.push(Entry::named(name, kind, Target::Data(offset + field)));
    }
    Ok((KIND, Table::record(entries)))
}
