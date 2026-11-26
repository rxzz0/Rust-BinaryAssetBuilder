use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, percentage, sage_time,
    significant_children, validate_attributes,
};
use super::animal_ai;
use super::parking::encode_inline_filter;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const AI_UPDATE: u32 = 0x08db_c00e;
const HORDE_AI_UPDATE: u32 = 0x55d7_7703;
const AI_GATE_UPDATE: u32 = 0x347c_ed1c;
const CONSTRUCTION_YARD_AI_UPDATE: u32 = 0x00bd_afa3;
const JET_AI_UPDATE: u32 = 0x4486_cf48;
const TIME: u32 = 0x9986_69e8;
const REAL: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;
const DEPLOY_STYLE_AI_UPDATE: u32 = 0xee30_dfd4;
const TRANSPORT_AI_UPDATE: u32 = 0xbe9c_d6fe;
const WANDER_AI_UPDATE: u32 = 0xd813_af2a;
const SUPPLY_TRUCK_AI_UPDATE: u32 = 0x9ab8_094d;
const UINT: u32 = 0x7631_77a4;
const MODEL_CONDITIONS: u32 = 0x67db_bfc9;
const ATTRIBUTE_MODIFIER: u32 = 0x09dd_cff3;
const DEPLOY_FIELDS: &[(&str, u32, usize)] = &[
    ("UnpackTime", TIME, 0x4c),
    ("PackTime", TIME, 0x50),
    ("ResetTurretBeforePacking", BOOL, 0x58),
    ("TurretsFunctionOnlyWhenDeployed", BOOL, 0x59),
    ("TurretsMustCenterBeforePacking", BOOL, 0x5a),
    ("MustDeployToAttack", BOOL, 0x5b),
    ("DeployedAttributeModifierName", ATTRIBUTE_MODIFIER, 0x54),
];
const WANDER_FIELDS: &[(&str, u32, usize)] = &[
    ("AttackAll", BOOL, 0x8c),
    ("EntryCondition", MODEL_CONDITIONS, 0x4c),
    ("Selectable", BOOL, 0x8d),
    ("WanderDistance", UINT, 0x88),
];
const SUPPLY_TRUCK_FIELDS: &[(&str, u32, usize)] = &[
    ("MaxBoxes", UINT, 0x4c),
    ("SupplyCenterActionDelay", TIME, 0x50),
    ("SupplyWarehouseActionDelay", TIME, 0x54),
    ("SupplyWarehouseScanDistance", REAL, 0x58),
    ("DepletedAnnounceDistance", REAL, 0x5c),
    ("HarvestTrees", BOOL, 0x6c),
    ("HarvestTiberium", BOOL, 0x6d),
    ("HarvestActivationRange", REAL, 0x60),
    ("HarvestPreparationTime", TIME, 0x64),
    ("HarvestActionTime", TIME, 0x68),
    ("HarvestsFromRearEnd", BOOL, 0x6e),
    ("CanPathThroughUnitsWhileDocking", BOOL, 0x6f),
    ("CanPathThroughUnitsWhileHarvesting", BOOL, 0x70),
];
const JET_FIELDS: &[(&str, u32, usize)] = &[
    ("OutOfAmmoDamagePerSecond", REAL, 0x4c),
    ("OutOfAmmoDamageDelay", TIME, 0x50),
    ("NeedsRunway", BOOL, 0x60),
    ("ReturnToBaseWhenVictimDead", BOOL, 0x61),
    ("KeepsParkingSpaceWhenAirborne", BOOL, 0x62),
    ("ParkingOffset", REAL, 0x54),
    ("CirclesForAttack", BOOL, 0x63),
    ("UseForOutOfAmmoCheck", 0xd046_dd22, 0x58),
    ("OutOfAmmoEvaEvent", 0xc4ec_43b1, 0x5c),
];

