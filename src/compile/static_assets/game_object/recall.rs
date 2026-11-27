use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, percentage, sage_time,
    significant_children, validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields, model_condition_index};
use super::special_power_variants::{
    encode_initiate_sound, string_list, variant_fields, variant_filter, variant_layout,
};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0x4e2d_96c9;
const BOOL: u32 = 0x77e3_f4f7;
const REAL: u32 = 0x6458_64f3;
const INT: u32 = 0x23e6_50b8;
const FX: u32 = 0x0d1c_8e83;
const TEMPLATE: &str = "SpecialPowerTemplate";
const FILTER: &str = "CanAffectObjectFilter";

// Field order in the token table follows the schema, while the offsets follow
// the runtime record layout. The three embedded filters have their own tables.
const POWER_FIELDS: &[(&str, u32, usize)] = &[
    (TEMPLATE, 0xaee8_3888, 0x08),
    ("UpdateModuleStartsAttack", BOOL, 0x1b4),
    ("StartsPaused", BOOL, 0x1b5),
    ("AntiFX", FX, 0x0c),
    ("AttributeModifier", 0x09dd_cff3, 0x10),
    ("AttributeModifierRange", REAL, 0x14),
    ("AttributeModifierAffectsSelf", BOOL, 0x1b6),
    ("AttributeModifierFX", FX, 0x18),
    ("AttributeModifierWeatherBased", BOOL, 0x1b7),
    ("WeatherDuration", 0x6f6d_1ae7, 0x1c),
    ("RequirementsFilterMPSkirmish", 0x6d66_ae18, 0x20),
    ("InitiateFX", FX, 0x28),
    ("TriggerFX", FX, 0x2c),
    ("SetModelCondition", 0x1572_3e0b, 0x30),
    ("SetModelConditionTime", 0x9986_69e8, 0x34),
    ("GiveLevels", INT, 0x38),
    ("DisableDuringAnimDuration", BOOL, 0x1b8),
    ("IdleWhenStartingPower", BOOL, 0x1b9),
    ("AffectGood", BOOL, 0x1ba),
    ("AffectEvil", BOOL, 0x1bb),
    ("AffectAllies", BOOL, 0x1bc),
    ("AvailableAtStart", BOOL, 0x1bd),
    ("ChangeWeather", 0xc500_6c0f, 0x3c),
    ("AdjustVictim", BOOL, 0x1be),
    ("OnTriggerRechargeSpecialPower", 0x40bb_ffb2, 0x40),
    ("BurnDecayModifier", 0x7631_77a4, 0x44),
    ("UseDistanceFromCommandCenter", BOOL, 0x1bf),
    ("MaxDistanceFromCommandCenter", REAL, 0x48),
    ("ChildModuleHandlesFX", BOOL, 0x1c0),
    ("LeechRechargePercentPlayerTechTrigger", 0xdbbf_0fdf, 0x4c),
    ("LeechRechargePercentFromSpecialPower", 0x9631_1b79, 0x50),
    ("DisabledTypesToIgnore", 0x1965_70c9, 0x54),
    (FILTER, 0x1e59_2b76, 0x58),
    ("ObjectFilterDistType", 0x9f0d_a607, 0x5c),
];
const RECALL_FIELDS: &[(&str, u32, usize)] = &[
    ("PlayReturnVoiceFromRecalledUnit", BOOL, 0x1c8),
    ("MaxAircraftToRecall", INT, 0x1c4),
];

const STORE_FIELDS: &[(&str, u32, usize)] = &[
    ("Radius", REAL, 0x1c4),
    ("TeleportLinkID", INT, 0x1c8),
    ("OCL", 0x42e4_e9d1, 0x1cc),
    ("TargetMarkerObjectRef", 0x148e_d75d, 0x1d0),
];
const TELEPORT_FIELDS: &[(&str, u32, usize)] = &[
    ("GetObjectsCurrentlyAtStorePosition", BOOL, 0x1cc),
    ("StoreObjectsLinkID", INT, 0x1c4),
    ("OCL", 0x42e4_e9d1, 0x1c8),
];

