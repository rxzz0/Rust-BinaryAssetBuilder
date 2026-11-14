use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, asset_reference, fast_hash, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Vec<(&'static str, Target)>> {
    let mut targets = Vec::new();
    if let Some(child) = find(node, "DisplayName")? {
        validate_attributes(child, &[], data.path)?;
        if child.children().any(|item| item.is_element()) {
            return Err(invalid(data.path, "DisplayName cannot contain elements"));
        }
        let value = child.text().unwrap_or("");
        let header = data.allocate(0x188, 8)?;
        data.string(header, value)?;
        targets.push(("DisplayName", Target::Data(header)));
    }
    if let Some(child) = find(node, "GameDependency")? {
        targets.push((
            "GameDependency",
            dependency(child, 0x190, data, reference_types)?,
        ));
    }
    if let Some(child) = find(node, "ObjectResourceInfo")? {
        validate_attributes(child, &[], data.path)?;
        let costs = child
            .children()
            .filter(Node::is_element)
            .collect::<Vec<_>>();
        if costs
            .iter()
            .any(|item| !item.has_tag_name((SAGE_NS, "BuildCost")))
        {
            return Err(invalid(data.path, "unsupported ObjectResourceInfo child"));
        }
        let start = data.allocate(0x194, 8)?;
        let items = data.list(start, costs.len(), 8)?;
        let mut entries = Vec::new();
        for (index, cost) in costs.iter().enumerate() {
            validate_attributes(*cost, &["Account", "Amount"], data.path)?;
            if has_content(*cost) {
                return Err(invalid(data.path, "BuildCost cannot contain content"));
            }
            let account = required(*cost, "Account", data.path)?
                .parse::<u32>()
                .map_err(|_| invalid(data.path, "invalid BuildCost Account"))?;
            let amount = required(*cost, "Amount", data.path)?
                .parse::<i32>()
                .map_err(|_| invalid(data.path, "invalid BuildCost Amount"))?;
            let offset = items + index * 8;
            data.word(offset, account);
            data.word(offset + 4, amount as u32);
            entries.push((
                0,
                Target::Table(Table::record(vec![
                    Entry::named("Account", 0x7631_77a4, Target::Data(offset)),
                    Entry::named("Amount", 0x7631_77a4, Target::Data(offset + 4)),
                ])),
            ));
        }
        targets.push((
            "ObjectResourceInfo",
            Target::Table(Table::record(vec![Entry::named(
                "BuildCost",
                0xb706_334d,
                Target::Table(Table::array(start, entries)),
            )])),
        ));
    }
    let armors = node
        .children()
        .filter(|child| child.has_tag_name((SAGE_NS, "ArmorSet")))
        .collect::<Vec<_>>();
    if !armors.is_empty() {
        let start = data.list(0x198, armors.len(), 12)?;
        let mut entries = Vec::new();
        for (index, armor) in armors.iter().enumerate() {
            validate_attributes(
                *armor,
                &["id", "Armor", "DamageFX", "Conditions"],
                data.path,
            )?;
            if has_content(*armor) {
                return Err(invalid(data.path, "ArmorSet cannot contain content"));
            }
            let offset = start + index * 12;
            for (name, kind, field) in [("Armor", "ArmorTemplate", 0), ("DamageFX", "DamageFX", 4)]
            {
                if let Some(value) = armor.attribute(name) {
                    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                    let id = asset_reference(plain, kind, data.path)?;
                    references.push(id.into());
                    reference_types.push(fast_hash(kind.as_bytes()));
                    data.word(offset + field, references.len() as u32);
                }
            }
            if let Some(conditions) = armor.attribute("Conditions") {
                static CONDITIONS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
                let names = CONDITIONS.get_or_init(|| {
                    super::ai_update::enum_values(
                        include_str!("../../../../schemas/ra3/xsd/AssetTypeArmorTemplate.xsd"),
                        "ArmorSetType",
                    )
                });
                let mut bits = 0;
                for flag in conditions.split_ascii_whitespace() {
                    let index = names.iter().position(|name| name == flag).ok_or_else(|| {
                        invalid(data.path, format!("unsupported ArmorSet condition {flag}"))
                    })?;
                    bits |= 1 << index;
                }
                data.word(offset + 8, bits);
            }
            entries.push((
                0,
                Target::Table(Table::record(vec![
                    Entry::named("Armor", 0x34e9_127b, Target::Data(offset)),
                    Entry::named("DamageFX", 0x9a95_edff, Target::Data(offset + 4)),
                    Entry::named("Conditions", 0xed8b_c8e9, Target::Data(offset + 8)),
                ])),
            ));
        }
        targets.push(("ArmorSet", Target::Table(Table::array(0x198, entries))));
    }
    let locomotors: Vec<_> = node
        .children()
        .filter(|child| child.has_tag_name((SAGE_NS, "LocomotorSet")))
        .collect();
    if !locomotors.is_empty() {
        let count = 0x1a0;
        let start = data.list(count, locomotors.len(), 12)?;
        let mut entries = Vec::with_capacity(locomotors.len());
        for (index, locomotor) in locomotors.into_iter().enumerate() {
            let path = data.path;
            validate_attributes(locomotor, &["id", "Speed", "Condition", "Locomotor"], path)?;
            if has_content(locomotor) {
                return Err(invalid(path, "LocomotorSet cannot contain content"));
            }
            let offset = start + index * 12;
            let speed = super::super::finite_number(
                locomotor.attribute("Speed").unwrap_or("0"),
                "Speed",
                path,
            )?;
            let condition = super::ai_update::locomotor_condition(
                locomotor.attribute("Condition").unwrap_or("NORMAL"),
                "Condition",
                path,
            )?;
            data.word(offset, speed.to_bits());
            data.word(offset + 4, condition);
            if let Some(value) = locomotor
                .attribute("Locomotor")
                .filter(|value| !value.trim().is_empty())
            {
                let plain = value.split_once('\\').map_or(value, |(name, _)| name);
                let id = asset_reference(plain, "LocomotorTemplate", path)?;
                references.push(id.into());
                reference_types.push(id.type_id);
                data.word(offset + 8, references.len() as u32);
            }
            entries.push((
                0,
                Target::Table(Table::record(
                    super::draws::fields(
                        offset,
                        &[
                            ("Speed", 0x6458_64f3, 0),
                            ("Condition", 0x845d_12b2, 4),
                            ("Locomotor", 0xfed2_0c6c, 8),
                        ],
                    )
                    .collect(),
                )),
            ));
        }
        targets.push(("LocomotorSet", Target::Table(Table::array(count, entries))));
    }
    if let Some(child) = find(node, "SkirmishAIInformation")? {
        targets.push(("SkirmishAIInformation", skirmish(child, data)?));
    }
    Ok(targets)
}

