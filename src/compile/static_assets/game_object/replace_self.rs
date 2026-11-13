use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, model_condition_index};
use super::secondary_damage::write_status_flags;
use super::spawn::{id_list, weak_reference};
use super::tech_upgrade::encode_anim;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::{Document, Node};
use std::{io, sync::OnceLock};

const KIND: u32 = 0x9122_2181;
const FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    (
        "SpecialPowerTemplate",
        0xaee83888,
        0x8,
        "ref",
        "SpecialPowerTemplate",
    ),
    ("StartAbilityRange", 0x645864f3, 0xc, "real", "10000000"),
    ("AbilityAbortRange", 0x645864f3, 0x10, "real", "10000000"),
    ("PreparationTime", 0x998669e8, 0x14, "time", "0s"),
    ("PersistentPrepTime", 0x998669e8, 0x18, "time", "0s"),
    ("PersistentCount", 0x23e650b8, 0x1c, "int", "-1"),
    ("PackTime", 0x998669e8, 0x20, "time", "0s"),
    ("UnpackTime", 0x998669e8, 0x24, "time", "0s"),
    ("PreTriggerUnstealthTime", 0x998669e8, 0x28, "time", "0s"),
    ("Options", 0x982f7738, 0x2c, "options", ""),
    ("PackUnpackVariationFactor", 0x645864f3, 0x30, "real", "0"),
    (
        "ParalyzeDurationWhenCompleted",
        0x998669e8,
        0x34,
        "time",
        "0s",
    ),
    (
        "ParalyzeDurationWhenAborted",
        0x998669e8,
        0x38,
        "time",
        "0s",
    ),
    ("SpecialObject", 0x148ed75d, 0x3c, "weak", "GameObject"),
    ("SpecialObjectAttachToBone", 0x1a2933aa, 0x40, "string", ""),
    ("MaxSpecialObjects", 0x763177a4, 0x48, "uint", "1"),
    ("EffectDuration", 0x998669e8, 0x4c, "time", "0s"),
    ("EffectValue", 0x23e650b8, 0x50, "int", "1"),
    ("EffectRange", 0x645864f3, 0x54, "real", "0"),
    ("FleeRangeAfterCompletion", 0x645864f3, 0x58, "real", "0"),
    (
        "DisableFXParticleSystem",
        0x43f1ce36,
        0x5c,
        "ref",
        "FXParticleSystemTemplate",
    ),
    ("PackSound", 0x63d7b468, 0x60, "audio", ""),
    ("UnpackSound", 0x63d7b468, 0x64, "audio", ""),
    ("PrepSoundLoop", 0x63d7b468, 0x68, "audio", ""),
    ("TriggerSound", 0x63d7b468, 0x6c, "audio", ""),
    ("ActiveLoopSound", 0x63d7b468, 0x70, "audio", ""),
    ("AwardXPForTriggering", 0x23e650b8, 0x74, "int", "0"),
    ("SkillPointsForTriggering", 0x23e650b8, 0x78, "int", "-1"),
    ("UnpackingVariation", 0x23e650b8, 0x7c, "int", "0"),
    ("FreezeAfterTriggerDuration", 0x998669e8, 0x80, "time", "0s"),
    ("RequiredConditions", 0x041f5a89, 0x84, "conditions", ""),
    ("RejectedConditions", 0x041f5a89, 0x88, "conditions", ""),
    ("SetObjectStatusOnTrigger", 0xa7bd811e, 0x8c, "status", ""),
    ("ClearObjectStatusOnExit", 0xa7bd811e, 0xa8, "status", ""),
    ("ContactPointOverride", 0x1a2933aa, 0xc4, "string", ""),
    (
        "TriggerAttributeModifier",
        0x09ddcff3,
        0xcc,
        "ref",
        "AttributeModifier",
    ),
    ("AttributeModifierDuration", 0x998669e8, 0xd0, "time", "0s"),
    ("ChainedButton", 0xe0e2a81a, 0xd4, "ref", "LogicCommand"),
    ("StartRechargeOnExit", 0x77e3f4f7, 0xec, "bool", "false"),
    (
        "RequireAndSpendTiberiumOnCaster",
        0x763177a4,
        0xd8,
        "uint",
        "0",
    ),
    ("GoIdleInStartPreparation", 0x77e3f4f7, 0xed, "bool", "true"),
    ("FaceTarget", 0x77e3f4f7, 0xee, "bool", "true"),
    (
        "DisabledTypesToProcess",
        0x196570c9,
        0xdc,
        "disabled",
        "HELD",
    ),
    (
        "DisabledTypesToContinueSoundsFor",
        0x196570c9,
        0xe0,
        "disabled",
        "",
    ),
    ("ActiveModelCondition", 0x15723e0b, 0xe4, "model", ""),
];

