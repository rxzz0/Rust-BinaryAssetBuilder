use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, percentage, required_element, sage_time,
    significant_children, text_value, validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, module_base, optional_vector};
use super::draws::{boolean, fields};
use super::parking::{encode_filter, encode_inline_filter};
use super::secondary_damage::{status_index, write_status_flags};
use super::spawn::id_list;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const TRANSPORT: u32 = 0x578c_272f;
const OPEN: u32 = 0xd7cc_2e35;
const GARRISON: u32 = 0xbae3_bc7d;
const SLAUGHTER: u32 = 0xc194_7edc;
const BOOL: u32 = 0x77e3_f4f7;
const UINT: u32 = 0x7631_77a4;
const INT: u32 = 0x23e6_50b8;
const FLOAT: u32 = 0x6458_64f3;
const STATUS: u32 = 0xa7bd_811e;
const SOUND: u32 = 0x63d7_b468;
const TIME: u32 = 0x9986_69e8;
const DISABLED: u32 = 0x1965_70c9;
const OPEN_FIELDS: &[(&str, u32, usize, &str)] = &[
    ("ContainMax", UINT, 8, "0"),
    ("EnterSound", SOUND, 0xc, ""),
    ("ExitSound", SOUND, 0x10, ""),
    ("DamagePercentToUnits", FLOAT, 0x14, "0"),
    ("PassengersTestCollisionHeight", FLOAT, 0x18, "-1000"),
    ("PassengersInTurret", BOOL, 0x88, "false"),
    ("NumberOfExitPaths", INT, 0x1c, "1"),
    ("DoorOpenTime", UINT, 0x20, "1"),
    ("AllowOwnPlayerInsideOverride", BOOL, 0x89, "false"),
    ("AllowAlliesInside", BOOL, 0x8a, "true"),
    ("AllowEnemiesInside", BOOL, 0x8b, "true"),
    ("AllowNeutralInside", BOOL, 0x8c, "true"),
    ("PassDisabilityToRiders", BOOL, 0x8d, "false"),
    (
        "IgnoreDisabledBitsForRiders",
        DISABLED,
        0x24,
        "UNDERPOWERED EMP",
    ),
    ("ShowPips", BOOL, 0x8e, "true"),
    ("CollidePickup", BOOL, 0x8f, "true"),
    ("EjectPassengersOnDeath", BOOL, 0x90, "true"),
    ("KillPassengersOnDeath", BOOL, 0x91, "false"),
    ("HasObjectStatusOfContainedEntry", BOOL, 0x92, "true"),
    ("Enabled", BOOL, 0x93, "true"),
    ("ObjectStatusOfContained", STATUS, 0x28, ""),
    (
        "ObjectStatusWhileContaining",
        STATUS,
        0x44,
        "CONTAINER_OCCUPIED",
    ),
    ("ModifierRequiredTime", UINT, 0x60, "100"),
    ("KillIfEmptyTime", TIME, 0x64, "0s"),
];
const GARRISON_FLAGS: [(&str, u32, usize); 3] = [
    ("MobileGarrison", BOOL, 0x9c),
    ("ImmuneToClearBuildingAttacks", BOOL, 0x9d),
    ("ResetInitialTeamOnCapture", BOOL, 0x9e),
];