pub(in crate::compile::static_assets) fn dependency(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Target> {
    use super::draws::write_model_conditions;
    use super::parking::encode_filter;
    use super::secondary_damage::{STATUS_BYTES, write_status_flags};
    use super::spawn::{id_list, weak_reference};

    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "RequiredModelConditionsAny",
            "ForbiddenModelConditions",
            "RequiredObjectStatusAny",
        ],
        path,
    )?;
    let start = data.allocate(pointer, 0x28)?;
    let mut entries = Vec::new();
    for (name, kind, offset, size) in [
        ("RequiredModelConditionsAny", 0x67db_bfc9, 0, 0x3c),
        ("ForbiddenModelConditions", 0x67db_bfc9, 4, 0x3c),
        ("RequiredObjectStatusAny", 0xa7bd_811e, 8, STATUS_BYTES),
    ] {
        let target = if let Some(value) = node.attribute(name) {
            let flags = data.allocate(start + offset, size)?;
            if name == "RequiredObjectStatusAny" {
                write_status_flags(value, flags, data)?;
            } else {
                write_model_conditions(value, flags, data)?;
            }
            Target::Data(flags)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, kind, target));
    }

    let mut children = super::super::significant_children(node).peekable();
    for (name, kind, token, offset) in [
        ("RequiredObject", "GameObject", 0x148e_d75d, 0xc),
        ("ForbiddenUpgrade", "UpgradeTemplate", 0x26fa_cfc7, 0x14),
        ("NeededUpgrade", "UpgradeTemplate", 0x26fa_cfc7, 0x1c),
    ] {
        let mut ids = Vec::new();
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            ids.push(weak_reference(child, kind, data, reference_types)?);
        }
        let table = id_list(data, start + offset, ids)?;
        entries.push(Entry::named(name, token, Target::Table(table)));
    }
    let filter = match children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter"))) {
        Some(child) => Target::Table(encode_filter(child, start + 0x24, data, reference_types)?),
        None => Target::Null,
    };
    if let Some(child) = children.next() {
        return Err(invalid(
            path,
            format!(
                "unexpected GameDependency child {}",
                child.tag_name().name()
            ),
        ));
    }
    entries.push(Entry::named("ObjectFilter", 0xe46e_a3b2, filter));
    Ok(Target::Table(Table::record(entries)))
}