const MOVEMENT_POWER: u32 = 0x6712_612b;
const MOVEMENT_FIELDS: &[(&str, u32, usize)] = &[
    ("MovementType", 0x1e43_cb28, 0x1c4),
    ("Duration", 0x9986_69e8, 0x1c8),
    ("ModelCondition", 0x67db_bfc9, 0x1cc),
    ("ObjectStatus", 0xa7bd_811e, 0x1d0),
    ("LocomotorSet", 0x845d_12b2, 0x1d4),
    ("BusyDurationOnExit", 0x9986_69e8, 0x1d8),
    ("CollisionBounceHeight", REAL, 0x1dc),
    ("CollisionBounceDistance", REAL, 0x1e0),
    ("CollisionBounceSpeedScalar", REAL, 0x1e4),
];

const OCL_POWER: u32 = 0x8999_c6a4;
const OCL_FIELDS: &[(&str, u32, usize)] = &[
    ("OCL", 0x42e4_e9d1, 0x1c4),
    ("CreateLocation", 0xe5c0_9280, 0x1c8),
    ("CreateLocationOffset", REAL, 0x1cc),
    ("MaxCreateCount", 0x7631_77a4, 0x1d0),
    ("NumberToSpawn", 0x7631_77a4, 0x1d4),
    ("DestinationOCL", 0x42e4_e9d1, 0x1d8),
    ("RegisterObjectsWithSpecialAbilityUpdate", BOOL, 0x1f0),
];

const SPECIAL_POWER: u32 = 0xcefe_1113;

