use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, fast_hash, finite_number, hash_name, sage_time,
    significant_children, validate_attributes,
};
use super::draws::{array, boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn base(kind: u32, table: Table) -> Entry {
    Entry::new(1, kind, Target::Table(table))
}

pub(super) fn inherited(kind: u32, table: Table) -> Table {
    Table::record(vec![base(kind, table)])
}

pub(super) fn module_base(
    node: Node<'_, '_>,
    kind: u32,
    start: usize,
    data: &mut BinaryData<'_>,
    update: bool,
) -> io::Result<Entry> {
    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", data.path)?));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let behavior = inherited(0xd75d_93a2, module);
    Ok(if update {
        base(
            0xf100_2505,
            inherited(0x2900_211d, inherited(0xa50d_729a, behavior)),
        )
    } else {
        base(0xa50d_729a, behavior)
    })
}

fn squish_collide(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xb5eb_41ed;
    const TIMES: [(&str, &str, usize); 2] =
        [("CrushAnimTime", "1s", 8), ("CrushKillDelay", ".5s", 12)];
    const FLAGS: [(&str, u32, usize); 3] = [
        ("UseDirectionCheck", 0x77e3_f4f7, 16),
        ("UseVehicleStyleCrush", 0x77e3_f4f7, 17),
        ("AllowAllyCrush", 0x77e3_f4f7, 18),
    ];
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "CrushAnimTime",
            "CrushKillDelay",
            "UseDirectionCheck",
            "UseVehicleStyleCrush",
            "AllowAllyCrush",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "unexpected SquishCollide content"));
    }
    let start = data.allocate(pointer, 20)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    let mut entries = vec![base(0x70e5_c103, Table::record(vec![behavior]))];
    for (name, default, offset) in TIMES {
        let value = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(
            name,
            0x9986_69e8,
            Target::Data(start + offset),
        ));
    }
    for (name, _, offset) in FLAGS {
        data.payload[start + offset] = boolean(node, name, name == "UseDirectionCheck", path)?;
    }
    entries.extend(fields(start, &FLAGS));
    Ok((KIND, Table::record(entries)))
}

fn wall_builder(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x9590_538a;
    const FIELDS: [(&str, u32, usize); 4] = [
        ("MaxNumSegments", 0x23e6_50b8, 8),
        ("SecondaryModelConditions", 0x67db_bfc9, 0xc),
        ("SecondaryObjectStatus", 0xa7bd_811e, 0x48),
        ("StartInSecondaryModel", 0x77e3_f4f7, 0xd4),
    ];
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "MaxNumSegments",
            "SecondaryModelConditions",
            "SecondaryObjectStatus",
            "StartInSecondaryModel",
        ],
        path,
    )?;
    let mut children = significant_children(node);
    let filter = super::super::required_element(&mut children, "ConnectingObjectFilter", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected WallBuilderBehavior child"));
    }
    let start = data.allocate(pointer, 0xd8)?;
    let parent = module_base(node, KIND, start, data, false)?;
    let count = node
        .attribute("MaxNumSegments")
        .unwrap_or("5")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid MaxNumSegments"))?;
    data.word(start + 8, count as u32);
    super::draws::write_model_conditions(
        node.attribute("SecondaryModelConditions")
            .unwrap_or("WALL_SEGMENT"),
        start + 0xc,
        data,
    )?;
    super::secondary_damage::write_status_flags(
        node.attribute("SecondaryObjectStatus")
            .unwrap_or("WALL_SEGMENT"),
        start + 0x48,
        data,
    )?;
    data.payload[start + 0xd4] = boolean(node, "StartInSecondaryModel", false, path)?;
    let filter = super::parking::encode_inline_filter(filter, start + 0x64, data, reference_types)?;
    let mut entries = vec![parent];
    entries.extend(fields(start, &FIELDS));
    entries.push(Entry::named(
        "ConnectingObjectFilter",
        0xe46e_a3b2,
        Target::Table(filter),
    ));
    Ok((KIND, Table::record(entries)))
}