fn skirmish(node: Node<'_, '_>, data: &mut BinaryData<'_>) -> io::Result<Target> {
    validate_attributes(
        node,
        &["BaseBuildingLocation", "NearResourceNodeType"],
        data.path,
    )?;
    let location = super::super::global_ui::enum_value(
        node.attribute("BaseBuildingLocation").unwrap_or("FRONT"),
        &[
            "FRONT",
            "SIDE",
            "BACK",
            "SPREAD",
            "CENTER",
            "DEFENSE",
            "NEAR_RESOURCE_NODE",
        ],
        data.path,
    )?;
    let resource_type = super::super::global_ui::enum_value(
        node.attribute("NearResourceNodeType").unwrap_or("ORE"),
        &["TIBERIUM", "ORE", "SIGMA", "ENERGY"],
        data.path,
    )?;
    let mut children = super::super::significant_children(node).peekable();
    let construction = children
        .next_if(|child| child.has_tag_name((SAGE_NS, "ConstructionBaseSelectionCriteria")));
    let resource = children.next_if(|child| {
        child.has_tag_name((SAGE_NS, "ResourceNodeBaseClosenessTestSelectionCriteria"))
    });
    if let Some(child) = children.next() {
        return Err(invalid(
            data.path,
            format!(
                "unsupported SkirmishAIInformation child {}",
                child.tag_name().name()
            ),
        ));
    }
    let start = data.allocate(0x1c8, 0x58)?;
    for (offset, value) in [
        (0, location),
        (8, 3),
        (0x10, 0x3fa6_6666),
        (0x18, 0xe),
        (0x1c, 3),
        (0x28, 0x3f80_0000),
        (0x2c, resource_type),
        (0x30, 0x4396_0000),
        (0x50, 1),
    ] {
        data.word(start + offset, value);
    }
    let fields = [
        ("BaseBuildingLocation", 0xce5b_5436, Some(0)),
        ("AdditionalRotation", 0x4e16_ea4d, Some(4)),
        ("BaseBuildingMaxNudges", 0x23e6_50b8, Some(8)),
        (
            "BlockIfUnpackTargetBuildingCannotBeRebuilt",
            0x77e3_f4f7,
            Some(0x50),
        ),
        ("UnitBuilderStandardCombatUnit", 0x77e3_f4f7, Some(0x51)),
        ("TargetDuringCrush", 0x77e3_f4f7, Some(0x52)),
        ("CrushSpeedBonus", 0x6458_64f3, Some(0xc)),
        ("OutOfRangePenalty", 0x6458_64f3, Some(0x10)),
        ("ContainedMembersProvideWeapon", 0x77e3_f4f7, Some(0x53)),
        ("ConquerMetricsOverrideDPS", 0x6458_64f3, None),
        ("ConquerMetricsOverrideDamageType", 0x4648_6ff1, Some(0x18)),
        ("ConquerMetricsOverrideAntiMask", 0xc8c6_9390, Some(0x1c)),
        (
            "ConquerMetricsOverrideForbiddenAntiMask",
            0xc8c6_9390,
            Some(0x20),
        ),
        ("ConquerMetricsOverrideDefendingAntiMask", 0xc8c6_9390, None),
        (
            "ConquerMetricsDpsBonusAgainstDefenses",
            0x6458_64f3,
            Some(0x28),
        ),
        ("NearResourceNodeType", 0x071b_12c5, Some(0x2c)),
        ("DodgeSafetyDistance", 0x6458_64f3, Some(0x30)),
        ("OverrideInnerDistanceToTarget", 0x6458_64f3, None),
        ("OverrideOuterDistanceToTarget", 0x6458_64f3, None),
        ("CanFireUpCliffs", 0x77e3_f4f7, Some(0x54)),
        ("DoesSecondaryDamage", 0x77e3_f4f7, Some(0x55)),
    ];
    let mut entries = fields
        .into_iter()
        .map(|(name, kind, offset)| {
            Entry::named(
                name,
                kind,
                offset.map_or(Target::Null, |offset| Target::Data(start + offset)),
            )
        })
        .collect::<Vec<_>>();
    entries.push(Entry::named(
        "AIContextualPower",
        0xcba6_c0c2,
        Target::Table(Table::array(start + 0x3c, [])),
    ));
    for (name, criteria, pointer) in [
        ("ConstructionBaseSelectionCriteria", construction, 0x44),
        (
            "ResourceNodeBaseClosenessTestSelectionCriteria",
            resource,
            0x48,
        ),
    ] {
        let target = match criteria {
            Some(criteria) => base_selection(criteria, start + pointer, data)?,
            None => Target::Null,
        };
        entries.push(Entry::named(name, 0xc153_d5e0, target));
    }
    entries.push(Entry::named("DodgeWithUnits", 0xe46e_a3b2, Target::Null));
    Ok(Target::Table(Table::record(entries)))
}