pub(super) fn encode(
    group: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    const AI_LIST: usize = 0x1d4;
    let modules: Vec<_> = significant_children(group).collect();
    if modules.len() > 2 {
        return Err(invalid(data.path, "AI accepts at most two modules"));
    }
    let pointers = data.list(AI_LIST, modules.len(), 4)?;
    let mut items = Vec::with_capacity(modules.len());
    for (index, node) in modules.into_iter().enumerate() {
        let tag = node.tag_name();
        let kind = match (tag.namespace(), tag.name()) {
            (Some(SAGE_NS), "AIUpdate") => AI_UPDATE,
            (Some(SAGE_NS), "AIGateUpdate") => AI_GATE_UPDATE,
            (Some(SAGE_NS), "ConstructionYardAIUpdate") => CONSTRUCTION_YARD_AI_UPDATE,
            (Some(SAGE_NS), "HordeAIUpdate") => HORDE_AI_UPDATE,
            (Some(SAGE_NS), "JetAIUpdate") => JET_AI_UPDATE,
            (Some(SAGE_NS), "DeployStyleAIUpdate") => DEPLOY_STYLE_AI_UPDATE,
            (Some(SAGE_NS), "TransportAIUpdate") => TRANSPORT_AI_UPDATE,
            (Some(SAGE_NS), "WanderAIUpdate") => WANDER_AI_UPDATE,
            (Some(SAGE_NS), "SupplyTruckAIUpdate") => SUPPLY_TRUCK_AI_UPDATE,
            (Some(SAGE_NS), "AnimalAIUpdate") => animal_ai::KIND,
            _ => {
                return Err(invalid(
                    data.path,
                    format!("unsupported AI module {}", tag.name()),
                ));
            }
        };
        let pointer = pointers + index * 4;
        let table = match kind {
            AI_GATE_UPDATE => encode_gate(node, pointer, data)?,
            CONSTRUCTION_YARD_AI_UPDATE => {
                super::behaviors_basic::fieldless_module(node, pointer, data, kind, true)?
            }
            _ => encode_update(node, kind, pointer, data, references, reference_types)?,
        };
        items.push((
            0,
            Target::Table(Table::sequence(vec![Entry::new(
                2,
                kind,
                Target::Table(table),
            )])),
        ));
    }
    Ok(Target::Table(Table::array(AI_LIST, items)))
}

fn encode_gate(node: Node<'_, '_>, pointer: usize, data: &mut BinaryData<'_>) -> io::Result<Table> {
    super::super::fx_list::validate_leaf(
        node,
        &["id", "TriggerWidthX", "TriggerWidthY"],
        data.path,
    )?;
    let start = data.allocate(pointer, 16)?;
    let mut entries = vec![super::behaviors_basic::module_base(
        node,
        AI_GATE_UPDATE,
        start,
        data,
        true,
    )?];
    for (name, offset) in [("TriggerWidthX", start + 8), ("TriggerWidthY", start + 12)] {
        write_real(node, name, "0", offset, data)?;
        entries.push(Entry::named(name, REAL, Target::Data(offset)));
    }
    Ok(Table::record(entries))
}