fn status_bits_upgrade(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    use super::spawn::UPGRADE;

    const KIND: u32 = 0x786c_9458;
    const STATUSES: [(&str, u32, usize); 2] = [
        ("StatusToSet", 0xa7bd_811e, 0x24),
        ("StatusToClear", 0xa7bd_811e, 0x40),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(UPGRADE.iter().chain(&STATUSES).map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x5c)?;
    for (name, _, offset) in STATUSES {
        super::secondary_damage::write_status_flags(
            node.attribute(name).unwrap_or(""),
            start + offset,
            data,
        )?;
    }
    let mut children = significant_children(node).peekable();
    let upgrade = upgrade_parent(node, KIND, start, data, reference_types, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected StatusBitsUpgrade child"));
    }
    let mut entries = vec![upgrade];
    entries.extend(fields(start, &STATUSES));
    Ok((KIND, Table::record(entries)))
}

fn upgrade_parent<'a, 'input: 'a>(
    node: Node<'_, '_>,
    kind: u32,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
    children: &mut std::iter::Peekable<impl Iterator<Item = Node<'a, 'input>>>,
) -> io::Result<Entry> {
    use super::spawn::{UPGRADE, id_list, weak_reference};

    let mut upgrade = vec![module_base(node, kind, start, data, false)?];
    for &(name, _, offset) in UPGRADE {
        data.payload[start + offset] =
            boolean(node, name, name == "WaypointModeTerminal", data.path)?;
    }
    upgrade.extend(fields(start, UPGRADE));
    let anim =
        match children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration"))) {
            Some(child) => Target::Table(super::tech_upgrade::encode_anim(child, start + 8, data)?),
            None => Target::Null,
        };
    upgrade.push(Entry::named("CustomAnimAndDuration", 0x6b74_6af6, anim));
    for (name, offset) in [("TriggeredBy", 0xc), ("ConflictsWith", 0x14)] {
        let mut ids = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            ids.push(weak_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        }
        let list = id_list(data, start + offset, ids)?;
        upgrade.push(Entry::named(name, 0x26fa_cfc7, Target::Table(list)));
    }
    Ok(base(0xae60_ae98, Table::record(upgrade)))
}