fn base_selection(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Target> {
    let start = data.payload.len();
    super::super::ai_personality::write_base_criteria(node, pointer, data)?;
    Ok(Target::Table(Table::record(
        super::draws::fields(
            start,
            &[
                ("RequiredBaseTypes", 0xd743_1325, 0),
                ("ExcludedBaseTypes", 0xd743_1325, 4),
                ("PreferredBaseTypes", 0xd743_1325, 8),
                ("PreferredNotBaseTypes", 0xd743_1325, 12),
                ("SortOrder", 0xb0e6_6c4e, 16),
            ],
        )
        .collect(),
    )))
}

pub(super) fn encode_crusher(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Option<Target>> {
    let Some(node) = find(node, "CrusherInfo")? else {
        return Ok(None);
    };
    const BYTE: u32 = 0x371e_5dc2;
    const BOOL: u32 = 0x77e3_f4f7;
    const REAL: u32 = 0x6458_64f3;
    const FIELDS: &[(&str, u32, usize, &str)] = &[
        ("CrusherLevel", BYTE, 0x1c, "0"),
        ("CrushableLevel", BYTE, 0x1d, "127"),
        ("CrushKnockback", 0x23e6_50b8, 0, "0"),
        ("CrushZFactor", REAL, 4, "1"),
        ("CrushEqualLevelProps", BOOL, 0x1e, "false"),
        ("UseCrushAttack", BOOL, 0x1f, "true"),
        ("CrushOnlyWhileCharging", BOOL, 0x20, "false"),
        ("MinCrushVelocityPercent", REAL, 8, "1"),
        ("CrushDecelerationPercent", REAL, 12, "0"),
        ("CrushAllies", BOOL, 0x21, "false"),
        ("CrushWeapon", 0xd046_dd22, 16, ""),
        ("CrushWhileStationary", BOOL, 0x22, "false"),
    ];
    let path = data.path;
    let allowed: Vec<_> = FIELDS
        .iter()
        .map(|&(name, ..)| name)
        .chain(["id"])
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(0x228, 0x24)?;
    let mut entries = Vec::with_capacity(FIELDS.len() + 1);
    for &(name, kind, relative, default) in FIELDS {
        let offset = start + relative;
        let value = node.attribute(name).unwrap_or(default);
        match kind {
            BYTE => {
                data.payload[offset] = value
                    .trim()
                    .parse::<i8>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?
                    as u8
            }
            BOOL => {
                data.payload[offset] = super::draws::boolean(node, name, default == "true", path)?
            }
            _ => {
                let word = match kind {
                    REAL if name.ends_with("Percent") => {
                        super::super::percentage(value, name, path)?.to_bits()
                    }
                    REAL => super::super::finite_number(value, name, path)?.to_bits(),
                    0x23e6_50b8 => value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| invalid(path, "invalid CrushKnockback"))?
                        as u32,
                    _ => super::passive_area::reference(
                        value,
                        "WeaponTemplate",
                        path,
                        references,
                        reference_types,
                    )?,
                };
                data.word(offset, word);
            }
        }
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    let children: Vec<_> = super::super::significant_children(node).collect();
    let count = start + 0x14;
    let first = data.list(count, children.len(), 8)?;
    let mut items = Vec::with_capacity(children.len());
    for (index, child) in children.into_iter().enumerate() {
        if !child.has_tag_name((SAGE_NS, "ExtraCrushLevels")) || has_content(child) {
            return Err(invalid(path, "unexpected CrusherInfo child"));
        }
        validate_attributes(
            child,
            &["CrusherLevel", "CrushableLevel", "ModelConditionMatch"],
            path,
        )?;
        let offset = first + index * 8;
        let flag = required(child, "ModelConditionMatch", path)?;
        let value = if flag == "INVALID" {
            u32::MAX
        } else {
            super::draws::model_condition_index(flag, path)? as u32
        };
        data.word(offset, value);
        for (name, default, field) in [("CrusherLevel", "0", 4), ("CrushableLevel", "127", 5)] {
            data.payload[offset + field] = child
                .attribute(name)
                .unwrap_or(default)
                .trim()
                .parse::<i8>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?
                as u8;
        }
        items.push((
            0,
            Target::Table(Table::record(
                super::draws::fields(
                    offset,
                    &[
                        ("CrusherLevel", BYTE, 4),
                        ("CrushableLevel", BYTE, 5),
                        ("ModelConditionMatch", 0x1572_3e0b, 0),
                    ],
                )
                .collect(),
            )),
        ));
    }
    entries.push(Entry::named(
        "ExtraCrushLevels",
        0x9091_f97b,
        Target::Table(Table::array(count, items)),
    ));
    Ok(Some(Target::Table(Table::record(entries))))
}

pub(super) fn encode_vision(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<Target>> {
    let Some(child) = find(node, "VisionInfo")? else {
        return Ok(None);
    };
    const FIELDS: [(&str, &str); 10] = [
        ("VisionRange", "0"),
        ("ShroudClearingRange", "0"),
        ("ShroudClearingHeight", "50"),
        ("VisionSide", "100"),
        ("VisionRear", "100"),
        ("VisionBonusTestRadius", "100"),
        ("VisionBonusTestSegments", "8"),
        ("VisionBonusPercentPerFoot", "0"),
        ("MaxVisionBonusPercent", "50"),
        ("MinVisionBonusPercent", "-50"),
    ];
    let path = data.path;
    validate_attributes(child, &FIELDS.map(|(name, _)| name), path)?;
    if has_content(child) {
        return Err(invalid(path, "VisionInfo cannot contain content"));
    }
    let start = data.allocate(0x224, 0x28)?;
    let mut entries = Vec::with_capacity(FIELDS.len());
    for (index, (name, default)) in FIELDS.into_iter().enumerate() {
        let value = child.attribute(name).unwrap_or(default);
        let (kind, word) = match index {
            6 => (
                0x7631_77a4,
                value
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?,
            ),
            3 | 4 | 7..=9 => (
                0x6458_64f3,
                super::super::percentage(value, name, path)?.to_bits(),
            ),
            _ => (0x6458_64f3, finite_float(value, name, path)?.to_bits()),
        };
        let offset = start + index * 4;
        data.word(offset, word);
        entries.push(Entry::named(name, kind, Target::Data(offset)));
    }
    Ok(Some(Target::Table(Table::record(entries))))
}

pub(super) fn encode_geometry(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<Target>> {
    let Some(child) = find(node, "Geometry")? else {
        return Ok(None);
    };
    validate_attributes(child, &["IsSmall"], data.path)?;
    use super::super::significant_children;

    let mut children = significant_children(child).peekable();
    let anchor = children.next_if(|node| node.has_tag_name((SAGE_NS, "RotationAnchorOffset")));
    let placement = children.next_if(|node| node.has_tag_name((SAGE_NS, "PlacementShape")));
    let mut shapes = Vec::new();
    while let Some(shape) = children.next_if(|node| node.has_tag_name((SAGE_NS, "Shape"))) {
        shapes.push(shape);
    }
    if shapes.is_empty() {
        return Err(invalid(data.path, "Geometry requires Shape children"));
    }
    let mut contacts = Vec::new();
    while let Some(contact) = children.next_if(|node| node.has_tag_name((SAGE_NS, "ContactPoint")))
    {
        contacts.push(contact);
    }
    if children.next().is_some() {
        return Err(invalid(data.path, "unexpected Geometry child"));
    }

    let start = data.allocate(0x1e8, 0x1c)?;
    data.payload[start + 0x18] = super::draws::boolean(child, "IsSmall", false, data.path)?;
    let anchor = if let Some(anchor) = anchor {
        let offset = data.allocate(start, 8)?;
        encode_geometry_vector(anchor, offset, &["x", "y"], data)?;
        Target::Data(offset)
    } else {
        Target::Null
    };
    let placement = if let Some(shape) = placement {
        let offset = data.allocate(start + 4, 0x30)?;
        Target::Table(encode_geometry_shape(shape, offset, data)?)
    } else {
        Target::Null
    };
    let items = data.list(start + 8, shapes.len(), 0x30)?;
    let mut entries = Vec::with_capacity(shapes.len());
    for (index, shape) in shapes.into_iter().enumerate() {
        let table = encode_geometry_shape(shape, items + index * 0x30, data)?;
        entries.push((0, Target::Table(table)));
    }
    let items = data.list(start + 0x10, contacts.len(), 0x14)?;
    let mut points = Vec::with_capacity(contacts.len());
    for (index, contact) in contacts.into_iter().enumerate() {
        let offset = items + index * 0x14;
        validate_attributes(contact, &["Label"], data.path)?;
        if let Some(label) = contact.attribute("Label") {
            data.string(offset, label)?;
        }
        let mut children = significant_children(contact);
        let pos = super::super::required_element(&mut children, "Pos", data.path)?;
        if children.next().is_some() {
            return Err(invalid(data.path, "unexpected Geometry ContactPoint child"));
        }
        encode_geometry_vector(pos, offset + 8, &["x", "y", "z"], data)?;
        points.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Label", 0x1a29_33aa, Target::Data(offset)),
                Entry::named("Pos", 0x2936_8077, Target::Data(offset + 8)),
            ])),
        ));
    }
    Ok(Some(Target::Table(Table::record(vec![
        Entry::named("IsSmall", 0x77e3_f4f7, Target::Data(start + 0x18)),
        Entry::named("RotationAnchorOffset", 0x4ae3_1f24, anchor),
        Entry::new(0x9e21_d104, 0xe721_0555, placement),
        Entry::named(
            "Shape",
            0xe721_0555,
            Target::Table(Table::array(start + 8, entries)),
        ),
        Entry::named(
            "ContactPoint",
            0x6b8e_e8ef,
            Target::Table(Table::array(start + 0x10, points)),
        ),
    ]))))
}