pub(super) fn base_layout_variant(name: &str) -> Option<u32> {
    Some(match name {
        "SpecialPower" => SPECIAL_POWER,
        "ReturnToDockSpecialPower" => 0x07fe_a222,
        "ReturnToProducerSpecialPower" => 0x2800_4ab1,
        "AssignSlavesTargetObjectSpecialPower" => 0x9daf_d38c,
        "AttackTargetSpecialPower" => 0x239d_954f,
        "PrematureDischargeSpecialPower" => 0x8e22_e8c8,
        "HordeDispatchSpecialPower" => 0x90a9_315b,
        "MoveToPositionAndEvacuateSpecialPower" => 0xac7e_2a4a,
        "MoveToPositionAndReplaceSelfSpecialPower" => 0xd679_1f61,
        "RepairSpecialPower" => 0x39b8_25ff,
        "SpecialPowerTimerRefreshSpecialPower" => 0x5420_3fa0,
        "UncombineSpecialPower" => 0x7c29_8d04,
        _ => return None,
    })
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    const PRODUCTION_BONUS: u32 = 0x8ed3_3e53;
    const ACTIVATE_MODULE: u32 = 0x4856_3167;
    const PLAYER_UPGRADE: u32 = 0x005b_408e;
    const BONUS_FIELDS: &[(&str, u32, usize)] = &[
        ("BuildCostBonus", REAL, 0x1c4),
        ("BuildCostBonusAllies", REAL, 0x1c8),
        ("BuildTimeBonus", REAL, 0x1cc),
        ("BuildTimeBonusAllies", REAL, 0x1d0),
        ("HarvesterBonus", REAL, 0x1d4),
        ("HarvesterBonusAllies", REAL, 0x1d8),
    ];
    let name = node.tag_name().name();
    let variant = variant_layout(name);
    let (kind, size, derived) = match name {
        "OCLSpecialPower" => (OCL_POWER, 0x1f4, OCL_FIELDS),
        "StoreObjectsSpecialPower" => (0xa74a_234c, 0x1d4, STORE_FIELDS),
        "TeleportStoredObjectsSpecialPower" => (0x318d_c4d6, 0x1d0, TELEPORT_FIELDS),
        "RecallUnitsSpecialPower" => (KIND, 0x1cc, RECALL_FIELDS),
        "MovementSpecialPower" => (MOVEMENT_POWER, 0x1e8, MOVEMENT_FIELDS),
        "GiveProductionBonusSpecialPower" => (PRODUCTION_BONUS, 0x1dc, BONUS_FIELDS),
        "ActivateModuleSpecialPower" => (ACTIVATE_MODULE, 0x1cc, &[][..]),
        "PlayerUpgradeSpecialPower" => (PLAYER_UPGRADE, 0x1cc, &[][..]),
        _ => variant
            .or_else(|| base_layout_variant(name).map(|kind| (kind, 0x1c4, &[][..])))
            .ok_or_else(|| invalid(path, format!("unsupported special power module {name}")))?,
    };
    let allowed: Vec<_> = POWER_FIELDS
        .iter()
        .chain(derived)
        .map(|&(name, _, _)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    // EA allocates derived lists, masks, and filters before variable-length base fields.
    let derived_entries = if variant.is_some() {
        Some(variant_fields(node, start, derived, data, reference_types)?)
    } else {
        (kind == MOVEMENT_POWER)
            .then(|| movement_fields(node, start, data))
            .transpose()?
    };
    let scalar_fields = if derived_entries.is_some() {
        &[]
    } else {
        derived
    };
    for &(name, field_type, offset) in POWER_FIELDS.iter().chain(scalar_fields) {
        match field_type {
            BOOL => {
                let default = matches!(
                    name,
                    "AffectAllies" | "AvailableAtStart" | "PlayReturnVoiceFromRecalledUnit"
                );
                data.payload[start + offset] = boolean(node, name, default, path)?;
            }
            REAL if kind == PRODUCTION_BONUS && offset >= 0x1c4 => data.word(
                start + offset,
                percentage(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
            ),
            REAL => data.word(
                start + offset,
                finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
            ),
            INT => {
                let default = if name == "MaxAircraftToRecall" {
                    "4"
                } else {
                    "0"
                };
                let value = node
                    .attribute(name)
                    .unwrap_or(default)
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?;
                data.word(start + offset, value as u32);
            }
            _ => {}
        }
    }
    if kind == OCL_POWER {
        if let Some(value) = node.attribute("CreateLocation") {
            static LOCATIONS: OnceLock<Vec<String>> = OnceLock::new();
            let locations = LOCATIONS.get_or_init(|| {
                super::ai_update::enum_values(
                    include_str!("../../../../schemas/ra3/xsd/Modules/OCLSpecialPower.xsd"),
                    "OCLCreateLocType",
                )
            });
            let index = locations
                .iter()
                .position(|candidate| candidate == value)
                .ok_or_else(|| invalid(path, "invalid OCL CreateLocation"))?;
            data.word(start + 0x1c8, index as u32);
        }
        for (name, default, offset) in [
            ("MaxCreateCount", "0", 0x1d0),
            ("NumberToSpawn", "1", 0x1d4),
        ] {
            let value = node
                .attribute(name)
                .unwrap_or(default)
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?;
            data.word(start + offset, value);
        }
    }
    let time = sage_time(
        node.attribute("SetModelConditionTime").unwrap_or("0s"),
        "SetModelConditionTime",
        path,
    )?;
    data.word(start + 0x34, time.to_bits());
    let decay = node
        .attribute("BurnDecayModifier")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid BurnDecayModifier"))?;
    data.word(start + 0x44, decay);
    use super::super::global_ui::enum_value;
    let mut disabled = 0;
    for name in node
        .attribute("DisabledTypesToIgnore")
        .unwrap_or("HELD")
        .split_ascii_whitespace()
    {
        disabled |= 1 << enum_value(name, super::production::DISABLED, path)?;
    }
    data.word(start + 0x54, disabled);
    if let Some(state) = node.attribute("SetModelCondition") {
        let value = if state == "INVALID" {
            u32::MAX
        } else {
            model_condition_index(state, path)? as u32
        };
        data.word(start + 0x30, value);
    }
    for (name, offset, options) in [
        (
            "ChangeWeather",
            0x3c,
            [
                "NORMAL",
                "CLOUDY",
                "RAINY",
                "CLOUDYRAINY",
                "SUNNY",
                "SNOWY",
                "INVALID",
            ]
            .as_slice(),
        ),
        (
            "ObjectFilterDistType",
            0x5c,
            ["CENTER_2D", "CIRCLE", "CENTER_3D", "SPHERE", "EXTENTS_3D"].as_slice(),
        ),
    ] {
        if let Some(value) = node.attribute(name) {
            data.word(start + offset, enum_value(value, options, path)?);
        }
    }
    if let Some(value) = node.attribute("WeatherDuration") {
        data.word(
            start + 0x1c,
            value
                .trim()
                .parse()
                .map_err(|_| invalid(path, "invalid WeatherDuration"))?,
        );
    }
    required(node, TEMPLATE, path)?;
    if name == "InvisibilitySpecialPower" {
        required(node, "InvisibilityTemplate", path)?;
    }
    for (name, value) in node.attributes().map(|a| (a.name(), a.value())).chain(
        (node.attribute(FILTER).is_none()).then_some((FILTER, "GenericSpecialPowerObjectFilter")),
    ) {
        let (kind, offset) = match name {
            TEMPLATE => ("SpecialPowerTemplate", 8),
            "AntiFX" => ("FXList", 0xc),
            "AttributeModifier" => ("AttributeModifier", 0x10),
            "AttributeModifierFX" => ("FXList", 0x18),
            "InitiateFX" => ("FXList", 0x28),
            "TriggerFX" => ("FXList", 0x2c),
            "OnTriggerRechargeSpecialPower" => ("SpecialPowerTemplate", 0x40),
            "LeechRechargePercentPlayerTechTrigger" => ("PlayerTech", 0x4c),
            "LeechRechargePercentFromSpecialPower" => ("SpecialPowerTemplate", 0x50),
            FILTER => ("ObjectFilterAsset", 0x58),
            "OCL" | "DestinationOCL" => {
                let offset = derived
                    .iter()
                    .find(|&&(field, _, _)| field == name)
                    .unwrap()
                    .2;
                ("ObjectCreationList", offset)
            }
            "TargetMarkerObjectRef" => ("GameObject", 0x1d0),
            "CommandSet" => ("LogicCommandSet", 0x1c4),
            "InvisibilityTemplate" => ("InvisibilityTemplate", 0x1c4),
            _ => continue,
        };
        if value.trim().is_empty() {
            continue;
        }
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, kind, path)?;
        reference_types.push(id.type_id);
        let word = if matches!(
            name,
            "OnTriggerRechargeSpecialPower" | "TargetMarkerObjectRef" | "CommandSet"
        ) {
            id.instance_id
        } else {
            references.push(id.into());
            references.len() as u32
        };
        data.word(start + offset, word);
    }

    let mut strings = Some(string_list(
        node.attribute("RequirementsFilterMPSkirmish").unwrap_or(""),
        start + 0x20,
        data,
    )?);
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut power = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    power.extend(POWER_FIELDS.iter().map(|&(name, kind, offset)| {
        let target = if name == "RequirementsFilterMPSkirmish" {
            Target::Table(strings.take().unwrap())
        } else {
            Target::Data(start + offset)
        };
        Entry::named(name, kind, target)
    }));
    let mut children = significant_children(node).peekable();
    for (name, offset) in [
        ("AttributeModifierAffects", 0x60),
        ("RequirementsFilterMP", 0xd0),
        ("RequirementsFilterStrategic", 0x140),
    ] {
        power.push(Entry::named(
            name,
            0xe46e_a3b2,
            Target::Table(
                match children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                    Some(child) => super::parking::encode_inline_filter(
                        child,
                        start + offset,
                        data,
                        reference_types,
                    )?,
                    None => empty_filter(start + offset),
                },
            ),
        ));
    }
    let sound = if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "InitiateSound")))
    {
        encode_initiate_sound(child, start + 0x1b0, data, references, reference_types)?
    } else {
        Target::Null
    };
    power.push(Entry::named("InitiateSound", 0x1ae6_b6b7, sound));
    if let Some((name, _)) = variant_filter(node.tag_name().name()) {
        children.next_if(|child| child.has_tag_name((SAGE_NS, name)));
    }
    if !matches!(kind, OCL_POWER | PLAYER_UPGRADE | ACTIVATE_MODULE)
        && let Some(child) = children.next()
    {
        return Err(invalid(
            path,
            format!(
                "unsupported special power child {}",
                child.tag_name().name()
            ),
        ));
    }
    if kind == SPECIAL_POWER {
        return Ok((kind, Table::record(power)));
    }
    let mut entries = vec![base(SPECIAL_POWER, Table::record(power))];
    if let Some(derived_entries) = derived_entries {
        entries.extend(derived_entries);
    } else {
        entries.extend(fields(start, derived));
    }
    if kind == PLAYER_UPGRADE {
        let mut upgrades = Vec::new();
        for child in children.by_ref() {
            if !child.has_tag_name((SAGE_NS, "Upgrade")) {
                return Err(invalid(path, "expected PlayerUpgradeSpecialPower Upgrade"));
            }
            let value = super::super::text_value(child, path)?;
            upgrades.push(super::passive_area::reference(
                value,
                "UpgradeTemplate",
                path,
                references,
                reference_types,
            )?);
        }
        let table = super::spawn::id_list(data, start + 0x1c4, upgrades)?;
        entries.push(Entry::named("Upgrade", 0x4aba_1f29, Target::Table(table)));
    }
    if kind == ACTIVATE_MODULE {
        entries.push(super::replace_self::module_list(
            children,
            start + 0x1c4,
            data,
            "Modules",
            true,
        )?);
        return Ok((kind, Table::record(entries)));
    }
    if kind == OCL_POWER {
        entries.extend(ocl_children(
            &mut children,
            start,
            data,
            references,
            reference_types,
        )?);
    }
    Ok((kind, Table::record(entries)))
}