const DURATION: u32 = 0x6f6d_1ae7;
const STRING: u32 = 0x1a29_33aa;
const KIND_OF: u32 = 0x9afc_9193;
const CONDITION: u32 = 0x1572_3e0b;
const WEAPON: u32 = 0xd046_dd22;
const TRANSPORT_FIELDS: &[(&str, u32, usize, &str)] = &[
    ("ScatterNearbyOnExit", BOOL, 0x140, "true"),
    ("OrientLikeContainerOnExit", BOOL, 0x141, "false"),
    ("DestroyRidersWhoAreNotFreeToExit", BOOL, 0x142, "false"),
    ("ExitBone", STRING, 0x94, ""),
    ("HealthRegenPercentPerSec", FLOAT, 0x9c, "0"),
    ("ExitDelay", DURATION, 0xa0, "0"),
    ("TypeOneForWeaponState", KIND_OF, 0xa4, ""),
    ("TypeTwoForWeaponState", KIND_OF, 0xc8, ""),
    ("TypeThreeForWeaponState", KIND_OF, 0xec, ""),
    ("ForceOrientationContainer", BOOL, 0x143, "true"),
    ("ConditionForEntry", CONDITION, 0x110, ""),
    ("ShouldThrowOutPassengers", BOOL, 0x144, "false"),
    ("ThrowOutPassengersDelay", DURATION, 0x114, "0"),
    ("ThrowOutPassengersLandingWarhead", WEAPON, 0x118, ""),
    ("FadePassengerOnEnter", BOOL, 0x145, "false"),
    ("FadePassengerOnExit", BOOL, 0x146, "false"),
    ("EnterFadeTime", FLOAT, 0x11c, "0"),
    ("ExitFadeTime", FLOAT, 0x120, "0"),
    ("FadeReverse", BOOL, 0x147, "false"),
    ("ReleaseSnappyness", FLOAT, 0x124, "0.7"),
    ("ExtendedExitContainerChecks", BOOL, 0x148, "false"),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let (kind, size) = match node.tag_name().name() {
        "OpenContain" => (OPEN, 0x94),
        "TransportContain" => (TRANSPORT, 0x14c),
        "SlaughterHordeContain" => (SLAUGHTER, 0x144),
        _ => (GARRISON, 0xa0),
    };
    let mut allowed: Vec<_> = std::iter::once("id")
        .chain(OPEN_FIELDS.iter().map(|&(name, _, _, _)| name))
        .collect();
    match kind {
        TRANSPORT => allowed.extend(TRANSPORT_FIELDS.iter().map(|&(name, ..)| name)),
        GARRISON | SLAUGHTER => allowed.extend(GARRISON_FLAGS.map(|(name, _, _)| name)),
        _ => {}
    }
    if kind == SLAUGHTER {
        allowed.extend([
            "ExitDelay",
            "CashBackPercent",
            "CanAlwaysEnterStatus",
            "SlaughterFX",
        ]);
    }
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut children: Vec<_> = significant_children(node).collect();
    let slaughter = if kind == SLAUGHTER {
        Some(slaughter_fields(
            node,
            &mut children,
            start,
            data,
            references,
            reference_types,
        )?)
    } else {
        None
    };
    let ordered_imports = matches!(kind, OPEN | TRANSPORT);
    if ordered_imports {
        contain_attribute_references(node, start, data, references, reference_types)?;
    }
    // Transport stores derived blocks first, but EA hashes references in XML order.
    let mut modifiers = Vec::new();
    let mut modifier_reference_types = Vec::new();
    let mut derived_reference_types = Vec::new();
    let transport = if kind == TRANSPORT {
        for child in children
            .iter()
            .filter(|child| child.has_tag_name((SAGE_NS, "ModifierToGiveOnExit")))
        {
            modifiers.push(super::passive_area::reference(
                text_value(*child, path)?,
                "AttributeModifier",
                path,
                references,
                &mut modifier_reference_types,
            )?);
        }
        let first_derived = children
            .iter()
            .position(|child| {
                child.tag_name().namespace() == Some(SAGE_NS)
                    && matches!(
                        child.tag_name().name(),
                        "InitialPayload"
                            | "ThrowOutPassengersVelocity"
                            | "UpgradeCreationTrigger"
                            | "FadeFilter"
                    )
            })
            .unwrap_or(children.len());
        let derived = children.split_off(first_derived);
        Some(transport_fields(
            node,
            derived,
            start,
            data,
            references,
            &mut derived_reference_types,
        )?)
    } else {
        None
    };
    let mut open = vec![module_base(node, kind, start, data, true)?];
    for &(name, kind, offset, default) in OPEN_FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        let field = start + offset;
        match kind {
            SOUND if ordered_imports => {}
            BOOL => data.payload[field] = boolean(node, name, default == "true", path)?,
            STATUS => write_status_flags(value, field, data)?,
            _ => {
                let word = match kind {
                    UINT => value
                        .trim()
                        .parse::<u32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?,
                    INT => signed(value, name, path)?,
                    FLOAT if name == "DamagePercentToUnits" => {
                        percentage(value, name, path)?.to_bits()
                    }
                    FLOAT => finite_number(value, name, path)?.to_bits(),
                    TIME => sage_time(value, name, path)?.to_bits(),
                    SOUND => super::passive_area::reference(
                        value,
                        "BaseAudioEventInfo",
                        path,
                        references,
                        reference_types,
                    )?,
                    DISABLED => disabled_flags(value, path)?,
                    _ => unreachable!("OpenContain field type"),
                };
                data.word(field, word);
            }
        }
        open.push(Entry::named(name, kind, Target::Data(field)));
    }

    let mut children = children.into_iter().peekable();
    let filter = match children.next_if(|child| child.has_tag_name((SAGE_NS, "PassengerFilter"))) {
        Some(child) => Target::Table(encode_filter(child, start + 0x68, data, reference_types)?),
        None => Target::Null,
    };
    let die = match children.next_if(|child| child.has_tag_name((SAGE_NS, "DieMuxData"))) {
        Some(child) => {
            let offset = data.allocate(start + 0x6c, 0x2c)?;
            Target::Table(encode_die_mux(child, offset, data)?)
        }
        None => Target::Null,
    };
    let mut passengers = Vec::new();
    while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "PassengerData")))
    {
        passengers.push(child);
    }
    let first = data.list(start + 0x70, passengers.len(), 0x80)?;
    let mut passenger_items = Vec::with_capacity(passengers.len());
    for (index, child) in passengers.into_iter().enumerate() {
        let offset = first + index * 0x80;
        passenger_items.push((
            0,
            Target::Table(passenger_data(child, offset, data, reference_types)?),
        ));
    }
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ModifierToGiveOnExit")))
    {
        if transport.is_none() {
            modifiers.push(super::passive_area::reference(
                text_value(child, path)?,
                "AttributeModifier",
                path,
                references,
                reference_types,
            )?);
        }
    }
    reference_types.extend(modifier_reference_types);
    let modifiers = id_list(data, start + 0x78, modifiers)?;
    let mut members = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "MemberTemplateStatusInfo")))
    {
        members.push(child);
    }
    let first = data.list(start + 0x80, members.len(), 8)?;
    let mut member_items = Vec::with_capacity(members.len());
    for (index, child) in members.into_iter().enumerate() {
        named_leaf(child, &["ThingTemplate", "ObjectStatus"], path)?;
        let offset = first + index * 8;
        data.word(
            offset,
            object_id(
                required(child, "ThingTemplate", path)?,
                path,
                reference_types,
            )?,
        );
        let status = status_index(required(child, "ObjectStatus", path)?, path)?;
        data.word(offset + 4, status as u32);
        member_items.push((
            0,
            Target::Table(Table::record(
                fields(
                    offset,
                    &[
                        ("ThingTemplate", 0x148e_d75d, 0),
                        ("ObjectStatus", 0xfa0d_e777, 4),
                    ],
                )
                .collect(),
            )),
        ));
    }
    open.extend([
        Entry::named("PassengerFilter", 0xe46e_a3b2, filter),
        Entry::named("DieMuxData", 0x0084_001b, die),
        Entry::named(
            "PassengerData",
            0x5cb5_dd8b,
            Target::Table(Table::array(start + 0x70, passenger_items)),
        ),
        Entry::named(
            "ModifierToGiveOnExit",
            0x09dd_cff3,
            Target::Table(modifiers),
        ),
        Entry::named(
            "MemberTemplateStatusInfo",
            0xb774_aecb,
            Target::Table(Table::array(start + 0x80, member_items)),
        ),
    ]);
    if matches!(kind, OPEN | TRANSPORT) {
        if children.next().is_some() {
            return Err(invalid(
                path,
                format!("unexpected {} child", node.tag_name().name()),
            ));
        }
        if let Some(mut entries) = transport {
            reference_types.extend(derived_reference_types);
            entries.insert(0, base(OPEN, Table::record(open)));
            return Ok((kind, Table::record(entries)));
        }
        return Ok((kind, Table::record(open)));
    }
    if let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, "InitialRoster"))) {
        named_leaf(child, &["TemplateId", "Count"], path)?;
        let id = match child.attribute("TemplateId") {
            Some(value) => object_id(value, path, reference_types)?,
            None => 0,
        };
        data.word(start + 0x94, id);
        data.word(
            start + 0x98,
            signed(child.attribute("Count").unwrap_or("0"), "Count", path)?,
        );
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected GarrisonContain child"));
    }
    for (name, _, offset) in GARRISON_FLAGS {
        data.payload[start + offset] = boolean(node, name, false, path)?;
    }
    let roster = Table::record(
        fields(
            start,
            &[("TemplateId", 0x148e_d75d, 0x94), ("Count", INT, 0x98)],
        )
        .collect(),
    );
    let mut entries = vec![base(OPEN, Table::record(open))];
    entries.extend(fields(start, &GARRISON_FLAGS));
    entries.push(Entry::named(
        "InitialRoster",
        0x201c_212b,
        Target::Table(roster),
    ));
    let garrison = Table::record(entries);
    Ok((
        kind,
        match slaughter {
            Some((mut horde, mut slaughter)) => {
                horde.insert(0, base(GARRISON, garrison));
                slaughter.insert(0, base(0x1d7f_c977, Table::record(horde)));
                Table::record(slaughter)
            }
            None => garrison,
        },
    ))
}