fn encode_geometry_vector(
    node: Node<'_, '_>,
    offset: usize,
    axes: &[&str],
    data: &mut BinaryData<'_>,
) -> io::Result<()> {
    validate_attributes(node, axes, data.path)?;
    if has_content(node) {
        return Err(invalid(data.path, "Geometry vector cannot contain content"));
    }
    for (index, &axis) in axes.iter().enumerate() {
        let value =
            super::super::finite_number(node.attribute(axis).unwrap_or("0"), axis, data.path)?;
        data.word(offset + index * 4, value.to_bits());
    }
    Ok(())
}

pub(super) fn encode_geometry_shape(
    shape: Node<'_, '_>,
    offset: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        shape,
        &[
            "Type",
            "ContactPointGeneration",
            "Name",
            "Active",
            "UsedForHealthBox",
            "MajorRadius",
            "MinorRadius",
            "Other",
            "Height",
            "FrontAngle",
        ],
        path,
    )?;
    use super::super::global_ui::enum_value;
    let kind = enum_value(
        shape.attribute("Type").unwrap_or("SPHERE"),
        &["SPHERE", "BOX", "CYLINDER", "RING"],
        path,
    )?;
    let contact = enum_value(
        shape.attribute("ContactPointGeneration").unwrap_or("NONE"),
        &["NONE", "VEHICLE", "STRUCTURE", "INFANTRY", "SQUAD_MEMBER"],
        path,
    )?;
    data.word(offset, kind);
    data.word(offset + 4, contact);
    for (name, field) in [("Name", 8), ("Other", 0x18)] {
        if let Some(value) = shape.attribute(name) {
            data.string(offset + field, value)?;
        }
    }
    for (name, field) in [
        ("MajorRadius", 0x10),
        ("MinorRadius", 0x14),
        ("Height", 0x20),
    ] {
        let value = super::super::finite_number(shape.attribute(name).unwrap_or("1"), name, path)?;
        data.word(offset + field, value.to_bits());
    }
    if let Some(value) = shape.attribute("FrontAngle").map(str::trim) {
        let angle = if let Some(number) = value.strip_suffix('d') {
            super::super::finite_number(number, "FrontAngle", path)?.to_radians()
        } else if let Some(number) = value.strip_suffix('r') {
            super::super::finite_number(number, "FrontAngle", path)?
        } else {
            return Err(invalid(path, "invalid FrontAngle"));
        };
        data.word(offset + 0x24, angle.to_bits());
    }
    data.payload[offset + 0x2c] = super::draws::boolean(shape, "Active", true, path)?;
    data.payload[offset + 0x2d] = super::draws::boolean(shape, "UsedForHealthBox", true, path)?;

    let mut children = super::super::significant_children(shape).peekable();
    let coord = children.next_if(|node| node.has_tag_name((SAGE_NS, "Offset")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected Geometry Shape child"));
    }
    let coord_target = if let Some(coord) = coord {
        let coord_start = data.allocate(offset + 0x28, 12)?;
        encode_geometry_vector(coord, coord_start, &["x", "y", "z"], data)?;
        Target::Data(coord_start)
    } else {
        Target::Null
    };
    let attrs = vec![
        Entry::named("Type", 0xb0d4_9164, Target::Data(offset)),
        Entry::named(
            "ContactPointGeneration",
            0x4655_08ce,
            Target::Data(offset + 4),
        ),
        Entry::named("Name", 0x1a29_33aa, Target::Data(offset + 8)),
        Entry::named("Active", 0x77e3_f4f7, Target::Data(offset + 0x2c)),
        Entry::named("UsedForHealthBox", 0x77e3_f4f7, Target::Data(offset + 0x2d)),
        Entry::named("MajorRadius", 0x6458_64f3, Target::Data(offset + 0x10)),
        Entry::named("MinorRadius", 0x6458_64f3, Target::Data(offset + 0x14)),
        Entry::named("Other", 0x1a29_33aa, Target::Data(offset + 0x18)),
        Entry::named("Height", 0x6458_64f3, Target::Data(offset + 0x20)),
        Entry::named("FrontAngle", 0x4e16_ea4d, Target::Data(offset + 0x24)),
        Entry::named("Offset", 0x2936_8077, coord_target),
    ];
    Ok(Table::record(attrs))
}