fn lift_object(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xbf91_bfcd;
    const REAL: u32 = 0x6458_64f3;
    const TIME: u32 = 0x9986_69e8;
    const WEAPON: u32 = 0xd046_dd22;
    const FIELDS: [(&str, u32, usize, &str); 7] = [
        ("LiftVelocity", REAL, 8, "0"),
        ("MaxElevationFromGround", REAL, 0xc, "0"),
        ("TimeIncrement", TIME, 0x10, "0s"),
        ("MaxTimeLifted", TIME, 0x14, "1s"),
        ("MaxStructureShakeVelocity", REAL, 0x18, "0"),
        ("AirplaneCrashWeapon", WEAPON, 0x1c, ""),
        ("SoftLandingWeapon", WEAPON, 0x20, ""),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["id", "CrusherModifiesVelocity"]
        .into_iter()
        .chain(FIELDS.map(|(name, _, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected LiftObjectUpdate content"));
    }
    let start = data.allocate(pointer, 0x28)?;
    let parent = module_base(node, KIND, start, data, true)?;
    data.payload[start + 0x24] = boolean(node, "CrusherModifiesVelocity", false, path)?;
    let mut entries = vec![
        parent,
        Entry::named(
            "CrusherModifiesVelocity",
            0x77e3_f4f7,
            Target::Data(start + 0x24),
        ),
    ];
    for (name, kind, offset, default) in FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        let word = match kind {
            REAL => finite_number(value, name, path)?.to_bits(),
            TIME => sage_time(value, name, path)?.to_bits(),
            WEAPON => super::passive_area::reference(
                value,
                "WeaponTemplate",
                path,
                references,
                reference_types,
            )?,
            _ => unreachable!("LiftObjectUpdate field type"),
        };
        data.word(start + offset, word);
        entries.push(Entry::named(name, kind, Target::Data(start + offset)));
    }
    Ok((KIND, Table::record(entries)))
}

fn attribute_modifier_aura(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x5e1c_afdb;
    const BOOL: u32 = 0x77e3_f4f7;
    const FIELDS: [(&str, u32, usize); 16] = [
        ("AttributeModifierName", 0x09dd_cff3, 0x24),
        ("RefreshDelay", 0x9986_69e8, 0x28),
        ("Range", 0x6458_64f3, 0x2c),
        ("AllowPowerWhenAttacking", BOOL, 0x60),
        ("InitiallyActive", BOOL, 0x61),
        ("RequiredConditions", 0x3d7e_e58a, 0x30),
        ("AntiCategory", 0x45f1_59e8, 0x34),
        ("AntiFX", 0x0d1c_8e83, 0x38),
        ("RunWhileDead", BOOL, 0x62),
        ("AllowSelf", BOOL, 0x63),
        ("AffectContainedOnly", BOOL, 0x64),
        ("AffectHordeMembersOnly", BOOL, 0x65),
        ("AffectAttacheeOnly", BOOL, 0x66),
        ("MaxActiveRank", 0x23e6_50b8, 0x3c),
        ("RequiredObjectStatusFlags", 0xa7bd_811e, 0x40),
        ("ShowDebugCircle", BOOL, 0x67),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(
            super::spawn::UPGRADE
                .iter()
                .chain(&FIELDS)
                .map(|&(name, _, _)| name),
        )
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children: Vec<_> = significant_children(node).collect();
    let filter = children
        .last()
        .copied()
        .filter(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")));
    if filter.is_some() {
        children.pop();
    }
    let start = data.allocate(pointer, 0x68)?;
    let mut filter_types = Vec::new();
    let filter = match filter {
        Some(child) => Target::Table(super::parking::encode_filter(
            child,
            start + 0x5c,
            data,
            &mut filter_types,
        )?),
        None => Target::Null,
    };
    let mut children = children.into_iter().peekable();
    let inherited_type_start = reference_types.len();
    let parent = upgrade_parent(node, KIND, start, data, reference_types, &mut children)?;
    let inherited_types = reference_types.split_off(inherited_type_start);
    if children.next().is_some() {
        return Err(invalid(
            path,
            "unexpected AttributeModifierAuraUpdate child",
        ));
    }
    for (name, token, offset) in FIELDS {
        let value = node.attribute(name).unwrap_or("");
        let offset = start + offset;
        let word = match token {
            BOOL => {
                data.payload[offset] =
                    boolean(node, name, name == "AllowPowerWhenAttacking", path)?;
                continue;
            }
            0xa7bd_811e => {
                super::secondary_damage::write_status_flags(value, offset, data)?;
                continue;
            }
            0x09dd_cff3 | 0x0d1c_8e83 => super::passive_area::reference(
                value,
                if name == "AntiFX" {
                    "FXList"
                } else {
                    "AttributeModifier"
                },
                path,
                references,
                reference_types,
            )?,
            0x9986_69e8 => sage_time(node.attribute(name).unwrap_or("0s"), name, path)?.to_bits(),
            0x6458_64f3 => {
                finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits()
            }
            0x23e6_50b8 => {
                node.attribute(name)
                    .unwrap_or("0")
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))? as u32
            }
            0x3d7e_e58a => value.split_ascii_whitespace().try_fold(0, |bits, flag| {
                Ok::<_, io::Error>(
                    bits | (1 << super::super::global_ui::enum_value(flag, &["MOUNTED"], path)?),
                )
            })?,
            0x45f1_59e8 => super::passive_area::category_flags(value, name, path)?,
            _ => unreachable!("AttributeModifierAuraUpdate field type"),
        };
        data.word(offset, word);
    }
    let mut entries = vec![parent];
    reference_types.extend(inherited_types);
    reference_types.extend(filter_types);
    entries.extend(fields(start, &FIELDS));
    entries.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
    Ok((KIND, Table::record(entries)))
}

pub(super) fn weapon_slot(value: &str, path: &std::path::Path) -> io::Result<u32> {
    super::super::global_ui::enum_value(
        value,
        &[
            "PRIMARY_WEAPON",
            "SECONDARY_WEAPON",
            "TERTIARY_WEAPON",
            "QUATERNARY_WEAPON",
            "QUINARY_WEAPON",
            "NO_WEAPON",
        ],
        path,
    )
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Option<(u32, Table)>> {
    if node.has_tag_name((SAGE_NS, "SquishCollide")) {
        return squish_collide(node, pointer, data).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "WallBuilderBehavior")) {
        return wall_builder(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "StatusBitsUpgrade")) {
        return status_bits_upgrade(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "AttributeModifierAuraUpdate")) {
        return attribute_modifier_aura(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "LiftObjectUpdate")) {
        return lift_object(node, pointer, data, references, reference_types).map(Some);
    }
    if matches!(
        node.tag_name().name(),
        "GarrisonContain" | "SlaughterHordeContain"
    ) {
        return super::garrison::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "InvisibilityBehavior")) {
        const KIND: u32 = 0x054b_09c4;
        let path = data.path;
        validate_attributes(node, &["id", "InvisibilityTemplate"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected InvisibilityBehavior content"));
        }
        let start = data.allocate(pointer, 12)?;
        let parent = module_base(node, KIND, start, data, false)?;
        let template = super::passive_area::reference(
            required(node, "InvisibilityTemplate", path)?,
            "InvisibilityTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(start + 8, template);
        return Ok(Some((
            KIND,
            Table::record(vec![
                parent,
                Entry::named("InvisibilityTemplate", 0x914c_a765, Target::Data(start + 8)),
            ]),
        )));
    }
    if node.has_tag_name((SAGE_NS, "SecondaryObjectListenerModule")) {
        return super::secondary_listener::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "StreamState")) {
        const KIND: u32 = 0xfef8_5f95;
        let path = data.path;
        validate_attributes(node, &["id", "StreamId"], path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected StreamState content"));
        }
        let stream_id = node
            .attribute("StreamId")
            .unwrap_or("0")
            .trim()
            .parse::<i32>()
            .map_err(|_| invalid(path, "invalid StreamId"))?;
        let start = data.allocate(pointer, 12)?;
        let parent = module_base(node, KIND, start, data, true)?;
        data.word(start + 8, stream_id as u32);
        return Ok(Some((
            KIND,
            Table::record(vec![
                parent,
                Entry::named("StreamId", 0x23e6_50b8, Target::Data(start + 8)),
            ]),
        )));
    }
    if node.has_tag_name((SAGE_NS, "FireWeaponWhenDead")) {
        return fire_weapon_when_dead(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SecondaryDamageTracker")) {
        return super::secondary_damage::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if matches!(
        node.tag_name().name(),
        "RecallUnitsSpecialPower"
            | "SpecialPower"
            | "AssignSlavesTargetObjectSpecialPower"
            | "OCLSpecialPower"
            | "StoreObjectsSpecialPower"
            | "TeleportStoredObjectsSpecialPower"
    ) {
        return super::recall::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "BuildingDestructionBehavior")) {
        return super::building_destruction::encode(
            node,
            pointer,
            data,
            references,
            reference_types,
        )
        .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "FXListBehavior")) {
        return super::fx_list_behavior::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "ProductionUpdate")) {
        return super::production::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "MonitorSpecialPowerTimerUpdate")) {
        const KIND: u32 = 0x8da1_2662;
        let path = data.path;
        validate_attributes(
            node,
            &["id", "SpecialPowerTemplate", "ReadyCondition"],
            path,
        )?;
        if has_content(node) {
            return Err(invalid(
                path,
                "unexpected special-power timer monitor content",
            ));
        }
        let start = data.allocate(pointer, 0x48)?;
        let parent = module_base(node, KIND, start, data, true)?;
        let ordinal = super::passive_area::reference(
            node.attribute("SpecialPowerTemplate").unwrap_or(""),
            "SpecialPowerTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(start + 8, ordinal);
        super::draws::write_model_conditions(
            node.attribute("ReadyCondition").unwrap_or(""),
            start + 0xc,
            data,
        )?;
        return Ok(Some((
            KIND,
            Table::record(vec![
                parent,
                Entry::named("SpecialPowerTemplate", 0xaee8_3888, Target::Data(start + 8)),
                Entry::named("ReadyCondition", 0x67db_bfc9, Target::Data(start + 0xc)),
            ]),
        )));
    }
    if node.has_tag_name((SAGE_NS, "StructurePlacementBehavior")) {
        const KIND: u32 = 0xe9d1_b7eb;
        let path = data.path;
        validate_attributes(node, &["id"], path)?;
        let mut children = significant_children(node);
        let shape = super::super::required_element(&mut children, "RestrictedAreaShape", path)?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected StructurePlacementBehavior child"));
        }
        let start = data.allocate(pointer, 0x38)?;
        let parent = module_base(node, KIND, start, data, false)?;
        let shape = super::children::encode_geometry_shape(shape, start + 8, data)?;
        return Ok(Some((
            KIND,
            Table::record(vec![
                parent,
                Entry::named("RestrictedAreaShape", 0xe721_0555, Target::Table(shape)),
            ]),
        )));
    }
    if node.has_tag_name((SAGE_NS, "AssistedPlacementBehavior")) {
        return assisted_placement(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "QueueDockBehavior")) {
        return queue_dock(node, pointer, data).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "ParkingPlaceBehavior")) {
        return super::parking::encode(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "RepairUpdate")) {
        return super::repair::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "EngineerContain")) {
        return super::engineer::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "InfiltratorContain")) {
        return super::infiltrator::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "LaserState")) {
        return super::laser_state::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if matches!(
        node.tag_name().name(),
        "NotifyWhenAttackingBehavior" | "AttackAssistResponseBehavior"
    ) {
        return super::attack_assist::encode(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SlavedUpdate")) {
        return super::slaved::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "PassiveAreaEffectBehavior")) {
        return super::passive_area::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "AttachUpdate")) {
        return super::attach::encode(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "TeleportStoredObjectsUpdate")) {
        return super::teleport_stored::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SlowDeath")) {
        return super::slow_death::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if matches!(
        node.tag_name().name(),
        "ReplaceSelfSpecialAbility" | "WeaponFireSpecialAbilityUpdate"
    ) {
        return super::replace_self::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    let (kind, size, extra) = match node.tag_name().name() {
        "SubStructureBehavior" => (0x331e_6db8, 8, None),
        "ProductionUpdateInfo" => (0x1b33_7783, 8, None),
        "ObjectSMCHelper" => (0x7f72_f242, 8, None),
        "ObjectDefectionHelper" => (0x18a4_208f, 8, None),
        "AttributeModifierPoolUpdate" => (0xe622_f055, 8, None),
        "StancesBehavior" => (0xc0a1_e43d, 12, Some("StanceTemplate")),
        "LUAEventList" => (0xfa6a_c8b6, 16, Some("EventListName")),
        "DestroyDie" => (0x0f7e_c801, 52, None),
        _ => return Ok(None),
    };
    let allowed: Vec<_> = std::iter::once("id").chain(extra).collect();
    validate_attributes(node, &allowed, data.path)?;
    if has_content(node) && kind != 0x0f7e_c801 {
        return Err(crate::compile::invalid(
            data.path,
            "unexpected behavior module content",
        ));
    }
    let id = required(node, "id", data.path)?;
    let offset = data.allocate(pointer, size)?;
    data.word(offset, kind);
    data.word(offset + 4, hash_name(id));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(offset)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(offset + 4)),
    ]);
    let behavior = inherited(0xd75d_93a2, module);
    if kind == 0x0f7e_c801 {
        let mut children = super::super::significant_children(node);
        let die = children
            .next()
            .filter(|n| n.has_tag_name((crate::compile::SAGE_NS, "DieMuxData")))
            .ok_or_else(|| crate::compile::invalid(data.path, "DestroyDie requires DieMuxData"))?;
        if children.next().is_some() {
            return Err(invalid(data.path, "unexpected DestroyDie content"));
        }
        let mux = encode_die_mux(die, offset + 8, data)?;
        let die = Table::record(vec![
            base(0xa50d_729a, behavior),
            Entry::new(0x33d7_093c, 0x0084_001b, Target::Table(mux)),
        ]);
        return Ok(Some((kind, inherited(0x9e4f_2b72, die))));
    }
    if matches!(kind, 0x1b33_7783 | 0x331e_6db8) {
        return Ok(Some((kind, inherited(0xa50d_729a, behavior))));
    }
    if kind == 0xfa6a_c8b6 {
        let name = required(node, "EventListName", data.path)?;
        data.string(offset + 8, name)?;
        return Ok(Some((
            kind,
            Table::record(vec![
                base(0xa50d_729a, behavior),
                Entry::new(0x05c8_facb, 0x1a29_33aa, Target::Data(offset + 8)),
            ]),
        )));
    }
    let update = inherited(0x2900_211d, inherited(0xa50d_729a, behavior));
    if kind == 0xc0a1_e43d {
        let value = required(node, "StanceTemplate", data.path)?;
        references.push(asset_reference(value, "StanceTemplate", data.path)?.into());
        reference_types.push(fast_hash(b"StanceTemplate"));
        data.word(offset + 8, references.len() as u32);
        return Ok(Some((
            kind,
            Table::record(vec![
                base(0xf100_2505, update),
                Entry::new(0x9684_c743, 0x0d82_18fb, Target::Data(offset + 8)),
            ]),
        )));
    }
    let update = inherited(0xf100_2505, update);
    let table = if kind == 0xe622_f055 {
        update
    } else {
        inherited(0x29ca_d490, update)
    };
    Ok(Some((kind, table)))
}

