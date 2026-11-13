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
        targets.push(("GameDependency", dependency(child, data, reference_types)?));
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
                for flag in conditions.split_ascii_whitespace() {
                    match flag {
                        "SECONDARY_DAMAGE" => {
                            data.word(offset + 8, data.payload[offset + 8] as u32 | (1 << 13))
                        }
                        _ => {
                            return Err(invalid(
                                data.path,
                                format!("unsupported ArmorSet condition {flag}"),
                            ));
                        }
                    }
                }
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
    if let Some(child) = find(node, "SkirmishAIInformation")? {
        targets.push(("SkirmishAIInformation", skirmish(child, data)?));
    }
    Ok(targets)
}

fn dependency(
    node: Node<'_, '_>,
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
    let start = data.allocate(0x190, 0x28)?;
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
    validate_attributes(node, &["BaseBuildingLocation"], data.path)?;
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
    let mut children = node.children().filter(Node::is_element);
    let criteria = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "ConstructionBaseSelectionCriteria")))
        .ok_or_else(|| {
            invalid(
                data.path,
                "SkirmishAIInformation requires ConstructionBaseSelectionCriteria",
            )
        })?;
    if children.next().is_some() {
        return Err(invalid(
            data.path,
            "unsupported SkirmishAIInformation child",
        ));
    }
    validate_attributes(
        criteria,
        &["ExcludedBaseTypes", "PreferredBaseTypes", "SortOrder"],
        data.path,
    )?;
    if has_content(criteria) {
        return Err(invalid(
            data.path,
            "ConstructionBaseSelectionCriteria cannot contain content",
        ));
    }
    for (name, expected) in [
        ("ExcludedBaseTypes", "NO_BUILD_RADIUS CAPTURED"),
        ("PreferredBaseTypes", "MAIN_BASE"),
        ("SortOrder", "PREFER_OLDEST"),
    ] {
        if criteria.attribute(name) != Some(expected) {
            return Err(invalid(
                data.path,
                format!("unsupported ConstructionBaseSelectionCriteria {name}"),
            ));
        }
    }
    let start = data.allocate(0x1c8, 0x58)?;
    for (offset, value) in [
        (0, location),
        (8, 3),
        (0x10, 0x3fa6_6666),
        (0x18, 0xe),
        (0x1c, 3),
        (0x28, 0x3f80_0000),
        (0x2c, 1),
        (0x30, 0x4396_0000),
        (0x50, 1),
    ] {
        data.word(start + offset, value);
    }
    let selection = data.allocate(start + 0x44, 0x14)?;
    data.word(selection + 4, 0x204);
    data.word(selection + 8, 1);
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
    entries.push(Entry::named(
        "ConstructionBaseSelectionCriteria",
        0xc153_d5e0,
        Target::Table(Table::record(vec![
            Entry::named("RequiredBaseTypes", 0xd743_1325, Target::Data(selection)),
            Entry::named(
                "ExcludedBaseTypes",
                0xd743_1325,
                Target::Data(selection + 4),
            ),
            Entry::named(
                "PreferredBaseTypes",
                0xd743_1325,
                Target::Data(selection + 8),
            ),
            Entry::named(
                "PreferredNotBaseTypes",
                0xd743_1325,
                Target::Data(selection + 12),
            ),
            Entry::named("SortOrder", 0xb0e6_6c4e, Target::Data(selection + 16)),
        ])),
    ));
    entries.push(Entry::named(
        "ResourceNodeBaseClosenessTestSelectionCriteria",
        0xc153_d5e0,
        Target::Null,
    ));
    entries.push(Entry::named("DodgeWithUnits", 0xe46e_a3b2, Target::Null));
    Ok(Target::Table(Table::record(entries)))
}

pub(super) fn encode_vision(
    node: Node<'_, '_>,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<Target>> {
    let Some(child) = find(node, "VisionInfo")? else {
        return Ok(None);
    };
    validate_attributes(child, &["VisionRange", "ShroudClearingRange"], data.path)?;
    if has_content(child) {
        return Err(invalid(data.path, "VisionInfo cannot contain content"));
    }
    let start = data.allocate(0x224, 0x28)?;
    let defaults = [
        0.0f32.to_bits(),
        0.0f32.to_bits(),
        50.0f32.to_bits(),
        1.0f32.to_bits(),
        1.0f32.to_bits(),
        100.0f32.to_bits(),
        8,
        0,
        0.5f32.to_bits(),
        (-0.5f32).to_bits(),
    ];
    for (index, word) in defaults.into_iter().enumerate() {
        data.word(start + 4 * index, word);
    }
    for (name, offset) in [("VisionRange", 0), ("ShroudClearingRange", 4)] {
        if let Some(value) = child.attribute(name) {
            data.word(
                start + offset,
                finite_float(value, name, data.path)?.to_bits(),
            );
        }
    }
    let names = [
        "VisionRange",
        "ShroudClearingRange",
        "ShroudClearingHeight",
        "VisionSide",
        "VisionRear",
        "VisionBonusTestRadius",
        "VisionBonusTestSegments",
        "VisionBonusPercentPerFoot",
        "MaxVisionBonusPercent",
        "MinVisionBonusPercent",
    ];
    let entries = names
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            Entry::named(
                name,
                if index == 6 { 0x7631_77a4 } else { 0x6458_64f3 },
                Target::Data(start + index * 4),
            )
        })
        .collect();
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
    let shapes = child
        .children()
        .filter(Node::is_element)
        .collect::<Vec<_>>();
    if shapes.is_empty()
        || shapes
            .iter()
            .any(|shape| !shape.has_tag_name((SAGE_NS, "Shape")))
    {
        return Err(invalid(data.path, "Geometry requires Shape children"));
    }
    let start = data.allocate(0x1e8, 0x1c)?;
    data.payload[start + 0x18] = super::draws::boolean(child, "IsSmall", false, data.path)?;
    let items = data.list(start + 8, shapes.len(), 0x30)?;
    let mut entries = Vec::new();
    for (index, shape) in shapes.iter().enumerate() {
        let shape = *shape;
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
        let offset = items + index * 0x30;
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
            let value =
                super::super::finite_number(shape.attribute(name).unwrap_or("1"), name, path)?;
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
            validate_attributes(coord, &["x", "y", "z"], path)?;
            if has_content(coord) {
                return Err(invalid(path, "Geometry Offset cannot contain content"));
            }
            let coord_start = data.allocate(offset + 0x28, 12)?;
            for (axis, field) in [("x", 0), ("y", 4), ("z", 8)] {
                let value =
                    super::super::finite_number(coord.attribute(axis).unwrap_or("0"), axis, path)?;
                data.word(coord_start + field, value.to_bits());
            }
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
        entries.push((0, Target::Table(Table::record(attrs))));
    }
    Ok(Some(Target::Table(Table::record(vec![
        Entry::named("IsSmall", 0x77e3_f4f7, Target::Data(start + 0x18)),
        Entry::named("RotationAnchorOffset", 0x4ae3_1f24, Target::Null),
        Entry::new(0x9e21_d104, 0xe721_0555, Target::Null),
        Entry::named(
            "Shape",
            0xe721_0555,
            Target::Table(Table::array(start + 8, entries)),
        ),
        Entry::named(
            "ContactPoint",
            0x6b8e_e8ef,
            Target::Table(Table::array(start + 0x10, [])),
        ),
    ]))))
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