fn finite_float(value: &str, name: &str, path: &std::path::Path) -> io::Result<f32> {
    let number = value
        .parse::<f32>()
        .map_err(|_| invalid(path, format!("invalid {name}")))?;
    if !number.is_finite() {
        return Err(invalid(path, format!("invalid {name}")));
    }
    Ok(number)
}

fn find<'a, 'input>(node: Node<'a, 'input>, name: &str) -> io::Result<Option<Node<'a, 'input>>> {
    let mut found = node
        .children()
        .filter(|child| child.has_tag_name((SAGE_NS, name)));
    let first = found.next();
    if found.next().is_some() {
        return Err(invalid(
            std::path::Path::new(name),
            format!("duplicate {name}"),
        ));
    }
    Ok(first)
}

pub(super) fn encode_buildability(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Option<Target>> {
    use super::super::global_ui::enum_value;
    use super::super::{finite_number, significant_children};
    use super::draws::{boolean, write_model_conditions};
    use super::parking::encode_filter;
    use super::secondary_damage::write_status_flags;

    let nodes: Vec<_> = significant_children(node)
        .filter(|child| child.has_tag_name((SAGE_NS, "ProjectedBuildabilityInfo")))
        .collect();
    if nodes.is_empty() {
        return Ok(None);
    }
    let path = data.path;
    let count = 0x22c;
    let first = data.list(count, nodes.len(), 0x70)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(
            node,
            &[
                "Radius",
                "RadiusY",
                "BuildPlacementTypes",
                "StatusToReject",
                "ModelConditionsToReject",
                "AllowedBuildabilityHeightVariation",
                "PrimaryBuidability",
            ],
            path,
        )?;
        let start = first + index * 0x70;
        let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
        data.word(start, radius.to_bits());
        let radius_y = if let Some(value) = node.attribute("RadiusY") {
            let value = finite_number(value, "RadiusY", path)?;
            let offset = data.allocate(start + 4, 4)?;
            data.word(offset, value.to_bits());
            Target::Data(offset)
        } else {
            Target::Null
        };
        let mut flags = 0;
        for name in node
            .attribute("BuildPlacementTypes")
            .unwrap_or("INVALID")
            .split_ascii_whitespace()
        {
            flags |= 1
                << enum_value(
                    name,
                    &[
                        "INVALID",
                        "MAIN_STRUCTURE",
                        "OTHER_STRUCTURE",
                        "TIBERIUM_FIELD",
                        "BLOCKED",
                    ],
                    path,
                )?;
        }
        data.word(start + 8, flags);
        write_status_flags(
            node.attribute("StatusToReject").unwrap_or(""),
            start + 0xc,
            data,
        )?;
        write_model_conditions(
            node.attribute("ModelConditionsToReject")
                .unwrap_or("STRUCTURE_UNPACKING"),
            start + 0x28,
            data,
        )?;
        let height = finite_number(
            node.attribute("AllowedBuildabilityHeightVariation")
                .unwrap_or("50"),
            "AllowedBuildabilityHeightVariation",
            path,
        )?;
        data.word(start + 0x64, height.to_bits());
        data.payload[start + 0x6c] = boolean(node, "PrimaryBuidability", true, path)?;
        let mut children = significant_children(node).peekable();
        let filter =
            match children.next_if(|child| child.has_tag_name((SAGE_NS, "AllowedObjectFilter"))) {
                Some(child) => {
                    Target::Table(encode_filter(child, start + 0x68, data, reference_types)?)
                }
                None => Target::Null,
            };
        if children.next().is_some() {
            return Err(invalid(path, "unexpected ProjectedBuildabilityInfo child"));
        }
        items.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Radius", 0x6458_64f3, Target::Data(start)),
                Entry::named("RadiusY", 0x6458_64f3, radius_y),
                Entry::named("BuildPlacementTypes", 0xf446_cf6f, Target::Data(start + 8)),
                Entry::named("StatusToReject", 0xa7bd_811e, Target::Data(start + 0xc)),
                Entry::named(
                    "ModelConditionsToReject",
                    0x67db_bfc9,
                    Target::Data(start + 0x28),
                ),
                Entry::named(
                    "AllowedBuildabilityHeightVariation",
                    0x6458_64f3,
                    Target::Data(start + 0x64),
                ),
                Entry::named(
                    "PrimaryBuidability",
                    0x77e3_f4f7,
                    Target::Data(start + 0x6c),
                ),
                Entry::named("AllowedObjectFilter", 0xe46e_a3b2, filter),
            ])),
        ));
    }
    Ok(Some(Target::Table(Table::array(count, items))))
}