fn assisted_placement(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xae0e_e9a0;
    let path = data.path;
    validate_attributes(node, &["id", "PlacementHint", "SnappingDistance"], path)?;
    let start = data.allocate(pointer, 0x14)?;
    let mut entries = vec![module_base(node, KIND, start, data, false)?];
    let mut flags = 0;
    for flag in node
        .attribute("PlacementHint")
        .unwrap_or("NONE")
        .split_ascii_whitespace()
    {
        flags |= 1
            << super::super::global_ui::enum_value(
                flag,
                &["NONE", "FACING_TOWARDS", "FACING_AWAY", "ALLOW_SHROUDED"],
                path,
            )?;
    }
    data.word(start + 8, flags);
    let distance = finite_number(
        node.attribute("SnappingDistance").unwrap_or("0"),
        "SnappingDistance",
        path,
    )?;
    data.word(start + 0xc, distance.to_bits());
    let mut children = significant_children(node).peekable();
    let filter = if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "RequirementObjectFilter")))
    {
        let offset = data.allocate(start + 0x10, 0x70)?;
        Target::Table(super::parking::encode_inline_filter(
            child,
            offset,
            data,
            reference_types,
        )?)
    } else {
        Target::Null
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected AssistedPlacementBehavior child"));
    }
    entries.extend([
        Entry::named("PlacementHint", 0xfa98_de4d, Target::Data(start + 8)),
        Entry::named("SnappingDistance", 0x6458_64f3, Target::Data(start + 0xc)),
        Entry::named("RequirementObjectFilter", 0xe46e_a3b2, filter),
    ]);
    Ok((KIND, Table::record(entries)))
}