const REPLACE_FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    ("NewObjectUnpackTime", 0x998669e8, 0xf0, "time", "0s"),
    (
        "ReplaceOptions",
        0x63591cba,
        0xf4,
        "replace",
        "DISABLE_DURING_REPLACE",
    ),
    ("ClearTriggerDistance", 0x645864f3, 0xf8, "real", "120"),
];

const WEAPON_FIELDS: &[(&str, u32, usize, &str, &str)] = &[
    ("SpecialWeapon", 0xd046_dd22, 0xf0, "ref", "WeaponTemplate"),
    ("WhichSpecialWeapon", 0x23e6_50b8, 0xf4, "int", "0"),
    ("SkipContinue", 0x77e3_f4f7, 0x108, "bool", "false"),
    ("BusyForDuration", 0x9986_69e8, 0xf8, "time", "0s"),
    ("NeedLivingTargets", 0x77e3_f4f7, 0x109, "bool", "false"),
    ("PlayWeaponPreFireFX", 0x77e3_f4f7, 0x10a, "bool", "false"),
    (
        "OffsetTargetLocationFromObject",
        0x77e3_f4f7,
        0x10b,
        "bool",
        "false",
    ),
    ("WeaponSlotID", 0x23e6_50b8, 0xfc, "int", "1"),
    (
        "WeaponSlotType",
        0x1848_c549,
        0x100,
        "slot",
        "PRIMARY_WEAPON",
    ),
    (
        "DestealthClosestObjectMatchingFilter",
        0x77e3_f4f7,
        0x10c,
        "bool",
        "false",
    ),
    (
        "ModelConditionsOnUnpack",
        0x1572_3e0b,
        0x104,
        "model",
        "INVALID",
    ),
];

fn flags(value: &str, names: &[&str], path: &std::path::Path) -> io::Result<u32> {
    value.split_ascii_whitespace().try_fold(0, |bits, name| {
        super::super::global_ui::enum_value(name, names, path).map(|index| bits | 1 << index)
    })
}