fn encode_update(
    node: Node<'_, '_>,
    kind: u32,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const FIELDS: &[(&str, u32, usize)] = &[
        ("AttackPriority", 0x1a29_33aa, 0x08),
        ("AutoAcquireEnemiesWhenIdle", 0x8b0f_b9da, 0x10),
        ("StopChaseDistance", REAL, 0x14),
        ("StandGround", BOOL, 0x48),
        ("MinCowerTime", TIME, 0x18),
        ("MaxCowerTime", TIME, 0x1c),
        ("BurningDeathTime", TIME, 0x20),
        ("ComboLocoAttackDistance", REAL, 0x24),
        ("SpecialContactPoints", 0x6d66_ae18, 0x28),
        ("FadeOnPortals", BOOL, 0x49),
        ("IdleTargetTime", TIME, 0x30),
        ("MaxCombineChildren", 0x7631_77a4, 0x34),
        ("SpawnOffsetRadius", REAL, 0x38),
        ("StateMachine", 0xac6e_35e0, 0x3c),
        ("DefaultLocomotorSet", 0x845d_12b2, 0x40),
    ];
    let (size, derived_fields) = match kind {
        JET_AI_UPDATE => (0x64, JET_FIELDS),
        DEPLOY_STYLE_AI_UPDATE => (0x5c, DEPLOY_FIELDS),
        WANDER_AI_UPDATE => (0x90, WANDER_FIELDS),
        SUPPLY_TRUCK_AI_UPDATE => (0x74, SUPPLY_TRUCK_FIELDS),
        animal_ai::KIND => (animal_ai::SIZE, animal_ai::FIELDS),
        _ => (0x4c, &[][..]),
    };
    let extra: Vec<_> = std::iter::once("id")
        .chain(derived_fields.iter().map(|&(name, _, _)| name))
        .collect();
    validate_fields(node, FIELDS, &extra, data.path)?;
    let chooser = optional_chooser(node, data.path)?;
    let start = data.allocate(pointer, size)?;
    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", data.path)?));
    if let Some(text) = node
        .attribute("AttackPriority")
        .filter(|text| !text.is_empty())
    {
        data.string(start + 8, text)?;
    }
    let acquire = flags(
        node,
        "AutoAcquireEnemiesWhenIdle",
        "",
        &[
            "YES",
            "STEALTHED",
            "NO",
            "NOTWHILEATTACKING",
            "ATTACK_BUILDINGS",
        ],
        data.path,
    )?;
    data.word(start + 0x10, acquire);
    for (name, default, offset) in [
        ("StopChaseDistance", "500", 0x14),
        ("ComboLocoAttackDistance", "80", 0x24),
        ("SpawnOffsetRadius", "0", 0x38),
    ] {
        write_real(node, name, default, start + offset, data)?;
    }
    for (name, offset) in [
        ("MinCowerTime", 0x18),
        ("MaxCowerTime", 0x1c),
        ("BurningDeathTime", 0x20),
        ("IdleTargetTime", 0x30),
    ] {
        write_time(node, name, "0s", start + offset, data)?;
    }
    let max_children = node
        .attribute("MaxCombineChildren")
        .unwrap_or("1")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(data.path, "invalid MaxCombineChildren"))?;
    data.word(start + 0x34, max_children);
    data.word(start + 0x3c, state_machine(node, data.path)?);
    data.word(start + 0x40, locomotor_set(node, data.path)?);
    write_bool(node, "StandGround", false, start + 0x48, data)?;
    write_bool(node, "FadeOnPortals", false, start + 0x49, data)?;
    if kind == JET_AI_UPDATE {
        encode_jet_fields(node, start, data, references, reference_types)?;
    } else if kind != animal_ai::KIND {
        for &(name, field_kind, offset) in derived_fields {
            let field = start + offset;
            match field_kind {
                TIME => write_time(node, name, "0s", field, data)?,
                REAL => {
                    let default = if name == "DepletedAnnounceDistance" {
                        "2000"
                    } else {
                        "0"
                    };
                    write_real(node, name, default, field, data)?;
                }
                BOOL => {
                    let default = matches!(
                        name,
                        "CanPathThroughUnitsWhileDocking" | "CanPathThroughUnitsWhileHarvesting"
                    );
                    write_bool(node, name, default, field, data)?;
                }
                UINT => {
                    let value = node
                        .attribute(name)
                        .unwrap_or("0")
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| invalid(data.path, format!("invalid {name}")))?;
                    data.word(field, value);
                }
                MODEL_CONDITIONS => {
                    super::draws::write_model_conditions(
                        node.attribute(name).unwrap_or(""),
                        field,
                        data,
                    )?;
                }
                ATTRIBUTE_MODIFIER => {
                    if let Some(value) = node.attribute(name) {
                        let index = super::passive_area::reference(
                            value,
                            "AttributeModifier",
                            data.path,
                            references,
                            reference_types,
                        )?;
                        data.word(field, index);
                    }
                }
                _ => unreachable!("unexpected derived AI field type"),
            }
        }
    }

    let points = node
        .attribute("SpecialContactPoints")
        .unwrap_or("")
        .split_ascii_whitespace();
    let strings = data.list(start + 0x28, points.clone().count(), 8)?;
    let mut point_items = Vec::new();
    for (index, point) in points.enumerate() {
        let offset = strings + index * 8;
        data.string(offset, point)?;
        point_items.push((0, Target::Data(offset)));
    }
    let mut point_items = Some(point_items);
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut fields = vec![base(
        0xf100_2505,
        Table::record(vec![base(
            0x2900_211d,
            Table::record(vec![base(
                0xa50d_729a,
                Table::record(vec![base(0xd75d_93a2, module)]),
            )]),
        )]),
    )];
    fields.extend(FIELDS.iter().map(|&(name, kind, offset)| {
        let target = if name == "SpecialContactPoints" {
            Target::Table(Table::array(start + offset, point_items.take().unwrap()))
        } else {
            Target::Data(start + offset)
        };
        Entry::named(name, kind, target)
    }));
    let chooser = match chooser {
        Some(node) => Target::Table(encode_chooser(
            node,
            start + 0x44,
            data,
            references,
            reference_types,
        )?),
        None => Target::Null,
    };
    fields.push(Entry::named(
        "UnitAITargetChooserData",
        0x243b_ef01,
        chooser,
    ));
    let table = Table::record(fields);
    match kind {
        AI_UPDATE => Ok(table),
        animal_ai::KIND => animal_ai::encode_fields(node, start, data, base(AI_UPDATE, table)),
        _ => {
            let mut entries = vec![base(AI_UPDATE, table)];
            entries.extend(super::draws::fields(start, derived_fields));
            Ok(Table::record(entries))
        }
    }
}