fn queue_dock(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xaf4c_1f21;
    const BONES: [&str; 5] = [
        "DockStartBoneName",
        "DockActionBoneName",
        "DockEndBoneName",
        "QueueBonePrefix",
        "TurnTableBoneName",
    ];
    let path = data.path;
    let allowed: Vec<_> = BONES
        .into_iter()
        .chain(["id", "DockingAnimation"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected QueueDockBehavior content"));
    }
    let start = data.allocate(pointer, 0x34)?;
    let parent = module_base(node, KIND, start, data, false)?;
    let mut entries = vec![base(0x30a3_6b8d, Table::record(vec![parent]))];
    for (index, name) in BONES.into_iter().enumerate() {
        let offset = start + 8 + index * 8;
        data.string(offset, node.attribute(name).unwrap_or(""))?;
        entries.push(Entry::named(name, 0x1a29_33aa, Target::Data(offset)));
    }
    let offset = start + 0x30;
    if let Some(flag) = node.attribute("DockingAnimation") {
        let value = if flag == "INVALID" {
            u32::MAX
        } else {
            super::draws::model_condition_index(flag, path)? as u32
        };
        data.word(offset, value);
    }
    entries.push(Entry::named(
        "DockingAnimation",
        0x1572_3e0b,
        Target::Data(offset),
    ));
    Ok((KIND, Table::record(entries)))
}

fn fire_weapon_when_dead(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x90ce_b49c;
    const BOOL: u32 = 0x77e3_f4f7;
    const UPGRADE_FLAGS: &[(&str, u32, usize)] = &[
        ("RequiresAllTriggers", BOOL, 0x1c),
        ("RequiresAllConflictingTriggers", BOOL, 0x1d),
        ("Permanent", BOOL, 0x1e),
        ("WaypointMode", BOOL, 0x1f),
        ("WaypointModeTerminal", BOOL, 0x20),
    ];
    const FIRE_FLAGS: &[(&str, u32, usize)] = &[
        ("InitiallyActive", BOOL, 0x88),
        ("ActiveDuringConstruction", BOOL, 0x89),
        ("ContinueToBezierDestination", BOOL, 0x8a),
        ("FireAtGround", BOOL, 0x8b),
        ("UseMyKillerAsWeaponSource", BOOL, 0x8c),
    ];
    let path = data.path;
    let allowed: Vec<_> = UPGRADE_FLAGS
        .iter()
        .chain(FIRE_FLAGS)
        .map(|&(name, _, _)| name)
        .chain(["id", "DelayTime", "DeathWeapon", "FocusFireKindOfFlag"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let die = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(path, "FireWeaponWhenDead requires DieMuxData"))?;
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unsupported FireWeaponWhenDead child {}",
                child.tag_name().name()
            ),
        ));
    }
    let start = data.allocate(pointer, 0x90)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, _, offset) in UPGRADE_FLAGS.iter().chain(FIRE_FLAGS) {
        data.payload[start + offset] = boolean(node, name, name == "WaypointModeTerminal", path)?;
    }
    let delay = sage_time(
        node.attribute("DelayTime").unwrap_or("0s"),
        "DelayTime",
        path,
    )?;
    data.word(start + 0x24, delay.to_bits());
    if let Some(value) = node
        .attribute("DeathWeapon")
        .filter(|value| !value.trim().is_empty())
    {
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, "WeaponTemplate", path)?;
        references.push(id.into());
        reference_types.push(id.type_id);
        data.word(start + 0x28, references.len() as u32);
    }
    if let Some(value) = node.attribute("FocusFireKindOfFlag") {
        super::fields::write_kind_of(value, start + 0x2c, data)?;
    }
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut upgrade = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    upgrade.extend(fields(start, UPGRADE_FLAGS));
    upgrade.extend([
        Entry::named("CustomAnimAndDuration", 0x6b74_6af6, Target::Null),
        array("TriggeredBy", 0x26fa_cfc7, start + 0x0c),
        array("ConflictsWith", 0x26fa_cfc7, start + 0x14),
    ]);
    let mut entries = vec![base(0xae60_ae98, Table::record(upgrade))];
    entries.extend(fields(start, &FIRE_FLAGS[..4]));
    entries.extend(fields(
        start,
        &[
            ("DelayTime", 0x9986_69e8, 0x24),
            ("DeathWeapon", 0xd046_dd22, 0x28),
            ("FocusFireKindOfFlag", 0x9afc_9193, 0x2c),
        ],
    ));
    entries.extend(fields(start, &FIRE_FLAGS[4..]));
    entries.extend([
        Entry::named("WeaponOffset", 0x2936_8077, Target::Null),
        Entry::named(
            "DieMuxData",
            0x0084_001b,
            Target::Table(encode_die_mux(die, start + 0x54, data)?),
        ),
        array("WeaponFireProbability", 0x080b_e21f, start + 0x80),
    ]);
    Ok((KIND, Table::record(entries)))
}

