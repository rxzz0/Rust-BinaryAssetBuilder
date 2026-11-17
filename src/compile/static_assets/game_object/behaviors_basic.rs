use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, fast_hash, finite_number, hash_name, percentage, sage_time,
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

fn crush_die(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x91ed_30dc;
    const SOUNDS: [(&str, u32, usize); 3] = [
        ("TotalCrushSound", 0x63d7_b468, 0x34),
        ("BackEndCrushSound", 0x63d7_b468, 0x38),
        ("FrontEndCrushSound", 0x63d7_b468, 0x3c),
    ];
    const PERCENTS: [(&str, u32, usize); 3] = [
        ("TotalCrushSoundPercent", 0x6458_64f3, 0x40),
        ("BackEndCrushSoundPercent", 0x6458_64f3, 0x44),
        ("FrontEndCrushSoundPercent", 0x6458_64f3, 0x48),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(SOUNDS.iter().chain(&PERCENTS).map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x4c)?;
    let behavior = module_base(node, KIND, start, data, false)?;
    let mut entries = vec![die_parent(node, start, behavior, data)?];
    for (name, _, offset) in SOUNDS {
        if let Some(value) = node.attribute(name) {
            let ordinal = super::audio_reference(value, path, references, reference_types)?;
            data.word(start + offset, ordinal);
        }
    }
    for (name, _, offset) in PERCENTS {
        let value = percentage(node.attribute(name).unwrap_or("100"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    entries.extend(fields(start, &SOUNDS));
    entries.extend(fields(start, &PERCENTS));
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

fn auto_heal(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xfbf6_592d;
    const BOOL: u32 = 0x77e3_f4f7;
    const INT: u32 = 0x23e6_50b8;
    const TIME: u32 = 0x9986_69e8;
    const FX: u32 = 0x0d1c_8e83;
    const KIND_OF: u32 = 0x9afc_9193;
    const FIELDS: [(&str, u32, usize); 16] = [
        ("InitiallyActive", BOOL, 0x68),
        ("HealingAmount", INT, 0x24),
        ("HealingDelay", TIME, 0x28),
        ("StartHealingDelay", TIME, 0x2c),
        ("Radius", INT, 0x30),
        ("AffectsWholePlayer", BOOL, 0x69),
        ("AffectsContained", BOOL, 0x6a),
        ("HealOnlyIfNotUnderAttack", BOOL, 0x6b),
        ("HealOnlyIfNotInCombat", BOOL, 0x6c),
        ("KindOf", KIND_OF, 0x34),
        ("HealOnlyOthers", BOOL, 0x6d),
        ("HealFXList", FX, 0x58),
        ("SpawnFXList", FX, 0x5c),
        ("NonStackable", BOOL, 0x6e),
        ("RespawnNearbyHordeMembers", BOOL, 0x6f),
        ("RespawnMinimumDelay", TIME, 0x60),
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
    let start = data.allocate(pointer, 0x70)?;
    let mut children: Vec<_> = significant_children(node).collect();
    let status = if children
        .last()
        .is_some_and(|child| child.has_tag_name((SAGE_NS, "ObjectStatusValidation")))
    {
        Target::Table(super::secondary_damage::encode_status(
            children.pop().unwrap(),
            start + 0x64,
            data,
        )?)
    } else {
        Target::Null
    };
    for (name, kind, offset) in FIELDS {
        let offset = start + offset;
        match kind {
            BOOL => data.payload[offset] = boolean(node, name, false, path)?,
            INT => {
                let number = node
                    .attribute(name)
                    .unwrap_or("0")
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?;
                data.word(offset, number as u32);
            }
            TIME => data.word(
                offset,
                sage_time(node.attribute(name).unwrap_or("0s"), name, path)?.to_bits(),
            ),
            KIND_OF => {
                super::fields::write_kind_of(node.attribute(name).unwrap_or(""), offset, data)?
            }
            FX => {
                let ordinal = super::passive_area::reference(
                    node.attribute(name).unwrap_or(""),
                    "FXList",
                    path,
                    references,
                    reference_types,
                )?;
                data.word(offset, ordinal);
            }
            _ => unreachable!("AutoHealBehavior field type"),
        }
    }
    let mut children = children.into_iter().peekable();
    let parent = upgrade_parent(node, KIND, start, data, reference_types, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected AutoHealBehavior child"));
    }
    let mut entries = vec![parent];
    entries.extend(fields(start, &FIELDS));
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}

fn charge_weapon(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x23e5_e3be;
    let path = data.path;
    validate_attributes(
        node,
        &["id", "ChargeTime", "OverChargeTime", "OverChargePercent"],
        path,
    )?;
    let mut children = significant_children(node);
    let status = children.next();
    if status.is_some_and(|child| !child.has_tag_name((SAGE_NS, "ObjectStatusValidation")))
        || children.next().is_some()
    {
        return Err(invalid(path, "unsupported ChargeWeaponUpdate child"));
    }
    let start = data.allocate(pointer, 0x18)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, default, offset) in [("ChargeTime", "1s", 8), ("OverChargeTime", "0s", 0xc)] {
        data.word(
            start + offset,
            sage_time(node.attribute(name).unwrap_or(default), name, path)?.to_bits(),
        );
        entries.push(Entry::named(
            name,
            0x9986_69e8,
            Target::Data(start + offset),
        ));
    }
    let name = "OverChargePercent";
    data.word(
        start + 0x10,
        percentage(node.attribute(name).unwrap_or("0%"), name, path)?.to_bits(),
    );
    entries.push(Entry::named(name, 0x6458_64f3, Target::Data(start + 0x10)));
    let status = match status {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0x14,
            data,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}

pub(in crate::compile::static_assets) fn weapon_slot(
    value: &str,
    path: &std::path::Path,
) -> io::Result<u32> {
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
    if node.has_tag_name((SAGE_NS, "CrushDie")) {
        return crush_die(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "StealthDetectorUpdate")) {
        return stealth_detector_update(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SuppressionUpdate")) {
        return suppression_update(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "FreefallUpdate")) {
        return freefall_update(node, pointer, data).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "EmotionTrackerUpdate")) {
        return emotion_tracker(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "WallBuilderBehavior")) {
        return wall_builder(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "StatusBitsUpgrade")) {
        return status_bits_upgrade(node, pointer, data, reference_types).map(Some);
    }
    if matches!(node.tag_name().name(), "PowerUpgrade" | "LevelUpUpgrade") {
        const LEVEL_FIELDS: [(&str, u32, usize); 3] = [
            ("LevelsToGain", 0x23e6_50b8, 0x24),
            ("LevelCap", 0x23e6_50b8, 0x28),
            ("DoFlash", 0x77e3_f4f7, 0x2c),
        ];
        let level = node.has_tag_name((SAGE_NS, "LevelUpUpgrade"));
        let (kind, size, extra) = if level {
            (0x977d_25a1, 0x30, &LEVEL_FIELDS[..])
        } else {
            (0xc3a6_2c68, 0x24, &[][..])
        };
        let path = data.path;
        let allowed: Vec<_> = std::iter::once("id")
            .chain(
                super::spawn::UPGRADE
                    .iter()
                    .chain(extra)
                    .map(|&(name, _, _)| name),
            )
            .collect();
        validate_attributes(node, &allowed, path)?;
        let start = data.allocate(pointer, size)?;
        let mut children = significant_children(node).peekable();
        let parent = upgrade_parent(node, kind, start, data, reference_types, &mut children)?;
        if children.next().is_some() {
            return Err(invalid(
                path,
                format!("unexpected {} child", node.tag_name().name()),
            ));
        }
        if level {
            for &(name, _, offset) in &LEVEL_FIELDS[..2] {
                let value = node
                    .attribute(name)
                    .unwrap_or("0")
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?;
                data.word(start + offset, value as u32);
            }
            data.payload[start + 0x2c] = boolean(node, "DoFlash", true, path)?;
        }
        let mut entries = vec![parent];
        entries.extend(fields(start, extra));
        return Ok(Some((kind, Table::record(entries))));
    }
    if node.has_tag_name((SAGE_NS, "AttributeModifierAuraUpdate")) {
        return attribute_modifier_aura(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "AutoHealBehavior")) {
        return auto_heal(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "RunOffMapBehavior")) {
        const KIND: u32 = 0x877a_cead;
        const FIELDS: [(&str, u32, usize); 4] = [
            ("RequiresSpecificTrigger", 0x77e3_f4f7, 0x10),
            ("RunOffMapWaypointName", 0x805d_25f4, 8),
            ("DieOnMap", 0x77e3_f4f7, 0x11),
            ("FlyingOffMap", 0x77e3_f4f7, 0x12),
        ];
        let path = data.path;
        let allowed: Vec<_> = std::iter::once("id")
            .chain(FIELDS.map(|(name, _, _)| name))
            .collect();
        validate_attributes(node, &allowed, path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected RunOffMapBehavior content"));
        }
        let start = data.allocate(pointer, 20)?;
        let mut entries = vec![module_base(node, KIND, start, data, false)?];
        for (name, token, offset) in FIELDS {
            if token == 0x77e3_f4f7 {
                data.payload[start + offset] =
                    boolean(node, name, name == "RequiresSpecificTrigger", path)?;
            } else if let Some(waypoint) = node.attribute(name) {
                data.string(start + offset, waypoint)?;
            }
        }
        entries.extend(fields(start, &FIELDS));
        return Ok(Some((KIND, Table::record(entries))));
    }
    if node.has_tag_name((SAGE_NS, "LiftObjectUpdate")) {
        return lift_object(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "DynamicsUpdate")) {
        return super::dynamics::encode(node, pointer, data).map(Some);
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
            | "PrematureDischargeSpecialPower"
            | "GiveProductionBonusSpecialPower"
            | "PlayerUpgradeSpecialPower"
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
    if node.has_tag_name((SAGE_NS, "ChargeWeaponUpdate")) {
        return charge_weapon(node, pointer, data).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "LineSegmentDamageUpdate")) {
        return super::line_segment_damage::encode(
            node,
            pointer,
            data,
            references,
            reference_types,
        )
        .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "LifetimeUpdate")) {
        return lifetime_update(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SuckUpObjectsUpdate")) {
        return suck_up_objects(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "GatherEjectUpdate")) {
        return gather_eject(node, pointer, data, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "SqueezeFlingUpdate")) {
        return super::squeeze_fling::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "BezierProjectile")) {
        return super::bezier_projectile::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    if node.has_tag_name((SAGE_NS, "FireWeaponUpdate")) {
        return fire_weapon_update(node, pointer, data, references, reference_types).map(Some);
    }
    if node.has_tag_name((SAGE_NS, "PlayerPowerManager")) {
        const KIND: u32 = 0x2e02_1907;
        const FIELDS: [(&str, u32, usize); 2] = [
            ("SpecialPowerTemplate", 0xaee8_3888, 8),
            ("OnAvailableEvaEvent", 0xc4ec_43b1, 12),
        ];
        let path = data.path;
        validate_attributes(
            node,
            &["id", "SpecialPowerTemplate", "OnAvailableEvaEvent"],
            path,
        )?;
        required(node, "SpecialPowerTemplate", path)?;
        if has_content(node) {
            return Err(invalid(path, "unexpected PlayerPowerManager content"));
        }
        let start = data.allocate(pointer, 16)?;
        let mut entries = vec![module_base(node, KIND, start, data, true)?];
        for attr in node.attributes() {
            let (kind, offset) = match attr.name() {
                "SpecialPowerTemplate" => ("SpecialPowerTemplate", 8),
                "OnAvailableEvaEvent" => ("EvaEvent", 12),
                _ => continue,
            };
            let ordinal = super::passive_area::reference(
                attr.value(),
                kind,
                path,
                references,
                reference_types,
            )?;
            data.word(start + offset, ordinal);
        }
        entries.extend(fields(start, &FIELDS));
        return Ok(Some((KIND, Table::record(entries))));
    }
    if matches!(
        node.tag_name().name(),
        "ReplaceSelfSpecialAbility"
            | "WeaponFireSpecialAbilityUpdate"
            | "ToggleStatusSpecialAbilityUpdate"
            | "CaptureSpecialAbilityUpdate"
    ) {
        return super::replace_self::encode(node, pointer, data, references, reference_types)
            .map(Some);
    }
    let (kind, size, extra) = match node.tag_name().name() {
        "SubStructureBehavior" => (0x331e_6db8, 8, None),
        "ProductionUpdateInfo" => (0x1b33_7783, 8, None),
        "ObjectSMCHelper" => (0x7f72_f242, 8, None),
        "ObjectDefectionHelper" => (0x18a4_208f, 8, None),
        "RadarBehavior" => (0x2e54_6f8e, 8, None),
        "StoreTemplateIDBehavior" => (0x50b8_001c, 8, None),
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
        let die = die_parent(node, offset, base(0xa50d_729a, behavior), data)?;
        return Ok(Some((kind, Table::record(vec![die]))));
    }
    if kind == 0x2e54_6f8e {
        return Ok(Some((
            kind,
            inherited(0x30a3_6b8d, inherited(0xa50d_729a, behavior)),
        )));
    }
    if matches!(kind, 0x1b33_7783 | 0x331e_6db8 | 0x50b8_001c) {
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

fn suppression_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xbcfb_5bb7;
    const FIELDS: [(&str, u32, usize); 6] = [
        ("UpdateDelay", 0x9986_69e8, 8),
        ("Suppressability", 0x6458_64f3, 12),
        ("SuppressionDuration", 0x9986_69e8, 16),
        ("AttributeModifierSuppressed", 0x09dd_cff3, 20),
        ("AttributeModifierForceMove", 0x09dd_cff3, 24),
        ("IgnoreSuppressionObjectStatus", 0xa7bd_811e, 28),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected SuppressionUpdate content"));
    }
    let start = data.allocate(pointer, 28 + super::secondary_damage::STATUS_BYTES)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, default, offset) in [("UpdateDelay", "1s", 8), ("SuppressionDuration", "3s", 16)] {
        let time = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, time.to_bits());
    }
    let suppressability = finite_number(
        node.attribute("Suppressability").unwrap_or("0"),
        "Suppressability",
        path,
    )?;
    data.word(start + 12, suppressability.to_bits());
    for &(name, _, offset) in &FIELDS[3..5] {
        if let Some(value) = node.attribute(name) {
            let ordinal = super::passive_area::reference(
                value,
                "AttributeModifier",
                path,
                references,
                reference_types,
            )?;
            data.word(start + offset, ordinal);
        }
    }
    if let Some(flags) = node.attribute("IgnoreSuppressionObjectStatus") {
        super::secondary_damage::write_status_flags(flags, start + 28, data)?;
    }
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}

fn stealth_detector_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x7456_9aaa;
    const AUDIO: u32 = 0x63d7_b468;
    const PARTICLE: u32 = 0x43f1_ce36;
    const BOOL: u32 = 0x77e3_f4f7;
    const FIELDS: &[(&str, u32, usize)] = &[
        ("DetectionRate", 0x9986_69e8, 0x24),
        ("DetectionRange", 0x6458_64f3, 0x28),
        ("InitiallyDisabled", BOOL, 0x98),
        ("PingSound", AUDIO, 0x2c),
        ("LoudPingSound", AUDIO, 0x30),
        ("IRBeaconParticleSys", PARTICLE, 0x34),
        ("IRParticleSys", PARTICLE, 0x38),
        ("IRBrightParticleSys", PARTICLE, 0x3c),
        ("IRGridParticleSys", PARTICLE, 0x40),
        ("IRParticleSysBone", 0x1a29_33aa, 0x44),
        ("ExtraRequiredKindOf", 0x9afc_9193, 0x4c),
        ("ExtraForbiddenKindOf", 0x9afc_9193, 0x70),
        ("CanDetectWhileGarrisoned", BOOL, 0x99),
        ("CanDetectWhileContained", BOOL, 0x9a),
        ("RequiredUpgrade", 0x4aba_1f29, 0x94),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(super::spawn::UPGRADE.iter().map(|&(name, _, _)| name))
        .chain(FIELDS.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x9c)?;
    for (name, _, offset) in &FIELDS[5..9] {
        let value = super::passive_area::reference(
            node.attribute(*name).unwrap_or(""),
            "FXParticleSystemTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(start + offset, value);
    }
    for (name, _, offset) in &FIELDS[3..5] {
        let value = super::audio_reference(
            node.attribute(*name).unwrap_or(""),
            path,
            references,
            reference_types,
        )?;
        data.word(start + offset, value);
    }
    let upgrade = super::passive_area::reference(
        node.attribute("RequiredUpgrade").unwrap_or(""),
        "UpgradeTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 0x94, upgrade);

    for &(name, kind, offset) in FIELDS {
        let address = start + offset;
        match kind {
            0x9986_69e8 => data.word(
                address,
                sage_time(node.attribute(name).unwrap_or("0.4s"), name, path)?.to_bits(),
            ),
            0x6458_64f3 => data.word(
                address,
                finite_number(node.attribute(name).unwrap_or("0"), name, path)?.to_bits(),
            ),
            BOOL => data.payload[address] = boolean(node, name, false, path)?,
            0x1a29_33aa => {
                if let Some(bone) = node.attribute(name).filter(|value| !value.is_empty()) {
                    data.string(address, bone)?;
                }
            }
            0x9afc_9193 => {
                for (index, word) in super::super::global_ui::kind_of_flags(
                    node.attribute(name).unwrap_or(""),
                    path,
                )?
                .into_iter()
                .enumerate()
                {
                    data.word(address + 4 * index, word);
                }
            }
            _ => {}
        }
    }

    let mut children = significant_children(node).peekable();
    let parent = upgrade_parent(node, KIND, start, data, reference_types, &mut children)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected StealthDetectorUpdate child"));
    }
    let mut entries = vec![parent];
    entries.extend(fields(start, FIELDS));
    Ok((KIND, Table::record(entries)))
}

fn freefall_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xfc22_2f04;
    const FIELDS: [(&str, u32, usize); 4] = [
        ("ModelConditionsToSet", 0x67db_bfc9, 8),
        ("InitialVelocity", 0x6458_64f3, 0x44),
        ("Acceleration", 0x6458_64f3, 0x48),
        ("TerminalVelocity", 0x6458_64f3, 0x4c),
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected FreefallUpdate content"));
    }
    let start = data.allocate(pointer, 0x50)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    if let Some(flags) = node.attribute("ModelConditionsToSet") {
        super::draws::write_model_conditions(flags, start + 8, data)?;
    }
    for (&(name, _, offset), default) in FIELDS[1..].iter().zip(["0", "1", "15"]) {
        let value = finite_number(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    entries.extend(fields(start, &FIELDS));
    Ok((KIND, Table::record(entries)))
}

fn emotion_tracker(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xa966_196e;
    const FIELDS: [(&str, u32, usize); 6] = [
        ("TauntAndPointDistance", 0x6458_64f3, 8),
        ("HeroScanDistance", 0x6458_64f3, 0xc),
        ("TauntAndPointUpdateDelay", 0x9986_69e8, 0x10),
        ("FearScanDistance", 0x6458_64f3, 0x14),
        ("IgnoreVeterancy", 0x77e3_f4f7, 0x34),
        ("ImmuneToFearLevel", 0x23e6_50b8, 0x18),
    ];
    const FILTERS: [&str; 4] = [
        "TauntAndPointExcluded",
        "AfraidOf",
        "AlwaysAfraidOf",
        "PointAt",
    ];
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0x38)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for &(name, kind, offset) in &FIELDS[..4] {
        let value = if kind == 0x9986_69e8 {
            sage_time(node.attribute(name).unwrap_or("2s"), name, path)?
        } else {
            finite_number(node.attribute(name).unwrap_or("0"), name, path)?
        };
        data.word(start + offset, value.to_bits());
    }
    data.payload[start + 0x34] = boolean(node, "IgnoreVeterancy", false, path)?;
    let fear_level = node
        .attribute("ImmuneToFearLevel")
        .unwrap_or("5")
        .trim()
        .parse::<i32>()
        .map_err(|_| invalid(path, "invalid ImmuneToFearLevel"))?;
    data.word(start + 0x18, fear_level as u32);
    entries.extend(fields(start, &FIELDS));

    let mut children = significant_children(node).peekable();
    for (index, name) in FILTERS.into_iter().enumerate() {
        let target =
            if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                Target::Table(super::parking::encode_filter(
                    child,
                    start + 0x1c + index * 4,
                    data,
                    reference_types,
                )?)
            } else {
                Target::Null
            };
        entries.push(Entry::named(name, 0xe46e_a3b2, target));
    }
    let mut emotions = Vec::new();
    for child in children {
        if !child.has_tag_name((SAGE_NS, "AddEmotion")) {
            return Err(invalid(path, "unexpected EmotionTrackerUpdate content"));
        }
        validate_attributes(child, &[], path)?;
        let value = super::super::text_value(child, path)?;
        emotions.push(super::passive_area::reference(
            value,
            "EmotionTemplate",
            path,
            references,
            reference_types,
        )?);
    }
    let list = start + 0x2c;
    let items = data.list(list, emotions.len(), 4)?;
    let mut targets = Vec::new();
    for (index, ordinal) in emotions.into_iter().enumerate() {
        let offset = items + index * 4;
        data.word(offset, ordinal);
        targets.push((0, Target::Data(offset)));
    }
    entries.push(Entry::named(
        "AddEmotion",
        0x9cfe_d1b4,
        Target::Table(Table::array(list, targets)),
    ));
    Ok((KIND, Table::record(entries)))
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

fn gather_eject(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xe611_c8ee;
    const TIME: u32 = 0x9986_69e8;
    const REAL: u32 = 0x6458_64f3;
    const INT: u32 = 0x23e6_50b8;
    let fields = [
        ("Lifetime", TIME, 8, "10s"),
        ("Slots", INT, 0xc, "5"),
        ("ScanTime", TIME, 0x10, "0.2s"),
        ("HeightOffGround", REAL, 0x14, "50"),
        ("EffectDistance", REAL, 0x18, "125"),
        ("LiftOffGroundDistance", REAL, 0x1c, "75"),
        ("GatherAcceleration", REAL, 0x20, "0.01"),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(fields.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, 0xac)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, ty, offset, default) in fields {
        let value = node.attribute(name).unwrap_or(default);
        let word = match ty {
            INT => value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, "invalid GatherEjectUpdate Slots"))?
                as u32,
            TIME => sage_time(value, name, path)?.to_bits(),
            _ => finite_number(value, name, path)?.to_bits(),
        };
        data.word(start + offset, word);
        entries.push(Entry::named(name, ty, Target::Data(start + offset)));
    }
    let mut children = significant_children(node);
    for (name, offset) in [("RotationSpeed", 0x24), ("UnitSpacing", 0x30)] {
        let vector = children
            .next()
            .filter(|child| child.has_tag_name((SAGE_NS, name)) && !has_content(*child))
            .ok_or_else(|| invalid(path, format!("GatherEjectUpdate requires {name}")))?;
        validate_attributes(vector, &["x", "y", "z"], path)?;
        for (axis, component) in ["x", "y", "z"].into_iter().zip([0, 4, 8]) {
            let value = finite_number(vector.attribute(axis).unwrap_or("0"), axis, path)?;
            data.word(start + offset + component, value.to_bits());
        }
        entries.push(Entry::named(
            name,
            0x2936_8077,
            Target::Data(start + offset),
        ));
    }
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "Filter")))
        .ok_or_else(|| invalid(path, "GatherEjectUpdate requires Filter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected GatherEjectUpdate content"));
    }
    let filter = super::parking::encode_inline_filter(filter, start + 0x3c, data, reference_types)?;
    entries.push(Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)));
    Ok((KIND, Table::record(entries)))
}