fn contain_attribute_references(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<()> {
    for attr in node.attributes() {
        let (kind, offset) = match attr.name() {
            "EnterSound" => ("BaseAudioEventInfo", 0xc),
            "ExitSound" => ("BaseAudioEventInfo", 0x10),
            "ThrowOutPassengersLandingWarhead" => ("WeaponTemplate", 0x118),
            _ => continue,
        };
        let ordinal = super::passive_area::reference(
            attr.value(),
            kind,
            data.path,
            references,
            reference_types,
        )?;
        data.word(start + offset, ordinal);
    }
    Ok(())
}

fn transport_fields(
    node: Node<'_, '_>,
    children: Vec<Node<'_, '_>>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Vec<Entry>> {
    let path = data.path;
    let mut entries = Vec::with_capacity(TRANSPORT_FIELDS.len() + 4);
    for &(name, kind, offset, default) in TRANSPORT_FIELDS {
        let field = start + offset;
        let value = node.attribute(name).unwrap_or(default);
        match kind {
            BOOL => data.payload[field] = boolean(node, name, default == "true", path)?,
            STRING => {
                if let Some(value) = node.attribute(name) {
                    data.string(field, value)?;
                }
            }
            FLOAT => data.word(field, finite_number(value, name, path)?.to_bits()),
            DURATION => data.word(field, unsigned(value, name, path)?.div_ceil(200)),
            KIND_OF => super::fields::write_kind_of(value, field, data)?,
            CONDITION => {
                if let Some(value) = node.attribute(name) {
                    let condition = if value.trim() == "INVALID" {
                        u32::MAX
                    } else {
                        super::draws::model_condition_index(value.trim(), path)? as u32
                    };
                    data.word(field, condition);
                }
            }
            WEAPON => {}
            _ => unreachable!(),
        }
        entries.push(Entry::named(name, kind, Target::Data(field)));
    }
    let mut children = children.into_iter().peekable();
    let mut payloads = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "InitialPayload")))
    {
        payloads.push(child);
    }
    let payload = start + 0x128;
    if payloads.len() > 4 {
        return Err(invalid(
            path,
            "TransportContain allows at most four InitialPayload entries",
        ));
    }
    let first = data.list(payload, payloads.len(), 8)?;
    let mut items = Vec::with_capacity(payloads.len());
    for (index, child) in payloads.into_iter().enumerate() {
        named_leaf(child, &["Name", "Count"], path)?;
        let offset = first + index * 8;
        if let Some(name) = child
            .attribute("Name")
            .filter(|name| !name.trim().is_empty())
        {
            data.word(offset, object_id(name, path, reference_types)?);
        }
        data.word(
            offset + 4,
            signed(child.attribute("Count").unwrap_or("0"), "Count", path)?,
        );
        let table =
            Table::record(fields(offset, &[("Name", 0x148e_d75d, 0), ("Count", INT, 4)]).collect());
        items.push((0, Target::Table(table)));
    }
    entries.push(Entry::named(
        "InitialPayload",
        0xb967_ae82,
        Target::Table(Table::array(payload, items)),
    ));

    let velocity =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ThrowOutPassengersVelocity")));
    let velocity = optional_vector(velocity, start + 0x130, data)?;
    entries.push(Entry::named(
        "ThrowOutPassengersVelocity",
        0x2936_8077,
        velocity,
    ));

    let mut triggers = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "UpgradeCreationTrigger")))
    {
        triggers.push(child);
    }
    let trigger = start + 0x134;
    if triggers.len() > 4 {
        return Err(invalid(
            path,
            "TransportContain allows at most four UpgradeCreationTrigger entries",
        ));
    }
    let first = data.list(trigger, triggers.len(), 12)?;
    let mut items = Vec::with_capacity(triggers.len());
    for (index, child) in triggers.into_iter().enumerate() {
        named_leaf(child, &["Upgrade", "Template", "Num"], path)?;
        let offset = first + index * 12;
        for attr in child.attributes() {
            match attr.name() {
                "Upgrade" => {
                    let ordinal = super::passive_area::reference(
                        attr.value(),
                        "UpgradeTemplate",
                        path,
                        references,
                        reference_types,
                    )?;
                    data.word(offset, ordinal);
                }
                "Template" if !attr.value().trim().is_empty() => {
                    data.word(offset + 4, object_id(attr.value(), path, reference_types)?);
                }
                _ => {}
            }
        }
        data.word(
            offset + 8,
            unsigned(child.attribute("Num").unwrap_or("0"), "Num", path)?,
        );
        let table = Table::record(
            fields(
                offset,
                &[
                    ("Upgrade", 0x4aba_1f29, 0),
                    ("Template", 0x148e_d75d, 4),
                    ("Num", UINT, 8),
                ],
            )
            .collect(),
        );
        items.push((0, Target::Table(table)));
    }
    entries.push(Entry::named(
        "UpgradeCreationTrigger",
        0x1ce0_24d5,
        Target::Table(Table::array(trigger, items)),
    ));
    let fade = if let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "FadeFilter")))
    {
        Target::Table(encode_filter(child, start + 0x13c, data, reference_types)?)
    } else {
        Target::Null
    };
    entries.push(Entry::named("FadeFilter", 0xe46e_a3b2, fade));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TransportContain child"));
    }
    Ok(entries)
}