fn movement_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Vec<Entry>> {
    use super::super::global_ui::enum_value;
    let path = data.path;
    let mut entries = Vec::with_capacity(MOVEMENT_FIELDS.len());
    let defaults = ["NONE", "0s", "", "", "INVALID", "0s", "20", "50", "0.5"];
    for (&(name, kind, offset), default) in MOVEMENT_FIELDS.iter().zip(defaults) {
        let field = start + offset;
        let explicit = node.attribute(name);
        let value = explicit.unwrap_or(default);
        let word = match name {
            "ModelCondition" | "ObjectStatus" => {
                let target = match explicit {
                    Some(value) => {
                        let flags = if name == "ModelCondition" {
                            let flags = data.allocate(field, 0x3c)?;
                            super::draws::write_model_conditions(value, flags, data)?;
                            flags
                        } else {
                            let flags =
                                data.allocate(field, super::secondary_damage::STATUS_BYTES)?;
                            super::secondary_damage::write_status_flags(value, flags, data)?;
                            flags
                        };
                        Target::Data(flags)
                    }
                    None => Target::Null,
                };
                entries.push(Entry::named(name, kind, target));
                continue;
            }
            "MovementType" => enum_value(value, &["NONE", "STRAIGHT_AHEAD", "TARGETED"], path)?,
            "LocomotorSet" => super::ai_update::locomotor_condition(value, name, path)?,
            "Duration" | "BusyDurationOnExit" => sage_time(value, name, path)?.to_bits(),
            _ => finite_number(value, name, path)?.to_bits(),
        };
        data.word(field, word);
        entries.push(Entry::named(name, kind, Target::Data(field)));
    }
    Ok(entries)
}