fn encode_jet_fields(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<()> {
    let damage = "OutOfAmmoDamagePerSecond";
    let value = percentage(node.attribute(damage).unwrap_or("0"), damage, data.path)?;
    data.word(start + 0x4c, value.to_bits());
    write_time(node, "OutOfAmmoDamageDelay", "0s", start + 0x50, data)?;
    write_real(node, "ParkingOffset", "0", start + 0x54, data)?;
    for &(name, kind, offset) in JET_FIELDS {
        if kind == BOOL {
            write_bool(node, name, false, start + offset, data)?;
        }
    }
    for attribute in node.attributes() {
        let (target, offset) = match attribute.name() {
            "UseForOutOfAmmoCheck" => ("WeaponTemplate", 0x58),
            "OutOfAmmoEvaEvent" => ("EvaEvent", 0x5c),
            _ => continue,
        };
        let index = super::passive_area::reference(
            attribute.value(),
            target,
            data.path,
            references,
            reference_types,
        )?;
        data.word(start + offset, index);
    }
    Ok(())
}

fn optional_chooser<'a, 'input>(
    node: Node<'a, 'input>,
    path: &Path,
) -> io::Result<Option<Node<'a, 'input>>> {
    let mut children = significant_children(node);
    let first = children.next();
    if first.is_some_and(|child| !child.has_tag_name((SAGE_NS, "UnitAITargetChooserData")))
        || children.next().is_some()
    {
        return Err(invalid(path, "unsupported AIUpdate child"));
    }
    Ok(first)
}

pub(super) fn encode_chooser(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const FIELDS: &[(&str, u32, usize)] = &[
        ("IdleScanDelay", TIME, 0),
        ("ReacquireDelay", TIME, 4),
        ("StartupDelay", TIME, 8),
        ("SympathyRange", REAL, 0xc),
        ("CheckVisionRange", BOOL, 0x1c),
        ("CanPickDynamicTargets", BOOL, 0x1d),
        ("CanPickDynamicTargetType", 0x344e_0f38, 0x10),
        ("RotateToTargetWhenAiming", BOOL, 0x1e),
        ("CanAutoAcquireNonAutoAcquirable", BOOL, 0x1f),
        ("CanChaseAttack", BOOL, 0x20),
        ("CanOnlyAttackTargetsInFront", BOOL, 0x21),
    ];
    const TURRET_FIELDS: &[(&str, u32, usize)] = &[
        ("ActiveWhenPerformingSpecialAbility", BOOL, 0x28),
        ("CanAcquireDynamicIfAssignedOutOfRange", BOOL, 0x29),
        ("CanPickTargetsOutOfTurretAngle", BOOL, 0x2a),
    ];
    let turret = node.has_tag_name((SAGE_NS, "TurretAITargetChooserData"));
    let extra: Vec<_> = ["TargetingCompareList"]
        .into_iter()
        .chain(
            TURRET_FIELDS
                .iter()
                .filter(|_| turret)
                .map(|&(name, _, _)| name),
        )
        .collect();
    validate_fields(node, FIELDS, &extra, data.path)?;
    let start = data.allocate(pointer, if turret { 0x2c } else { 0x28 })?;
    for (name, default, offset) in [
        ("IdleScanDelay", "0.8s", 0),
        ("ReacquireDelay", "60s", 4),
        ("StartupDelay", "0s", 8),
    ] {
        write_time(node, name, default, start + offset, data)?;
    }
    write_real(node, "SympathyRange", "25", start + 0xc, data)?;
    let dynamic = flags(
        node,
        "CanPickDynamicTargetType",
        "ALWAYS",
        &["NEVER", "ATTACK_MOVE", "AGGRESSIVE_STANCE", "ALWAYS"],
        data.path,
    )?;
    data.word(start + 0x10, dynamic);
    for (name, default, offset) in [
        ("CheckVisionRange", false, 0x1c),
        ("CanPickDynamicTargets", true, 0x1d),
        ("RotateToTargetWhenAiming", false, 0x1e),
        ("CanAutoAcquireNonAutoAcquirable", false, 0x1f),
        ("CanChaseAttack", true, 0x20),
        ("CanOnlyAttackTargetsInFront", false, 0x21),
    ] {
        write_bool(node, name, default, start + offset, data)?;
    }
    let id = asset_reference(
        node.attribute("TargetingCompareList").unwrap_or(if turret {
            "DefaultTurretTargetingCompareList"
        } else {
            "DefaultTargetingCompareList"
        }),
        "TargetingCompareList",
        data.path,
    )?;
    references.push(id.into());
    reference_types.push(id.type_id);
    data.word(start + 0x24, references.len() as u32);

    let mut fields: Vec<_> = FIELDS
        .iter()
        .map(|&(name, kind, offset)| Entry::named(name, kind, Target::Data(start + offset)))
        .collect();
    fields.push(Entry::named(
        "ObjectVisionRange",
        0x2e11_fc54,
        Target::Table(encode_vision_ranges(
            node,
            start + 0x14,
            data,
            reference_types,
        )?),
    ));
    let mut entries = vec![
        base(0xaa42_eb9c, Table::record(fields)),
        Entry::named(
            "TargetingCompareList",
            0x1b85_b469,
            Target::Data(start + 0x24),
        ),
    ];
    if turret {
        for &(name, kind, offset) in TURRET_FIELDS {
            write_bool(node, name, false, start + offset, data)?;
            entries.push(Entry::named(name, kind, Target::Data(start + offset)));
        }
    }
    Ok(Table::record(entries))
}

fn encode_vision_ranges(
    node: Node<'_, '_>,
    count: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const STRIDE: usize = 0x78;
    let ranges: Vec<_> = significant_children(node).collect();
    let first = data.list(count, ranges.len(), STRIDE)?;
    let mut items = Vec::with_capacity(ranges.len());
    for (index, range) in ranges.into_iter().enumerate() {
        if !range.has_tag_name((SAGE_NS, "ObjectVisionRange")) {
            return Err(invalid(
                data.path,
                format!(
                    "unsupported target chooser child {}",
                    range.tag_name().name()
                ),
            ));
        }
        validate_attributes(range, &["id", "Range"], data.path)?;
        let mut children = significant_children(range);
        let filter = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, "Filter")));
        let Some(filter) = filter.filter(|_| children.next().is_none()) else {
            return Err(invalid(data.path, "ObjectVisionRange requires one Filter"));
        };
        let start = first + index * STRIDE;
        // Its XML id identifies the entry for inheritance. EA leaves the binary id zero.
        let value = percentage(
            range.attribute("Range").unwrap_or("100"),
            "Range",
            data.path,
        )?;
        data.word(start + 4, value.to_bits());
        let filter = encode_inline_filter(filter, start + 8, data, reference_types)?;
        items.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Range", REAL, Target::Data(start + 4)),
                Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)),
            ])),
        ));
    }
    Ok(Table::array(count, items))
}