fn unsigned(value: &str, name: &str, path: &std::path::Path) -> io::Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

fn slaughter_fields(
    node: Node<'_, '_>,
    children: &mut Vec<Node<'_, '_>>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(Vec<Entry>, Vec<Entry>)> {
    let path = data.path;
    let filter = children
        .pop()
        .filter(|child| child.has_tag_name((SAGE_NS, "CanAlwaysEnterObjectFilter")))
        .ok_or_else(|| {
            invalid(
                path,
                "SlaughterHordeContain requires CanAlwaysEnterObjectFilter",
            )
        })?;
    let ordinal = super::passive_area::reference(
        node.attribute("SlaughterFX").unwrap_or(""),
        "FXList",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 0xd0, ordinal);
    let filter = encode_inline_filter(filter, start + 0xd4, data, reference_types)?;
    data.word(
        start + 0xb0,
        percentage(
            node.attribute("CashBackPercent").unwrap_or("0"),
            "CashBackPercent",
            path,
        )?
        .to_bits(),
    );
    write_status_flags(
        node.attribute("CanAlwaysEnterStatus").unwrap_or(""),
        start + 0xb4,
        data,
    )?;
    let mut slaughter: Vec<_> = fields(
        start,
        &[
            ("CashBackPercent", FLOAT, 0xb0),
            ("CanAlwaysEnterStatus", STATUS, 0xb4),
            ("SlaughterFX", 0x0d1c_8e83, 0xd0),
        ],
    )
    .collect();
    slaughter.push(Entry::named(
        "CanAlwaysEnterObjectFilter",
        0xe46e_a3b2,
        Target::Table(filter),
    ));

    let delay = node
        .attribute("ExitDelay")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid ExitDelay"))?;
    data.word(start + 0xa0, delay);
    let mut horde = vec![Entry::named("ExitDelay", UINT, Target::Data(start + 0xa0))];
    let coords = [
        ("EntryOffset", 0xa4),
        ("EntryPosition", 0xa8),
        ("ExitOffset", 0xac),
    ];
    let mut nodes = [None; 3];
    for (index, (name, _)) in coords.into_iter().enumerate().rev() {
        if children
            .last()
            .is_some_and(|child| child.has_tag_name((SAGE_NS, name)))
        {
            nodes[index] = children.pop();
        }
    }
    for ((name, pointer), node) in coords.into_iter().zip(nodes) {
        let target = if let Some(node) = node {
            let axes = ["x", "y", "z"];
            named_leaf(node, &axes, path)?;
            let offset = data.allocate(start + pointer, 12)?;
            for (index, axis) in axes.into_iter().enumerate() {
                let value = finite_number(node.attribute(axis).unwrap_or("0"), axis, path)?;
                data.word(offset + 4 * index, value.to_bits());
            }
            Target::Data(offset)
        } else {
            Target::Null
        };
        horde.push(Entry::named(name, 0x2936_8077, target));
    }
    Ok((horde, slaughter))
}

fn passenger_data(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &["BonePrefix", "MaxPassengers", "SlingUnderBone"],
        path,
    )?;
    let mut children = significant_children(node);
    let filter = required_element(&mut children, "Filter", path)?;
    if children.next().is_some() {
        return Err(invalid(path, "unexpected PassengerData child"));
    }
    if let Some(bone) = node.attribute("BonePrefix") {
        data.string(start, bone)?;
    }
    data.word(
        start + 8,
        signed(
            node.attribute("MaxPassengers").unwrap_or("0"),
            "MaxPassengers",
            path,
        )?,
    );
    data.payload[start + 0x7c] = boolean(node, "SlingUnderBone", false, path)?;
    let filter = encode_inline_filter(filter, start + 0xc, data, reference_types)?;
    let mut entries: Vec<_> = fields(
        start,
        &[
            ("BonePrefix", 0x1a29_33aa, 0),
            ("MaxPassengers", INT, 8),
            ("SlingUnderBone", BOOL, 0x7c),
        ],
    )
    .collect();
    entries.push(Entry::named("Filter", 0xe46e_a3b2, Target::Table(filter)));
    Ok(Table::record(entries))
}