fn suck_up_objects(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x18c8_ffc4;
    const TIME: u32 = 0x9986_69e8;
    const REAL: u32 = 0x6458_64f3;
    const FIELDS: [(&str, u32, usize, &str); 5] = [
        ("InitialDelay", TIME, 8, "0.5s"),
        ("LiftVelocity", REAL, 0xc, "1"),
        ("DeathHeight", REAL, 0x10, "420"),
        ("Radius", REAL, 0x14, "10"),
        ("Duration", TIME, 0x18, "10s"),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(FIELDS.map(|(name, ..)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node);
    let filter = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")))
        .ok_or_else(|| invalid(path, "SuckUpObjectsUpdate requires an ObjectFilter"))?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SuckUpObjectsUpdate content"));
    }
    let start = data.allocate(pointer, 0x8c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, ty, offset, default) in FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        let value = if ty == TIME {
            sage_time(value, name, path)?
        } else {
            finite_number(value, name, path)?
        };
        data.word(start + offset, value.to_bits());
        entries.push(Entry::named(name, ty, Target::Data(start + offset)));
    }
    let filter = super::parking::encode_inline_filter(filter, start + 0x1c, data, reference_types)?;
    entries.push(Entry::named(
        "ObjectFilter",
        0xe46e_a3b2,
        Target::Table(filter),
    ));
    Ok((KIND, Table::record(entries)))
}