pub(super) fn encode_die_mux(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "VeterancyLevels",
            "ExemptStatus",
            "RequiredStatus",
            "DamageAmountRequired",
            "MinKillerAngle",
            "MaxKillerAngle",
            "DeathTypes",
            "DeathTypesForbidden",
        ],
        path,
    )?;
    if has_content(node) {
        return Err(invalid(path, "DieMuxData cannot contain content"));
    }
    let levels = node
        .attribute("VeterancyLevels")
        .unwrap_or("")
        .split_ascii_whitespace();
    let strings = data.list(start, levels.clone().count(), 8)?;
    let mut items = Vec::new();
    for (index, level) in levels.enumerate() {
        let offset = strings + index * 8;
        data.string(offset, level)?;
        items.push((0, Target::Data(offset)));
    }
    let damage = finite_number(
        node.attribute("DamageAmountRequired").unwrap_or("-1"),
        "DamageAmountRequired",
        path,
    )?;
    data.word(start + 16, damage.to_bits());
    for (name, default, offset) in [("MinKillerAngle", "1d", 20), ("MaxKillerAngle", "-1d", 24)] {
        let value = node.attribute(name).unwrap_or(default).trim();
        let (number, degrees) = if let Some(number) = value.strip_suffix('d') {
            (number, true)
        } else if let Some(number) = value.strip_suffix('r') {
            (number, false)
        } else {
            return Err(invalid(path, format!("invalid {name}")));
        };
        let angle = finite_number(number, name, path)?;
        let angle = if degrees { angle.to_radians() } else { angle };
        data.word(start + offset, angle.to_bits());
    }
    for (name, offset) in [("DeathTypes", 28), ("DeathTypesForbidden", 36)] {
        let flags = death_flags(node.attribute(name).unwrap_or(""), path)?;
        data.word(start + offset, flags as u32);
        data.word(start + offset + 4, (flags >> 32) as u32);
    }
    let mut entries = vec![Entry::named(
        "VeterancyLevels",
        0x6d66_ae18,
        Target::Table(Table::array(start, items)),
    )];
    for (name, offset) in [("ExemptStatus", 8), ("RequiredStatus", 12)] {
        let target = if let Some(value) = node.attribute(name) {
            let offset = data.allocate(start + offset, super::secondary_damage::STATUS_BYTES)?;
            super::secondary_damage::write_status_flags(value, offset, data)?;
            Target::Data(offset)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, 0xa7bd_811e, target));
    }
    entries.extend(fields(
        start,
        &[
            ("DamageAmountRequired", 0x6458_64f3, 16),
            ("MinKillerAngle", 0x4e16_ea4d, 20),
            ("MaxKillerAngle", 0x4e16_ea4d, 24),
            ("DeathTypes", 0x034d_f1a1, 28),
            ("DeathTypesForbidden", 0x034d_f1a1, 36),
        ],
    ));
    Ok(Table::record(entries))
}