fn signed(value: &str, name: &str, path: &std::path::Path) -> io::Result<u32> {
    value
        .trim()
        .parse::<i32>()
        .map(|number| number as u32)
        .map_err(|_| invalid(path, format!("invalid {name}")))
}

fn named_leaf(node: Node<'_, '_>, attributes: &[&str], path: &std::path::Path) -> io::Result<()> {
    validate_attributes(node, attributes, path)?;
    if has_content(node) {
        return Err(invalid(
            path,
            format!("unexpected {} content", node.tag_name().name()),
        ));
    }
    Ok(())
}

fn object_id(
    value: &str,
    path: &std::path::Path,
    reference_types: &mut Vec<u32>,
) -> io::Result<u32> {
    let name = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(name, "GameObject", path)?;
    reference_types.push(id.type_id);
    Ok(id.instance_id)
}

fn disabled_flags(value: &str, path: &std::path::Path) -> io::Result<u32> {
    const NAMES: &[&str] = &[
        "DEFAULT",
        "USER_PARALYZED",
        "EMP",
        "HELD",
        "PARALYZED",
        "UNMANNED",
        "UNDERPOWERED",
        "FREEFALL",
        "TEMPORARILY_BUSY",
        "SCRIPT_DISABLED",
        "SCRIPT_UNDERPOWERED",
        "INFILTRATED",
    ];
    value.split_ascii_whitespace().try_fold(0, |flags, name| {
        let bit = super::super::global_ui::enum_value(name, NAMES, path)?;
        Ok(flags | (1 << bit))
    })
}