fn lifetime_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x6c28_3c72;
    const TIME: u32 = 0x9986_69e8;
    const BOOL: u32 = 0x77e3_f4f7;
    const SOUND: u32 = 0x63d7_b468;
    const COUNT: &str = "NumberOfCountdownSoundPlays";
    const FIELDS: [(&str, u32, usize); 10] = [
        ("MinLifetime", TIME, 8),
        ("MaxLifetime", TIME, 0xc),
        ("WaitForWakeUp", BOOL, 0x28),
        ("ScoreKill", BOOL, 0x29),
        ("DeathType", 0xc608_ed36, 0x10),
        ("IgnoreProjectileState", BOOL, 0x2a),
        ("CountdownSound", SOUND, 0x14),
        ("CountdownCompleteSound", SOUND, 0x18),
        ("TimeBeforeDeathThatCountdownCompletes", TIME, 0x1c),
        ("TimeBetweenCountdownSounds", TIME, 0x20),
    ];
    let path = data.path;
    let allowed: Vec<_> = ["id", COUNT]
        .into_iter()
        .chain(FIELDS.map(|(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected LifetimeUpdate content"));
    }
    let start = data.allocate(pointer, 0x2c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for attr in node.attributes() {
        let Some(&(_, SOUND, offset)) = FIELDS.iter().find(|&&(name, ..)| name == attr.name())
        else {
            continue;
        };
        let ordinal = super::client_behaviors::audio_reference(
            attr.value(),
            path,
            references,
            reference_types,
        )?;
        data.word(start + offset, ordinal);
    }
    for (name, ty, offset) in FIELDS {
        match ty {
            TIME => {
                let default = if offset == 0x20 { "1s" } else { "0s" };
                let value = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
                data.word(start + offset, value.to_bits());
            }
            BOOL => data.payload[start + offset] = boolean(node, name, false, path)?,
            _ => (),
        }
    }
    if let Some(value) = node.attribute("DeathType") {
        static DEATH: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        let types = DEATH.get_or_init(|| {
            super::ai_update::enum_values(
                include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
                "DeathType",
            )
        });
        let ordinal = types
            .iter()
            .position(|name| name == value)
            .ok_or_else(|| invalid(path, "invalid LifetimeUpdate DeathType"))?;
        data.word(start + 0x10, ordinal as u32);
    }
    entries.extend(fields(start, &FIELDS));
    let count = match node.attribute(COUNT) {
        Some(value) => {
            let value = value
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {COUNT}")))?;
            let offset = data.allocate(start + 0x24, 4)?;
            data.word(offset, value as u32);
            Target::Data(offset)
        }
        None => Target::Null,
    };
    entries.push(Entry::named(COUNT, 0x23e6_50b8, count));
    Ok((KIND, Table::record(entries)))
}

fn fire_weapon_update(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0xb28c_3d52;
    let path = data.path;
    validate_attributes(
        node,
        &["id", "FireInterval", "AliveOnly", "ActiveWhenDisabled"],
        path,
    )?;
    let mut nuggets = Vec::new();
    let mut status = None;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "FireWeaponNugget")) && status.is_none() {
            nuggets.push(child);
        } else if child.has_tag_name((SAGE_NS, "ObjectStatusValidation")) && status.is_none() {
            status = Some(child);
        } else {
            return Err(invalid(path, "unsupported FireWeaponUpdate child"));
        }
    }
    let start = data.allocate(pointer, 0x20)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(
        start + 8,
        sage_time(
            node.attribute("FireInterval").unwrap_or("0s"),
            "FireInterval",
            path,
        )?
        .to_bits(),
    );
    data.payload[start + 0x1c] = boolean(node, "AliveOnly", false, path)?;
    let mut disabled = 0;
    for value in node
        .attribute("ActiveWhenDisabled")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        disabled |=
            1 << super::super::global_ui::enum_value(value, super::production::DISABLED, path)?;
    }
    data.word(start + 0xc, disabled);
    entries.extend(fields(
        start,
        &[
            ("FireInterval", 0x9986_69e8, 8),
            ("AliveOnly", 0x77e3_f4f7, 0x1c),
            ("ActiveWhenDisabled", 0x1965_70c9, 0xc),
        ],
    ));
    let count = start + 0x10;
    let first = data.list(count, nuggets.len(), 0x10)?;
    let mut items = Vec::with_capacity(nuggets.len());
    for (index, nugget) in nuggets.into_iter().enumerate() {
        let offset = first + index * 0x10;
        validate_attributes(nugget, &["WeaponName", "FireDelay", "OneShot"], path)?;
        let weapon = super::passive_area::reference(
            nugget.attribute("WeaponName").unwrap_or(""),
            "WeaponTemplate",
            path,
            references,
            reference_types,
        )?;
        data.word(offset, weapon);
        data.word(
            offset + 4,
            sage_time(
                nugget.attribute("FireDelay").unwrap_or("0s"),
                "FireDelay",
                path,
            )?
            .to_bits(),
        );
        data.payload[offset + 0xc] = boolean(nugget, "OneShot", false, path)?;
        let mut children = significant_children(nugget);
        let position = if let Some(position) = children.next() {
            if !position.has_tag_name((SAGE_NS, "Offset"))
                || has_content(position)
                || children.next().is_some()
            {
                return Err(invalid(path, "unsupported FireWeaponNugget child"));
            }
            validate_attributes(position, &["x", "y", "z"], path)?;
            let vector = data.allocate(offset + 8, 12)?;
            for (axis, component) in ["x", "y", "z"].into_iter().zip([0, 4, 8]) {
                data.word(
                    vector + component,
                    finite_number(position.attribute(axis).unwrap_or("0"), axis, path)?.to_bits(),
                );
            }
            Target::Data(vector)
        } else {
            Target::Null
        };
        let mut nugget: Vec<_> = fields(
            offset,
            &[
                ("WeaponName", 0xd046_dd22, 0),
                ("FireDelay", 0x9986_69e8, 4),
                ("OneShot", 0x77e3_f4f7, 0xc),
            ],
        )
        .collect();
        nugget.push(Entry::named("Offset", 0x2936_8077, position));
        items.push((0, Target::Table(Table::record(nugget))));
    }
    entries.push(Entry::named(
        "FireWeaponNugget",
        0x8d65_067a,
        Target::Table(Table::array(count, items)),
    ));
    let status = match status {
        Some(child) => Target::Table(super::secondary_damage::encode_status(
            child,
            start + 0x18,
            data,
        )?),
        None => Target::Null,
    };
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
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

fn die_parent(
    node: Node<'_, '_>,
    start: usize,
    behavior: Entry,
    data: &mut BinaryData<'_>,
) -> io::Result<Entry> {
    let mut children = significant_children(node);
    let name = node.tag_name().name();
    let mux = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(data.path, format!("{name} requires DieMuxData")))?;
    if children.next().is_some() {
        return Err(invalid(data.path, format!("unexpected {name} content")));
    }
    let mux = encode_die_mux(mux, start + 8, data)?;
    Ok(base(
        0x9e4f_2b72,
        Table::record(vec![
            behavior,
            Entry::named("DieMuxData", 0x0084_001b, Target::Table(mux)),
        ]),
    ))
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