fn ocl_children<'a, 'input: 'a>(
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<[Entry; 3]> {
    let path = data.path;
    let mut pairs = Vec::new();
    while let Some(child) = children.next_if(|node| node.has_tag_name((SAGE_NS, "UpgradeOCL"))) {
        validate_attributes(child, &["OCL"], path)?;
        if super::super::has_content(child) {
            return Err(invalid(path, "unexpected UpgradeOCL content"));
        }
        pairs.push(super::passive_area::reference(
            child.attribute("OCL").unwrap_or(""),
            "ObjectCreationList",
            path,
            references,
            reference_types,
        )?);
    }
    let pair_count = start + 0x1dc;
    let first = data.list(pair_count, pairs.len(), 4)?;
    let pair_items: Vec<_> = pairs
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| {
            let offset = first + index * 4;
            data.word(offset, ordinal);
            (
                0,
                Target::Table(Table::record(vec![Entry::named(
                    "OCL",
                    0x42e4_e9d1,
                    Target::Data(offset),
                )])),
            )
        })
        .collect();
    let name = "NearestSecondaryObjectFilter";
    let filter = if let Some(child) = children.next_if(|node| node.has_tag_name((SAGE_NS, name))) {
        let offset = data.allocate(start + 0x1e4, 0x70)?;
        Target::Table(super::parking::encode_inline_filter(
            child,
            offset,
            data,
            reference_types,
        )?)
    } else {
        Target::Null
    };
    let mut upgrades = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "Upgrade"))
            || child.children().any(|node| node.is_element())
        {
            return Err(invalid(path, "unexpected OCLSpecialPower child"));
        }
        validate_attributes(child, &[], path)?;
        let value: String = child
            .children()
            .filter(Node::is_text)
            .filter_map(|node| node.text())
            .collect();
        upgrades.push(super::passive_area::reference(
            &value,
            "UpgradeTemplate",
            path,
            references,
            reference_types,
        )?);
    }
    let upgrade_count = start + 0x1e8;
    let first = data.list(upgrade_count, upgrades.len(), 4)?;
    let upgrade_items: Vec<_> = upgrades
        .into_iter()
        .enumerate()
        .map(|(index, ordinal)| {
            let offset = first + index * 4;
            data.word(offset, ordinal);
            (0, Target::Data(offset))
        })
        .collect();
    Ok([
        Entry::named(
            "UpgradeOCL",
            0xa8b8_56f2,
            Target::Table(Table::array(pair_count, pair_items)),
        ),
        Entry::named(name, 0xe46e_a3b2, filter),
        Entry::named(
            "Upgrade",
            0x4aba_1f29,
            Target::Table(Table::array(upgrade_count, upgrade_items)),
        ),
    ])
}