pub(super) fn death_flags(value: &str, path: &std::path::Path) -> io::Result<u64> {
    let schema = roxmltree::Document::parse(include_str!(
        "../../../../schemas/ra3/xsd/Includes/Death.xsd"
    ))
    .unwrap();
    let names: Vec<_> = schema
        .descendants()
        .filter(|n| n.tag_name().name() == "enumeration")
        .filter_map(|n| n.attribute("value"))
        .collect();
    let mut flags = 0;
    for name in value.split_ascii_whitespace() {
        if name == "ALL" {
            flags = u64::MAX;
        } else {
            let bit = names.iter().position(|&n| n == name).ok_or_else(|| {
                crate::compile::invalid(path, format!("unknown death type {name}"))
            })?;
            flags |= 1u64 << bit;
        }
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::super::super::{Marshaled, tokenized};
    use super::*;
    use roxmltree::Document;
    use std::path::Path;

    #[test]
    fn fire_weapon_when_dead_matches_ea() {
        use crate::compile::static_assets::compile_asset;
        use crate::manifest::Manifest;

        let document = Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-death-behavior.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            document.root_element().first_element_child().unwrap(),
            Path::new("game-object-death-behavior.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-death-behavior.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-death-behavior.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-death-behavior.manifest"
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

    fn u32_at(bytes: &[u8], offset: usize) -> usize {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn basic_behavior_data_and_tokens_match_ea() {
        for name in ["smc", "defection", "pool", "stances", "lua", "destroy"] {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            let xml = std::fs::read_to_string(
                base.join(format!("game-object-behavior-basic-{name}.xml")),
            )
            .unwrap();
            let bytes =
                std::fs::read(base.join(format!("game-object-behavior-basic-{name}.bin"))).unwrap();
            let oracle = &bytes[4..];
            let root = u32_at(oracle, 8);
            let start = u32_at(oracle, root + 0x254);
            let end = u32_at(oracle, root + 0x1dc);
            let document = Document::parse(&xml).unwrap();
            let node = document
                .descendants()
                .find(|node| {
                    node.parent()
                        .is_some_and(|p| p.tag_name().name() == "Behaviors")
                })
                .unwrap();
            let mut data = BinaryData {
                payload: vec![0; 4],
                relocations: Vec::new(),
                path: Path::new("probe"),
            };
            let mut references = Vec::new();
            let mut reference_types = Vec::new();
            let (kind, table) = encode(node, 0, &mut data, &mut references, &mut reference_types)
                .unwrap()
                .unwrap();
            assert_eq!(kind as usize, u32_at(oracle, start));
            let mut expected = oracle[start..end].to_vec();
            if name == "lua" {
                let pointer = u32_at(&expected, 12) - start + 4;
                expected[12..16].copy_from_slice(&(pointer as u32).to_le_bytes());
            }
            assert_eq!(&data.payload[4..], expected, "{name} module data");
            let token_start = 0x648;
            let module_fields = (token_start..root)
                .step_by(12)
                .find(|&at| u32_at(oracle, at) == 0x19b0_42a0)
                .unwrap();
            let token_end = if name == "destroy" {
                0x738
            } else {
                module_fields + 36
            };
            let encoded = tokenized::finish(table, data, Marshaled::default()).unwrap();
            let data_start = token_end - token_start + 4;
            let mut expected_tokens = oracle[token_start..token_end].to_vec();
            for entry in expected_tokens.as_chunks_mut::<12>().0 {
                let target = u32_at(entry, 8);
                let adjusted = if target >= root {
                    target - start + 4 + data_start
                } else if target != 0 {
                    target - token_start
                } else {
                    0
                };
                entry[8..12].copy_from_slice(&(adjusted as u32).to_le_bytes());
            }
            assert_eq!(
                &encoded.payload[..expected_tokens.len()],
                expected_tokens,
                "{name} module tokens"
            );
        }
    }
}