fn base(kind: u32, table: Table) -> Entry {
    Entry::new(1, kind, Target::Table(table))
}

fn validate_fields(
    node: Node<'_, '_>,
    fields: &[(&str, u32, usize)],
    extra: &[&str],
    path: &Path,
) -> io::Result<()> {
    let names: Vec<_> = fields
        .iter()
        .map(|&(name, _, _)| name)
        .chain(extra.iter().copied())
        .collect();
    validate_attributes(node, &names, path)
}

fn write_real(
    node: Node<'_, '_>,
    name: &str,
    default: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let number = finite_number(node.attribute(name).unwrap_or(default), name, data.path)?;
    data.word(offset, number.to_bits());
    Ok(())
}

fn write_time(
    node: Node<'_, '_>,
    name: &str,
    default: &str,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    let seconds = sage_time(node.attribute(name).unwrap_or(default), name, data.path)?;
    data.word(offset, seconds.to_bits());
    Ok(())
}

fn write_bool(
    node: Node<'_, '_>,
    name: &str,
    default: bool,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    data.payload[offset] = match node.attribute(name).map(str::trim) {
        None => u8::from(default),
        Some("false" | "0") => 0,
        Some("true" | "1") => 1,
        Some(value) => return Err(invalid(data.path, format!("invalid {name} {value}"))),
    };
    Ok(())
}

