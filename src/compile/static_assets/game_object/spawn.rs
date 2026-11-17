use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    asset_reference, hash_name, sage_time, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, inherited};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0x0d91_6adf;
const INT: u32 = 0x23e6_50b8;
const TIME: u32 = 0x9986_69e8;
const BOOL: u32 = 0x77e3_f4f7;
const DAMAGE: u32 = 0x07fa_aea4;
pub(super) const UPGRADE: &[(&str, u32, usize)] = &[
    ("RequiresAllTriggers", BOOL, 0x1c),
    ("RequiresAllConflictingTriggers", BOOL, 0x1d),
    ("Permanent", BOOL, 0x1e),
    ("WaypointMode", BOOL, 0x1f),
    ("WaypointModeTerminal", BOOL, 0x20),
];
const SPAWN: &[(&str, u32, usize)] = &[
    ("SpawnNumberData", INT, 0x24),
    ("SpawnReplaceDelayData", TIME, 0x28),
    ("InitialBurst", INT, 0x2c),
    ("IsOneShotData", BOOL, 0x4c),
    ("CanReclaimOrphans", BOOL, 0x4d),
    ("AggregateHealth", BOOL, 0x4e),
    ("ExitByBudding", BOOL, 0x4f),
    ("SpawnedRequireSpawner", BOOL, 0x50),
    ("RespectCommandLimit", BOOL, 0x51),
    ("DamageTypesToPropagateToSlaves", DAMAGE, 0x30),
    ("FadeInTime", TIME, 0x38),
    ("KillSpawnBasedOnModelConditionState", BOOL, 0x52),
    ("ShareUpgrades", BOOL, 0x53),
    ("SpawnInsideBuilding", BOOL, 0x54),
    ("InitialDelay", TIME, 0x3c),
    ("CombineOnCreate", BOOL, 0x55),
    ("KillSpawnsOnDisabled", BOOL, 0x56),
    ("PassExperienceToSpawned", BOOL, 0x57),
    ("KillSpawnsOnCaptured", BOOL, 0x58),
    ("SpawnOnUnpack", BOOL, 0x59),
    ("SetPurchasePrice", BOOL, 0x5a),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().chain(SPAWN).map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut die = None;
    let mut templates = Vec::new();
    let mut triggers = Vec::new();
    let mut conflicts = Vec::new();
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "Die")) && die.is_none() {
            die = Some(child);
        } else if child.has_tag_name((SAGE_NS, "SpawnTemplate")) {
            templates.push(weak_reference(child, "GameObject", data, reference_types)?);
        } else if child.has_tag_name((SAGE_NS, "TriggeredBy")) {
            triggers.push(weak_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        } else if child.has_tag_name((SAGE_NS, "ConflictsWith")) {
            conflicts.push(weak_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        } else {
            return Err(invalid(
                path,
                format!(
                    "unsupported or repeated SpawnBehavior child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }
    let start = data.allocate(pointer, 0x5c)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, tag, offset) in UPGRADE.iter().chain(SPAWN) {
        let value = node.attribute(name);
        match tag {
            BOOL => {
                data.payload[start + offset] =
                    boolean(node, name, name == "WaypointModeTerminal", path)?
            }
            INT => {
                let word = value
                    .unwrap_or("0")
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?;
                data.word(start + offset, word as u32);
            }
            TIME => data.word(
                start + offset,
                sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
            ),
            DAMAGE => {
                let flags = damage_flags(value.unwrap_or("ALL"), data)?;
                data.word(start + offset, flags as u32);
                data.word(start + offset + 4, (flags >> 32) as u32);
            }
            _ => unreachable!(),
        }
    }
    let die = if let Some(node) = die {
        let offset = data.allocate(start + 0x40, 0x2c)?;
        Target::Table(encode_die_mux(node, offset, data)?)
    } else {
        Target::Null
    };
    let templates = id_list(data, start + 0x44, templates)?;
    let triggers = id_list(data, start + 0xc, triggers)?;
    let conflicts = id_list(data, start + 0x14, conflicts)?;

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut upgrade = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    upgrade.extend(fields(start, UPGRADE));
    upgrade.extend([
        Entry::named("CustomAnimAndDuration", 0x6b74_6af6, Target::Null),
        Entry::named("TriggeredBy", 0x26fa_cfc7, Target::Table(triggers)),
        Entry::named("ConflictsWith", 0x26fa_cfc7, Target::Table(conflicts)),
    ]);
    let mut entries = vec![base(0xae60_ae98, Table::record(upgrade))];
    entries.extend(fields(start, SPAWN));
    entries.extend([
        Entry::named("Die", 0x0084_001b, die),
        Entry::named("SpawnTemplate", 0x148e_d75d, Target::Table(templates)),
    ]);
    Ok((KIND, Table::record(entries)))
}

pub(in crate::compile::static_assets) fn weak_reference(
    node: Node<'_, '_>,
    kind: &str,
    data: &BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<u32> {
    validate_attributes(node, &[], data.path)?;
    if node.children().any(|child| child.is_element()) {
        return Err(invalid(
            data.path,
            format!("{} requires reference text", node.tag_name().name()),
        ));
    }
    let value: String = node
        .children()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .collect();
    let value = value
        .split_once('\\')
        .map_or(value.as_str(), |(name, _)| name);
    let id = asset_reference(value, kind, data.path)?;
    reference_types.push(id.type_id);
    Ok(id.instance_id)
}

pub(in crate::compile::static_assets) fn id_list(
    data: &mut BinaryData<'_>,
    count: usize,
    ids: Vec<u32>,
) -> io::Result<Table> {
    let start = data.list(count, ids.len(), 4)?;
    let mut entries = Vec::with_capacity(ids.len());
    for (index, id) in ids.into_iter().enumerate() {
        let offset = start + index * 4;
        data.word(offset, id);
        entries.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, entries))
}

pub(super) fn damage_flags(value: &str, data: &BinaryData<'_>) -> io::Result<u64> {
    static FLAGS: OnceLock<Vec<String>> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/Includes/DamageDef.xsd"
        ))
        .unwrap()
        .descendants()
        .find(|node| node.attribute("name") == Some("DamageType"))
        .unwrap()
        .descendants()
        .filter(|node| node.tag_name().name() == "enumeration")
        .filter_map(|node| node.attribute("value").map(str::to_owned))
        .collect()
    });
    let mut mask = 0;
    for name in value.split_ascii_whitespace() {
        if name == "ALL" {
            mask = u64::MAX;
        } else {
            let bit = flags
                .iter()
                .take(64)
                .position(|flag| flag == name)
                .ok_or_else(|| invalid(data.path, format!("unknown damage type {name}")))?;
            mask |= 1 << bit;
        }
    }
    Ok(mask)
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn spawn_behavior_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-spawn.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-spawn.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-spawn.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-spawn.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-spawn.manifest"
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
        assert!(actual.references.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
    }
}