#[cfg(test)]
mod tests {
    use super::super::super::Marshaled;
    use super::super::super::tokenized;
    use super::*;
    use roxmltree::Document;
    use std::path::Path;

    #[test]
    fn child_data_matches_ea_oracles() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        for (name, xml, oracle) in [
            (
                "display",
                include_str!("../../../../tests/fixtures/game-object-children-display.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-display.bin")
                    .as_slice(),
            ),
            (
                "dependency",
                include_str!("../../../../tests/fixtures/game-object-children-dependency.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-dependency.bin")
                    .as_slice(),
            ),
            (
                "resource",
                include_str!("../../../../tests/fixtures/game-object-children-resource.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-resource.bin")
                    .as_slice(),
            ),
            (
                "armor",
                include_str!("../../../../tests/fixtures/game-object-children-armor.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-armor.bin")
                    .as_slice(),
            ),
            (
                "skirmish",
                include_str!("../../../../tests/fixtures/game-object-children-skirmish.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-skirmish.bin")
                    .as_slice(),
            ),
        ] {
            let document = Document::parse(xml).unwrap();
            let node = document
                .root_element()
                .children()
                .find(Node::is_element)
                .unwrap();
            let mut root = BARE[4 + 0x7b4..4 + 0xa00].to_vec();
            root[0x1dc..0x1e0].fill(0);
            let mut data = BinaryData {
                payload: root,
                relocations: Vec::new(),
                path: Path::new("children.xml"),
            };
            let mut refs = Vec::new();
            let mut types = Vec::new();
            let targets = encode(node, &mut data, &mut refs, &mut types).unwrap();
            let oracle = &oracle[4..];
            let start = u32::from_le_bytes(oracle[8..12].try_into().unwrap()) as usize;
            for pointer in data.relocations.clone() {
                let offset = pointer as usize;
                let target =
                    u32::from_le_bytes(data.payload[offset..offset + 4].try_into().unwrap());
                data.word(offset, target + start as u32);
            }
            let body = u32::from_le_bytes(oracle[start + 0x1dc..start + 0x1e0].try_into().unwrap())
                as usize;
            let mut expected = oracle[start..body].to_vec();
            expected[0x1dc..0x1e0].fill(0);
            assert_eq!(data.payload, expected, "{name}");
            assert_eq!(targets.len(), 1, "{name}");
        }
    }

    #[test]
    fn late_child_data_matches_ea_oracles() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        for (name, xml, oracle) in [
            (
                "geometry",
                include_str!("../../../../tests/fixtures/game-object-children-geometry.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-geometry.bin")
                    .as_slice(),
            ),
            (
                "vision",
                include_str!("../../../../tests/fixtures/game-object-children-vision.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-vision.bin")
                    .as_slice(),
            ),
        ] {
            let document = Document::parse(xml).unwrap();
            let node = document
                .root_element()
                .children()
                .find(Node::is_element)
                .unwrap();
            let mut payload = BARE[4 + 0x7b4..].to_vec();
            payload[0x1dc..0x1e0].copy_from_slice(&(0x24c_u32).to_le_bytes());
            let mut data = BinaryData {
                payload,
                relocations: vec![0x1dc],
                path: Path::new("children.xml"),
            };
            let target = if name == "geometry" {
                encode_geometry(node, &mut data)
            } else {
                encode_vision(node, &mut data)
            }
            .unwrap();
            assert!(target.is_some(), "{name}");
            let oracle = &oracle[4..];
            let start = u32::from_le_bytes(oracle[8..12].try_into().unwrap()) as usize;
            for pointer in data.relocations.clone() {
                let offset = pointer as usize;
                let target =
                    u32::from_le_bytes(data.payload[offset..offset + 4].try_into().unwrap());
                data.word(offset, target + start as u32);
            }
            assert_eq!(data.payload, oracle[start..], "{name}");
        }
    }

    #[test]
    fn child_token_layouts_match_ea_oracles() {
        const BARE: &[u8] = include_bytes!("../../../../tests/fixtures/game-object-bare.bin");
        const CASES: [(&str, &str, &[u8], &[u8]); 8] = [
            (
                "display",
                include_str!("../../../../tests/fixtures/game-object-children-display.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-display.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-display.relo"),
            ),
            (
                "dependency",
                include_str!("../../../../tests/fixtures/game-object-children-dependency.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-dependency.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-dependency.relo"),
            ),
            (
                "resource",
                include_str!("../../../../tests/fixtures/game-object-children-resource.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-resource.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-resource.relo"),
            ),
            (
                "armor",
                include_str!("../../../../tests/fixtures/game-object-children-armor.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-armor.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-armor.relo"),
            ),
            (
                "skirmish",
                include_str!("../../../../tests/fixtures/game-object-children-skirmish.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-skirmish.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-skirmish.relo"),
            ),
            (
                "skirmish-front",
                include_str!("../../../../tests/fixtures/game-object-skirmish-front.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-skirmish-front.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-skirmish-front.relo"),
            ),
            (
                "geometry",
                include_str!("../../../../tests/fixtures/game-object-children-geometry.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-geometry.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-geometry.relo"),
            ),
            (
                "vision",
                include_str!("../../../../tests/fixtures/game-object-children-vision.xml"),
                include_bytes!("../../../../tests/fixtures/game-object-children-vision.bin"),
                include_bytes!("../../../../tests/fixtures/game-object-children-vision.relo"),
            ),
        ];
        for (name, xml, oracle, relocations) in CASES {
            let document = Document::parse(xml).unwrap();
            let node = document
                .root_element()
                .children()
                .find(Node::is_element)
                .unwrap();
            let path = Path::new("children.xml");
            let bare_data = &BARE[4 + 0x7b4..];
            let mut table = Table::from_layout(&BARE[4..4 + 0x7b4], bare_data, path).unwrap();
            let mut data = BinaryData {
                payload: bare_data[..0x24c].to_vec(),
                relocations: Vec::new(),
                path,
            };
            data.word(0x1dc, 0);
            let target = if name == "geometry" || name == "vision" {
                let body = data.allocate(0x1dc, 0x4c).unwrap();
                data.payload[body..body + 0x4c].copy_from_slice(&bare_data[0x24c..0x298]);
                if name == "geometry" {
                    encode_geometry(node, &mut data)
                } else {
                    encode_vision(node, &mut data)
                }
                .unwrap()
                .map(|target| {
                    (
                        if name == "geometry" {
                            "Geometry"
                        } else {
                            "VisionInfo"
                        },
                        target,
                    )
                })
                .into_iter()
                .collect::<Vec<_>>()
            } else {
                let targets = encode(node, &mut data, &mut Vec::new(), &mut Vec::new()).unwrap();
                table.shift_data_from(0x24c, data.payload.len() - 0x24c);
                let body = data.allocate(0x1dc, 0x4c).unwrap();
                data.payload[body..body + 0x4c].copy_from_slice(&bare_data[0x24c..0x298]);
                targets
            };
            for (key, value) in target {
                assert!(table.replace_target(key, value), "{name}: {key}");
            }
            data.relocations.sort_unstable();
            let result = tokenized::finish(table, data, Marshaled::default()).unwrap();
            assert_eq!(result.payload, oracle[4..], "{name} payload");
            let mut expected = Vec::new();
            for pointer in &relocations[4..] {
                let _ = pointer;
            }
            expected.extend_from_slice(&relocations[4..]);
            assert_eq!(
                super::super::super::encode_words(result.relocations),
                expected,
                "{name} relocations"
            );
        }
    }
}