fn flags(
    node: Node<'_, '_>,
    name: &str,
    default: &str,
    values: &[&str],
    path: &Path,
) -> io::Result<u32> {
    let mut flags = 0;
    for value in node
        .attribute(name)
        .unwrap_or(default)
        .split_ascii_whitespace()
    {
        let index = values
            .iter()
            .position(|&candidate| candidate == value)
            .ok_or_else(|| invalid(path, format!("invalid {name} {value}")))?;
        flags |= 1 << index;
    }
    Ok(flags)
}

fn state_machine(node: Node<'_, '_>, path: &Path) -> io::Result<u32> {
    static VALUES: OnceLock<Vec<String>> = OnceLock::new();
    let values = VALUES.get_or_init(|| {
        enum_values(
            include_str!("../../../../schemas/ra3/xsd/Includes/UnitAIStateEnums.xsd"),
            "StateMachineType",
        )
    });
    enum_index(node, "StateMachine", "AIStateMachine", values, path)
}

fn locomotor_set(node: Node<'_, '_>, path: &Path) -> io::Result<u32> {
    locomotor_condition(
        node.attribute("DefaultLocomotorSet").unwrap_or("NORMAL"),
        "DefaultLocomotorSet",
        path,
    )
}

pub(super) fn locomotor_condition(value: &str, field: &str, path: &Path) -> io::Result<u32> {
    static VALUES: OnceLock<Vec<String>> = OnceLock::new();
    let values = VALUES.get_or_init(|| {
        enum_values(
            include_str!("../../../../schemas/ra3/xsd/Includes/GlobalGameData.xsd"),
            "LocomotorSetType",
        )
    });
    values
        .iter()
        .position(|candidate| candidate == value)
        .map(|index| (index as u32).wrapping_sub(1))
        .ok_or_else(|| invalid(path, format!("invalid {field} {value}")))
}

pub(super) fn enum_values(schema: &str, name: &str) -> Vec<String> {
    Document::parse(schema)
        .unwrap()
        .descendants()
        .find(|node| {
            node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "simpleType"))
                && node.attribute("name") == Some(name)
        })
        .unwrap()
        .descendants()
        .filter(|node| node.tag_name().name() == "enumeration")
        .filter_map(|node| node.attribute("value").map(str::to_owned))
        .collect()
}

fn enum_index(
    node: Node<'_, '_>,
    name: &str,
    default: &str,
    values: &[String],
    path: &Path,
) -> io::Result<u32> {
    let value = node.attribute(name).unwrap_or(default);
    values
        .iter()
        .position(|candidate| candidate == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(path, format!("invalid {name} {value}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;

    #[test]
    fn ai_update_matches_ea() {
        const XML: &str = include_str!("../../../../tests/fixtures/game-object-ai-update.xml");
        const BIN: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-ai-update.bin");
        const RELO: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-ai-update.relo");
        const IMP: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-ai-update.imp");
        let oracle = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-ai-update.manifest"
        ))
        .unwrap();
        let doc = Document::parse(XML).unwrap();
        let mut bin = &BIN[4..];
        let mut relo = &RELO[4..];
        let mut imp = &IMP[4..];
        for asset in &oracle.assets {
            let node = doc
                .root_element()
                .children()
                .find(|node| {
                    node.attribute("id")
                        .is_some_and(|id| hash_name(id) == asset.id.instance_id)
                })
                .unwrap();
            let name = node.attribute("id").unwrap();
            let actual = compile_asset(node, Path::new("ai-update-oracle.xml"), "").unwrap();
            let (expected_bin, rest) = bin.split_at(asset.chunk_sizes.instance as usize);
            bin = rest;
            let (expected_relo, rest) = relo.split_at(asset.chunk_sizes.relocations as usize);
            relo = rest;
            let (expected_imp, rest) = imp.split_at(asset.chunk_sizes.imports as usize);
            imp = rest;
            assert_eq!(actual.payload.len(), expected_bin.len(), "{name} length");
            assert_eq!(
                actual
                    .payload
                    .iter()
                    .zip(expected_bin)
                    .position(|(a, b)| a != b),
                None,
                "{name} first mismatched payload byte"
            );
            assert_eq!(actual.relocations, expected_relo, "{name} relocations");
            assert_eq!(actual.imports, expected_imp, "{name} imports");
            assert_eq!(
                actual.instance_hash, asset.instance_hash,
                "{name} instance hash"
            );
            assert!(
                actual
                    .references
                    .iter()
                    .map(|reference| reference.id)
                    .eq(asset.references.iter()),
                "{name} references"
            );
        }
        assert!(bin.is_empty() && relo.is_empty() && imp.is_empty());
    }
}
