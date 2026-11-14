use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::Table;
use super::super::{AssetReference, finite_number, percentage, sage_time, validate_attributes};
use crate::compile::{has_content, invalid};
use roxmltree::Node;
use std::{io, sync::OnceLock};

const KIND: u32 = 0x0bbf_2a39;
const FIELDS: &[(&str, u32, usize, &str)] = &[
    ("Flags", 0xc526_ba4c, 0x8, "flags"),
    ("Type", 0xa11a_f410, 0xc, "type"),
    ("FireStraightDistance", 0x6458_64f3, 0x10, "real"),
    ("FirstHeight", 0x6458_64f3, 0x14, "real"),
    ("SecondHeight", 0x6458_64f3, 0x18, "real"),
    ("FirstHeightMin", 0x6458_64f3, 0x1c, "real"),
    ("FirstHeightMax", 0x6458_64f3, 0x20, "real"),
    ("SecondHeightMin", 0x6458_64f3, 0x24, "real"),
    ("SecondHeightMax", 0x6458_64f3, 0x28, "real"),
    ("FirstPercentIndent", 0x6458_64f3, 0x2c, "percent"),
    ("SecondPercentIndent", 0x6458_64f3, 0x30, "percent"),
    ("FinalStuckTime", 0x9986_69e8, 0x34, "time"),
    ("PreLandingStateTime", 0x9986_69e8, 0x38, "time"),
    ("BounceCount", 0x23e6_50b8, 0x3c, "int"),
    ("BounceDistance", 0x6458_64f3, 0x40, "real"),
    ("BounceFirstHeight", 0x6458_64f3, 0x44, "real"),
    ("BounceSecondHeight", 0x6458_64f3, 0x48, "real"),
    ("BounceFirstPercentIndent", 0x6458_64f3, 0x4c, "percent"),
    ("BounceSecondPercentIndent", 0x6458_64f3, 0x50, "percent"),
    ("GarrisonHitKillRequiredKindOf", 0x9afc_9193, 0x54, "kind"),
    ("GarrisonHitKillForbiddenKindOf", 0x9afc_9193, 0x78, "kind"),
    ("GarrisonHitKillCount", 0x7631_77a4, 0x9c, "uint"),
    ("GarrisonHitKillFX", 0x0d1c_8e83, 0xa0, "fx"),
    ("GroundHitFX", 0x0d1c_8e83, 0xa4, "fx"),
    ("MaxDistanceReachedFX", 0x0d1c_8e83, 0xa8, "fx"),
    ("GroundBounceFX", 0x0d1c_8e83, 0xac, "fx"),
    ("GroundHitWeapon", 0xd046_dd22, 0xb0, "weapon"),
    ("GroundBounceWeapon", 0xd046_dd22, 0xb4, "weapon"),
    (
        "FlightPathAdjustDistPerSecond",
        0x7a76_c60e,
        0xb8,
        "velocity",
    ),
    ("FirstPercentHeight", 0x6458_64f3, 0xbc, "percent"),
    ("SecondPercentHeight", 0x6458_64f3, 0xc0, "percent"),
    ("CurveFlattenMinDist", 0x6458_64f3, 0xc4, "real"),
    ("PreLandingEmotion", 0xabe9_f4d5, 0xc8, "emotion"),
    ("PreLandingEmotionRadius", 0x6458_64f3, 0xcc, "real"),
    ("InvisibleTime", 0x9986_69e8, 0xd0, "time"),
    ("FadeInTime", 0x9986_69e8, 0xd4, "time"),
    ("PostLandingStateTime", 0x9986_69e8, 0xd8, "time"),
    ("PostLandingEmotion", 0xabe9_f4d5, 0xdc, "emotion"),
    ("PostLandingEmotionRadius", 0x6458_64f3, 0xe0, "real"),
    ("SidewaysDrift", 0x6458_64f3, 0xe4, "real"),
    ("MaxDistanceToTravel", 0x6458_64f3, 0xe8, "real"),
    ("TargetVectorOffset", 0x6458_64f3, 0xec, "real"),
    ("Acceleration", 0x6458_64f3, 0xf0, "real"),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(FIELDS.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected BezierProjectile content"));
    }
    let start = data.allocate(pointer, 0xf4)?;
    let parent = super::behaviors_basic::module_base(node, KIND, start, data, true)?;
    for attr in node.attributes() {
        let Some(&(_, _, offset, storage @ ("fx" | "weapon"))) =
            FIELDS.iter().find(|&&(name, ..)| name == attr.name())
        else {
            continue;
        };
        let kind = if storage == "fx" {
            "FXList"
        } else {
            "WeaponTemplate"
        };
        let value =
            super::passive_area::reference(attr.value(), kind, path, references, reference_types)?;
        data.word(start + offset, value);
    }
    for &(name, _, offset, storage) in FIELDS {
        let offset = start + offset;
        let mut value = node
            .attribute(name)
            .unwrap_or(match name {
                "MaxDistanceToTravel" => "1000000",
                "Flags" => "PING_PONG_SIDEWAYS_DRIFT",
                "Type" => "MISSILE",
                _ => "",
            })
            .trim();
        let word = match storage {
            "fx" | "weapon" => continue,
            "flags" => {
                static FLAGS: OnceLock<Vec<String>> = OnceLock::new();
                let flags = FLAGS.get_or_init(|| {
                    super::ai_update::enum_values(
                        include_str!("../../../../schemas/ra3/xsd/Modules/BezierProjectile.xsd"),
                        "BezierProjectileFlag",
                    )
                });
                let mut bits = 0;
                for flag in value.split_ascii_whitespace() {
                    let index = flags.iter().position(|item| item == flag).ok_or_else(|| {
                        invalid(path, format!("invalid BezierProjectile flag {flag}"))
                    })?;
                    bits |= 1 << index;
                }
                bits
            }
            "type" => enum_value(value, &["MISSILE", "TORPEDO"], path)?,
            "kind" => {
                super::fields::write_kind_of(value, offset, data)?;
                continue;
            }
            _ if value.is_empty() => continue,
            "emotion" => enum_value(
                value,
                &[
                    "TAUNT",
                    "CHEER",
                    "HERO_CHEER",
                    "POINT",
                    "FEAR",
                    "UNCONTROLLABLE_FEAR",
                    "TERROR",
                    "DOOM",
                    "ALERT",
                    "BRACE_FOR_BEING_CRUSHED",
                    "CHEER_FOR_ABOUT_TO_CRUSH",
                ],
                path,
            )?,
            "real" => finite_number(value, name, path)?.to_bits(),
            "velocity" => {
                value = value.strip_suffix("/s").unwrap_or(value);
                (finite_number(value, name, path)? / 5.0).to_bits()
            }
            "percent" => percentage(value, name, path)?.to_bits(),
            "time" => sage_time(value, name, path)?.to_bits(),
            "int" => value
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            "uint" => value
                .parse::<u32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?,
            _ => unreachable!(),
        };
        data.word(offset, word);
    }
    let entries = FIELDS
        .iter()
        .map(|&(name, kind, offset, _)| (name, kind, offset))
        .collect::<Vec<_>>();
    let fields = std::iter::once(parent)
        .chain(super::draws::fields(start, &entries))
        .collect();
    Ok((KIND, Table::record(fields)))
}