pub(super) fn empty_filter(start: usize) -> Table {
    filter_table(start, [Vec::new(), Vec::new()])
}

pub(super) fn filter_table(start: usize, [included, excluded]: [Vec<(u32, Target)>; 2]) -> Table {
    let mut entries: Vec<_> = fields(
        start,
        &[
            ("id", 0x4d3b_3f19, 0),
            ("Rule", 0x0027_bb58, 4),
            ("Relationship", 0x5c89_81a9, 8),
            ("Alignment", 0x9ffd_43eb, 0xc),
            ("Include", 0x9afc_9193, 0x10),
            ("Exclude", 0x9afc_9193, 0x34),
        ],
    )
    .collect();
    entries.extend([
        Entry::named("StatusBitFlags", 0xa7bd_811e, Target::Null),
        Entry::named("StatusBitFlagsExclude", 0xa7bd_811e, Target::Null),
        Entry::named(
            "IncludeThing",
            0x9b19_049f,
            Target::Table(Table::array(start + 0x60, included)),
        ),
        Entry::named(
            "ExcludeThing",
            0x9b19_049f,
            Target::Table(Table::array(start + 0x68, excluded)),
        ),
    ]);
    Table::record(entries)
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn recall_units_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-recall.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            document.root_element().first_element_child().unwrap(),
            Path::new("game-object-recall.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-recall.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-recall.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-recall.manifest"
        ))
        .unwrap();
        assert_eq!(actual.payload.len(), bin.len() - 4);
        assert_eq!(
            actual
                .payload
                .iter()
                .zip(&bin[4..])
                .position(|(a, b)| a != b),
            None
        );
        assert_eq!(actual.relocations, relo[4..]);
        assert!(actual.imports.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.assets[0].references.iter())
        );
    }
}
