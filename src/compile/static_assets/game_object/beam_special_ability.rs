pub(super) const KIND: u32 = 0xa0aa_07e1;
pub(super) const SIZE: usize = 0x110;
pub(super) const FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    ("DamagePerSecond", 0x6458_64f3, 0xf0, "real", "0"),
    ("JoinWithOtherBeams", 0x77e3_f4f7, 0x10c, "bool", "false"),
    (
        "DissapateWhenTargetDead",
        0x77e3_f4f7,
        0x10d,
        "bool",
        "true",
    ),
    ("LineType", 0x77e3_f4f7, 0x10e, "bool", "false"),
    (
        "TargetAttributeModifierAdd",
        0x09dd_cff3,
        0xf4,
        "ref",
        "AttributeModifier",
    ),
    (
        "TargetObjectStatusAdd",
        0xa7bd_811e,
        0xf8,
        "optional_status",
        "",
    ),
    ("SweepFX", 0x0d1c_8e83, 0xfc, "ref", "FXList"),
    ("ReflectorExtendDistance", 0x6458_64f3, 0x100, "real", "0"),
    ("TargetSamePlayerOnly", 0x77e3_f4f7, 0x10f, "bool", "false"),
    ("PreferredTargetBone", 0x1a29_33aa, 0x104, "string", ""),
];
