use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields, model_condition_index};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

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

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    const SPECIAL_POWER: u32 = 0xcefe_1113;
    let (kind, size, derived) = match node.tag_name().name() {
        "StoreObjectsSpecialPower" => (0xa74a_234c, 0x1d4, STORE_FIELDS),
        "TeleportStoredObjectsSpecialPower" => (0x318d_c4d6, 0x1d0, TELEPORT_FIELDS),
        "RecallUnitsSpecialPower" => (KIND, 0x1cc, RECALL_FIELDS),
        "AssignSlavesTargetObjectSpecialPower" => (0x9daf_d38c, 0x1c4, &[][..]),
        _ => (SPECIAL_POWER, 0x1c4, &[][..]),
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
    for &(name, kind, offset) in POWER_FIELDS.iter().chain(derived) {
        match kind {
            BOOL => {
                let default = matches!(
                    name,
                    "AffectAllies" | "AvailableAtStart" | "PlayReturnVoiceFromRecalledUnit"
                );
                data.payload[start + offset] = boolean(node, name, default, path)?;
            }
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
            "OCL" => (
                "ObjectCreationList",
                if kind == 0xa74a_234c { 0x1cc } else { 0x1c8 },
            ),
            "TargetMarkerObjectRef" => ("GameObject", 0x1d0),
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
            "OnTriggerRechargeSpecialPower" | "TargetMarkerObjectRef"
        ) {
            id.instance_id
        } else {
            references.push(id.into());
            references.len() as u32
        };
        data.word(start + offset, word);
    }

    let names: Vec<_> = node
        .attribute("RequirementsFilterMPSkirmish")
        .unwrap_or("")
        .split_ascii_whitespace()
        .collect();
    let count = start + 0x20;
    let first = data.list(count, names.len(), 8)?;
    let mut strings = Vec::new();
    for (index, name) in names.into_iter().enumerate() {
        let offset = first + index * 8;
        data.string(offset, name)?;
        strings.push((0, Target::Data(offset)));
    }
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut power = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    let mut strings = Some(Table::array(count, strings));
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
    power.push(Entry::named("InitiateSound", 0x1ae6_b6b7, Target::Null));
    if let Some(child) = children.next() {
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
    entries.extend(fields(start, derived));
    Ok((kind, Table::record(entries)))
}

fn empty_filter(start: usize) -> Table {
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