fn options(value: &str, path: &std::path::Path) -> io::Result<u32> {
    static OPTIONS: OnceLock<Vec<String>> = OnceLock::new();
    let names = OPTIONS.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/Modules/SpecialAbilityUpdate.xsd"
        ))
        .unwrap();
        schema
            .descendants()
            .find(|n| n.attribute("name") == Some("SpecialAbilityUpdateOptionsType"))
            .unwrap()
            .descendants()
            .filter_map(|n| n.attribute("value").map(str::to_owned))
            .collect()
    });
    value.split_ascii_whitespace().try_fold(0, |bits, name| {
        names
            .iter()
            .position(|candidate| candidate == name)
            .map(|index| bits | 1 << index)
            .ok_or_else(|| invalid(path, format!("invalid special ability option {name}")))
    })
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let weapon = node.has_tag_name((SAGE_NS, "WeaponFireSpecialAbilityUpdate"));
    let (kind, size, variant) = if weapon {
        (0x1e45_a393, 0x110, WEAPON_FIELDS)
    } else {
        (KIND, 0x104, REPLACE_FIELDS)
    };
    let all_fields = FIELDS.iter().chain(variant);
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(all_fields.clone().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut ability = vec![module_base(node, kind, start, data, true)?];
    let mut derived = Vec::new();
    // Reference ordinals follow the source attribute order.
    for attr in node.attributes() {
        let Some(&(_, _, offset, storage @ ("ref" | "weak" | "audio"), target)) =
            all_fields.clone().find(|&&(name, ..)| name == attr.name())
        else {
            continue;
        };
        let value = attr.value();
        if value.trim().is_empty() {
            continue;
        }
        let word = if storage == "audio" {
            super::client_behaviors::audio_reference(value, path, references, reference_types)?
        } else {
            let plain = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(plain, target, path)?;
            reference_types.push(id.type_id);
            if storage == "weak" {
                id.instance_id
            } else {
                references.push(id.into());
                references.len() as u32
            }
        };
        data.word(start + offset, word);
    }
    for &(name, tag, offset, storage, default) in all_fields {
        let value = node.attribute(name).unwrap_or(default);
        let offset = start + offset;
        match storage {
            "ref" | "weak" | "audio" => {}
            "string" => {
                if let Some(value) = node.attribute(name) {
                    data.string(offset, value)?;
                }
            }
            "bool" => data.payload[offset] = boolean(node, name, default == "true", path)?,
            "status" => write_status_flags(value, offset, data)?,
            storage => {
                let word = match storage {
                    "int" => value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?
                        as u32,
                    "uint" => value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?,
                    "real" => finite_number(value, name, path)?.to_bits(),
                    "time" => sage_time(value, name, path)?.to_bits(),
                    "options" => options(value, path)?,
                    "conditions" => flags(value, &["MOUNTED", "WEAPON_TOGGLE", "MOVING"], path)?,
                    "disabled" => flags(value, super::production::DISABLED, path)?,
                    "slot" => super::behaviors_basic::weapon_slot(value, path)?,
                    "model" if value.is_empty() => 0,
                    "model" if value == "INVALID" => u32::MAX,
                    "model" => model_condition_index(value, path)? as u32,
                    "replace" => flags(
                        value,
                        &[
                            "COPY_TARGET_OBJECT",
                            "IGNORE_TERRAIN_RESTRICTIONS",
                            "COPY_UPGRADES",
                            "DISABLE_DURING_REPLACE",
                            "CHECK_BUILD_ASSISTANT",
                            "DISABLE_NEW_OBJECT_DURING_UNPACK",
                            "CLEAR_LOCATION",
                        ],
                        path,
                    )?,
                    _ => unreachable!("unknown special ability field storage"),
                };
                data.word(offset, word);
            }
        }
        let entry = Entry::named(name, tag, Target::Data(offset));
        if offset >= start + 0xf0 {
            derived.push(entry);
        } else {
            ability.push(entry);
        }
    }

    let mut children = significant_children(node).peekable();
    let animation =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration"))) {
            Some(child) => Target::Table(encode_anim(child, start + 0xe8, data)?),
            None => Target::Null,
        };
    ability.push(Entry::named(
        "CustomAnimAndDuration",
        0x6b74_6af6,
        animation,
    ));
    let mut entries = vec![base(0xac59_8f82, Table::record(ability))];
    entries.extend(derived);
    if weapon {
        if children.next().is_some() {
            return Err(invalid(
                path,
                "unexpected WeaponFireSpecialAbilityUpdate child",
            ));
        }
        return Ok((kind, Table::record(entries)));
    }
    let mut ids = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "ReplacementTemplate")) {
            return Err(invalid(path, "unexpected ReplaceSelfSpecialAbility child"));
        }
        ids.push(weak_reference(child, "GameObject", data, reference_types)?);
    }
    let replacements = id_list(data, start + 0xfc, ids)?;
    entries.push(Entry::named(
        "ReplacementTemplate",
        0x148e_d75d,
        Target::Table(replacements),
    ));
    Ok((kind, Table::record(entries)))
}
