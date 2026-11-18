use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, module_base};
use super::draws::{boolean, model_condition_index};
use super::eject_passengers;
use super::secondary_damage::write_status_flags;
use super::spawn::{id_list, weak_reference};
use super::tech_upgrade::encode_anim;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::{io, sync::OnceLock};

const KIND: u32 = 0x9122_2181;
const TOGGLE: u32 = 0xed1a_e55c;
const ABILITY: u32 = 0xac59_8f82;
const CAPTURE: u32 = 0x3139_98b5;
const WEAPON: u32 = 0x1e45_a393;
const TOGGLE_FIELDS: &[(&str, u32, usize, &str, &str)] = &[(
    "ReloadAllWeaponsOnToggle",
    0x77e3_f4f7,
    0xf8,
    "bool",
    "true",
)];
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
    const ACTIVATE_MODULE: u32 = 0xcd33_bcee;
    let (kind, size, variant) = match node.tag_name().name() {
        "WeaponFireSpecialAbilityUpdate" => (WEAPON, 0x110, WEAPON_FIELDS),
        "ToggleStatusSpecialAbilityUpdate" => (TOGGLE, 0xfc, TOGGLE_FIELDS),
        "ActivateModuleSpecialAbilityUpdate" => (ACTIVATE_MODULE, 0xf8, &[][..]),
        "SpecialAbilityUpdate" => (ABILITY, 0xf0, &[][..]),
        "CaptureSpecialAbilityUpdate" => (CAPTURE, 0xf0, &[][..]),
        "EjectPassengersSpecialAbilityUpdate" => (
            eject_passengers::KIND,
            eject_passengers::SIZE,
            eject_passengers::FIELDS,
        ),
        "ReplaceSelfSpecialAbility" => (KIND, 0x104, REPLACE_FIELDS),
        name => {
            return Err(invalid(
                data.path,
                format!("unsupported special ability {name}"),
            ));
        }
    };
    let all_fields = FIELDS.iter().chain(variant);
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(all_fields.clone().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut children = significant_children(node).peekable();
    let animation_node =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration")));
    let ejection = if kind == eject_passengers::KIND {
        let offset =
            children.next_if(|child| child.has_tag_name((SAGE_NS, eject_passengers::BONE_OFFSET)));
        Some(eject_passengers::nested(node, offset, start, data)?)
    } else {
        None
    };
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
            "ref" | "weak" | "audio" | "preencoded" => {}
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

    let animation = match animation_node {
        Some(child) => Target::Table(encode_anim(child, start + 0xe8, data)?),
        None => Target::Null,
    };
    ability.push(Entry::named(
        "CustomAnimAndDuration",
        0x6b74_6af6,
        animation,
    ));
    if kind == ABILITY {
        if children.next().is_some() {
            return Err(invalid(path, "unexpected SpecialAbilityUpdate child"));
        }
        return Ok((kind, Table::record(ability)));
    }
    let mut entries = vec![base(ABILITY, Table::record(ability))];
    entries.extend(derived);
    entries.extend(ejection);
    if kind == TOGGLE {
        let states: Vec<_> = children.collect();
        if states.is_empty() {
            return Err(invalid(path, "expected ToggleState"));
        }
        let first = data.list(start + 0xf0, states.len(), 0x118)?;
        let mut items = Vec::with_capacity(states.len());
        for (index, state) in states.into_iter().enumerate() {
            if !state.has_tag_name((SAGE_NS, "ToggleState")) {
                return Err(invalid(path, "expected ToggleState"));
            }
            items.push((
                0,
                Target::Table(toggle_state(
                    state,
                    first + index * 0x118,
                    data,
                    references,
                    reference_types,
                )?),
            ));
        }
        entries.push(Entry::named(
            "ToggleState",
            0xfc29_4b1d,
            Target::Table(Table::array(start + 0xf0, items)),
        ));
        return Ok((kind, Table::record(entries)));
    }
    if kind == ACTIVATE_MODULE {
        entries.push(module_list(
            children,
            start + 0xf0,
            data,
            "SpecialPowerModules",
            false,
        )?);
        return Ok((kind, Table::record(entries)));
    }
    if matches!(kind, WEAPON | CAPTURE | eject_passengers::KIND) {
        if children.next().is_some() {
            return Err(invalid(
                path,
                format!("unexpected {} child", node.tag_name().name()),
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

fn toggle_state(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let fields = [
        ("AttributeModifier", 0x09dd_cff3, 0),
        ("ModelConditions", 0x67db_bfc9, 4),
        ("ObjectStatus", 0xa7bd_811e, 0x40),
        ("Upgrade", 0x4aba_1f29, 0x5c),
        ("ArmorSet", 0x2037_048f, 0x60),
        ("LocomotorSet", 0x845d_12b2, 0x64),
        ("AIUpdateModuleId", 0xbdf0_7965, 0x68),
        ("GenericLuaEventParameter", 0x1a29_33aa, 0x6c),
        ("EnterStateSound", 0x63d7_b468, 0x74),
        ("DisabledType", 0xbde9_5b5a, 0x78),
    ];
    validate_attributes(node, &fields.map(|(name, _, _)| name), path)?;
    for attr in node.attributes() {
        let value = attr.value();
        let (offset, kind) = match attr.name() {
            "AttributeModifier" => (0, "AttributeModifier"),
            "Upgrade" => (0x5c, "UpgradeTemplate"),
            "EnterStateSound" => {
                let ordinal = super::client_behaviors::audio_reference(
                    value,
                    path,
                    references,
                    reference_types,
                )?;
                data.word(start + 0x74, ordinal);
                continue;
            }
            _ => continue,
        };
        write_reference(
            value,
            kind,
            start + offset,
            data,
            references,
            reference_types,
        )?;
    }
    super::draws::write_model_conditions(
        node.attribute("ModelConditions").unwrap_or(""),
        start + 4,
        data,
    )?;
    write_status_flags(
        node.attribute("ObjectStatus").unwrap_or(""),
        start + 0x40,
        data,
    )?;
    let armor = node.attribute("ArmorSet").unwrap_or("INVALID");
    static ARMOR_TYPES: OnceLock<Vec<String>> = OnceLock::new();
    let armor_types = ARMOR_TYPES.get_or_init(|| {
        super::ai_update::enum_values(
            include_str!("../../../../schemas/ra3/xsd/AssetTypeArmorTemplate.xsd"),
            "ArmorSetType",
        )
    });
    let armor = armor_types
        .iter()
        .position(|v| v == armor)
        .ok_or_else(|| invalid(path, "invalid ArmorSet"))?;
    data.word(start + 0x60, armor as u32);
    data.word(
        start + 0x64,
        super::ai_update::locomotor_condition(
            node.attribute("LocomotorSet").unwrap_or("INVALID"),
            "LocomotorSet",
            path,
        )?,
    );
    if let Some(id) = node.attribute("AIUpdateModuleId") {
        data.word(start + 0x68, super::super::hash_name(id));
    }
    data.string(
        start + 0x6c,
        node.attribute("GenericLuaEventParameter").unwrap_or(""),
    )?;
    if let Some(value) = node.attribute("DisabledType") {
        data.word(
            start + 0x78,
            super::super::global_ui::enum_value(value, super::production::DISABLED, path)?,
        );
    }
    let mut children = significant_children(node);
    let Some(ai) = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "SkirmishAiInfo")))
    else {
        return Err(invalid(path, "ToggleState requires SkirmishAiInfo"));
    };
    let mut entries: Vec<_> = super::draws::fields(start, &fields).collect();
    entries.push(Entry::named(
        "SkirmishAiInfo",
        0xfe37_4ccd,
        Target::Table(toggle_ai(
            ai,
            start + 0x7c,
            data,
            references,
            reference_types,
        )?),
    ));
    entries.push(module_list(children, start + 0x110, data, "Module", false)?);
    Ok(Table::record(entries))
}

pub(super) fn module_list<'a, 'input: 'a>(
    children: impl Iterator<Item = Node<'a, 'input>>,
    count: usize,
    data: &mut BinaryData<'_>,
    name: &str,
    deactivation: bool,
) -> io::Result<Entry> {
    const MODULE_INFO: u32 = 0xada5_4d29;
    const FIELDS: &[(&str, u32, usize)] = &[
        ("ModuleId", 0xbdf0_7965, 0),
        ("TriggerAtTargetPosition", 0x77e3_f4f7, 8),
        ("TriggerType", 0x01c3_a3a2, 4),
    ];
    let path = data.path;
    let children: Vec<_> = children.collect();
    if children.len() > 4 {
        return Err(invalid(path, format!("{name} allows at most four entries")));
    }
    let size = if deactivation { 16 } else { 12 };
    let start = data.list(count, children.len(), size)?;
    let mut items = Vec::with_capacity(children.len());
    let allowed: Vec<_> = FIELDS
        .iter()
        .map(|&(name, ..)| name)
        .chain(deactivation.then_some("Deactivate"))
        .collect();
    for (index, node) in children.into_iter().enumerate() {
        if !node.has_tag_name((SAGE_NS, name)) || super::super::has_content(node) {
            return Err(invalid(path, format!("expected {name} entry")));
        }
        validate_attributes(node, &allowed, path)?;
        let offset = start + index * size;
        if let Some(id) = node.attribute("ModuleId").filter(|id| !id.is_empty()) {
            data.word(offset, super::super::hash_name(id));
        }
        let trigger = super::super::global_ui::enum_value(
            node.attribute("TriggerType").unwrap_or("TARGET_POSITION"),
            &["SELF_POSITION", "TARGET_POSITION", "TARGET_OBJECT"],
            path,
        )?;
        data.word(offset + 4, trigger);
        data.payload[offset + 8] = boolean(node, "TriggerAtTargetPosition", true, path)?;
        let fields = Table::record(super::draws::fields(offset, FIELDS).collect());
        let table = if deactivation {
            data.payload[offset + 12] = boolean(node, "Deactivate", false, path)?;
            Table::record(vec![
                base(MODULE_INFO, fields),
                Entry::named("Deactivate", 0x77e3_f4f7, Target::Data(offset + 12)),
            ])
        } else {
            fields
        };
        items.push((0, Target::Table(table)));
    }
    let kind = if deactivation {
        0x58f6_065c
    } else {
        MODULE_INFO
    };
    Ok(Entry::named(
        name,
        kind,
        Target::Table(Table::array(count, items)),
    ))
}

fn write_reference(
    value: &str,
    kind: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<()> {
    if !value.trim().is_empty() {
        let id = asset_reference(
            value.split_once('\\').map_or(value, |(name, _)| name),
            kind,
            data.path,
        )?;
        references.push(id.into());
        reference_types.push(id.type_id);
        data.word(offset, references.len() as u32);
    }
    Ok(())
}

fn toggle_ai(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const REAL: u32 = 0x6458_64f3;
    const BOOL: u32 = 0x77e3_f4f7;
    const INT: u32 = 0x23e6_50b8;
    const TIME: u32 = 0x9986_69e8;
    const POID: u32 = 0xbdf0_7965;
    const STATE: u32 = 0xd762_0aaa;
    const TASK: u32 = 0x74ba_c2ed;
    const UPGRADE: u32 = 0x4aba_1f29;
    const KIND_OF: u32 = 0x9afc_9193;
    const LOCATION: u32 = 0x73b8_b420;
    const FIELDS: &[(&str, u32, usize, Option<&str>)] = &[
        ("UseOnlyWhenPowerAdvantageIsAbove", REAL, 4, None),
        ("UseOnlyWhenPowerAdvantageIsBelow", REAL, 8, None),
        ("UseOnlyWhenCostAdvantageIsAbove", REAL, 0xc, None),
        ("UseOnlyWhenCostAdvantageIsBelow", REAL, 0x10, None),
        ("NeverUseWhenFurtherThanNUnitsFromTarget", REAL, 0x14, None),
        ("NeverUseWhenCloserThanNUnitsFromTarget", REAL, 0x18, None),
        ("NeverUseWhenHealthIsBelow", REAL, 0x1c, None),
        ("OnlyUseWhenHaveTarget", BOOL, 0x90, Some("false")),
        ("MaxZDifferenceFromTarget", REAL, 0x20, None),
        ("NeverUseInState", STATE, 0x24, Some("")),
        ("NeverUseDuringTask", TASK, 0x28, Some("")),
        ("NeverUseWhenClear", BOOL, 0x91, Some("false")),
        ("SelfDefenseDamageLookAheadTime", TIME, 0x2c, None),
        ("SelfDefenseDamageLookBehindTime", TIME, 0x30, None),
        ("SelfDefenseDamageFraction", REAL, 0x34, None),
        ("DontUseIfInRange", BOOL, 0x92, Some("false")),
        ("NumOtherAttackersOnTarget", INT, 0x38, Some("1")),
        ("LeechWeaponUpgrade", UPGRADE, 0x3c, Some("")),
        ("LeechWeaponTargetHealthFraction", REAL, 0x40, Some("0.1")),
        ("LeechWeaponSOLModuleId", POID, 0x44, None),
        ("LeechHealthActivatePowerAdvantage", REAL, 0x48, Some("1.7")),
        (
            "LeechHealthDeactivatePowerAdvantage",
            REAL,
            0x4c,
            Some("1.2"),
        ),
        ("StealthOthersInvisibilityModuleId", POID, 0x50, None),
        ("ShieldMinCostProtected", INT, 0x54, Some("0")),
        ("ShieldMinEnemyDPS", REAL, 0x58, Some("1000")),
        (
            "ShieldCostAdjustPastDistanceFraction",
            REAL,
            0x5c,
            Some("0.5"),
        ),
        ("ShieldCostAdjustMultiplierAtEdge", REAL, 0x60, Some("0")),
        ("EnemyMustBeInVisionRange", BOOL, 0x93, Some("false")),
        ("NeverUseWhenTargetIsAnyKindOf", KIND_OF, 0x64, Some("")),
        ("Location", LOCATION, 0x88, Some("Water Land")),
    ];
    const AI_SCHEMA: &str =
        include_str!("../../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAISpecialPower.xsd");
    const MICRO_SCHEMA: &str =
        include_str!("../../../../schemas/ra3/xsd/SkirmishAI/AssetTypeAIMicroManagerStates.xsd");
    static HINTS: OnceLock<Vec<String>> = OnceLock::new();
    static MICRO_STATES: OnceLock<Vec<String>> = OnceLock::new();
    static MICRO_TASKS: OnceLock<Vec<String>> = OnceLock::new();

    let path = data.path;
    let allowed: Vec<_> = std::iter::once("ToggleHint")
        .chain(FIELDS.iter().map(|&(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let hints =
        HINTS.get_or_init(|| super::ai_update::enum_values(AI_SCHEMA, "ToggleStatusAiHint"));
    let hint = required(node, "ToggleHint", path)?;
    data.word(
        start,
        super::super::ai_strategic::enum_index(hint, hints, "ToggleHint", path)?,
    );

    let mut entries = vec![Entry::named("ToggleHint", 0x018d_d8b9, Target::Data(start))];
    for &(name, kind, offset, default) in FIELDS {
        let pointer = start + offset;
        let target = if let Some(value) = node.attribute(name).or(default) {
            let field = if default.is_some() {
                pointer
            } else {
                data.allocate(pointer, 4)?
            };
            match kind {
                BOOL => data.payload[field] = boolean(node, name, false, path)?,
                INT => data.word(
                    field,
                    value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?
                        as u32,
                ),
                REAL => data.word(field, finite_number(value, name, path)?.to_bits()),
                TIME => data.word(field, sage_time(value, name, path)?.to_bits()),
                POID => data.word(field, super::super::hash_name(value)),
                UPGRADE => write_reference(
                    value,
                    "UpgradeTemplate",
                    field,
                    data,
                    references,
                    reference_types,
                )?,
                KIND_OF => {
                    for (index, word) in super::super::global_ui::kind_of_flags(value, path)?
                        .into_iter()
                        .enumerate()
                    {
                        data.word(field + index * 4, word);
                    }
                }
                STATE | TASK => {
                    let (cache, schema_type) = if kind == STATE {
                        (&MICRO_STATES, "MicroManagerState")
                    } else {
                        (&MICRO_TASKS, "MicroManagerUnitTask")
                    };
                    let options = cache
                        .get_or_init(|| super::ai_update::enum_values(MICRO_SCHEMA, schema_type));
                    let bits = super::super::ai_strategic::flags(value, options, name, path)?;
                    data.word(field, bits);
                }
                LOCATION => {
                    let bits =
                        super::super::ai_strategic::flags(value, &["Water", "Land"], name, path)?;
                    data.word(field, bits);
                }
                _ => unreachable!(),
            }
            Target::Data(field)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, kind, target));
    }
    let mut children = significant_children(node);
    let weapon = if let Some(weapon) = children.next() {
        if !weapon.has_tag_name((SAGE_NS, "StateWeapon"))
            || has_content(weapon)
            || children.next().is_some()
        {
            return Err(invalid(path, "unsupported SkirmishAiInfo child"));
        }
        let weapon_start = data.allocate(start + 0x8c, 0x1c)?;
        super::super::ai_command::write_weapon_fields(weapon, weapon_start, data)?;
        write_reference(
            required(weapon, "Weapon", path)?,
            "WeaponTemplate",
            weapon_start,
            data,
            references,
            reference_types,
        )?;
        Target::Table(Table::record(
            super::draws::fields(
                weapon_start,
                &[
                    ("Weapon", 0xd046_dd22, 0),
                    ("IsDisablingAttack", 0x77e3_f4f7, 0x18),
                    ("AffectsFriendlies", 0x77e3_f4f7, 0x19),
                    ("AffectsSelf", 0x77e3_f4f7, 0x1a),
                    ("TotalValueGreaterThan", 0x23e6_50b8, 4),
                    ("AoeAttackPowerLocationPreference", 0x0b3b_cae4, 8),
                    (
                        "AoeAttackPowerBlockAreaAfterActivationTime",
                        0x9986_69e8,
                        0xc,
                    ),
                    ("AoeAttackEmanationLocation", 0x3343_3965, 0x10),
                    ("ToggleRangeHysteresis", 0x6458_64f3, 0x14),
                    ("ToggleShortRangeTargetForcesTarget", 0x77e3_f4f7, 0x1b),
                ],
            )
            .collect::<Vec<_>>(),
        ))
    } else {
        Target::Null
    };
    entries.push(Entry::named("StateWeapon", 0x43db_a49d, weapon));
    Ok(Table::record(entries))
}
