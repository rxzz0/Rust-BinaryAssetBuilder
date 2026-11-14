use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, percentage, required_element, sage_time,
    significant_children, text_value, validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, module_base};
use super::draws::{boolean, fields};
use super::parking::{encode_filter, encode_inline_filter};
use super::secondary_damage::{status_index, write_status_flags};
use super::spawn::id_list;
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

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

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let slaughter = node.has_tag_name((SAGE_NS, "SlaughterHordeContain"));
    let kind = if slaughter { SLAUGHTER } else { GARRISON };
    let mut allowed: Vec<_> = std::iter::once("id")
        .chain(OPEN_FIELDS.iter().map(|&(name, _, _, _)| name))
        .chain(GARRISON_FLAGS.map(|(name, _, _)| name))
        .collect();
    if slaughter {
        allowed.extend([
            "ExitDelay",
            "CashBackPercent",
            "CanAlwaysEnterStatus",
            "SlaughterFX",
        ]);
    }
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, if slaughter { 0x144 } else { 0xa0 })?;
    let mut children: Vec<_> = significant_children(node).collect();
    let derived = if slaughter {
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
    let mut open = vec![module_base(node, kind, start, data, true)?];
    for &(name, kind, offset, default) in OPEN_FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        let field = start + offset;
        match kind {
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
    let mut modifiers = Vec::new();
    while let Some(child) =
        children.next_if(|child| child.has_tag_name((SAGE_NS, "ModifierToGiveOnExit")))
    {
        modifiers.push(super::passive_area::reference(
            text_value(child, path)?,
            "AttributeModifier",
            path,
            references,
            reference_types,
        )?);
    }
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
    let mut entries = vec![base(0xd7cc_2e35, Table::record(open))];
    entries.extend(fields(start, &GARRISON_FLAGS));
    entries.push(Entry::named(
        "InitialRoster",
        0x201c_212b,
        Target::Table(roster),
    ));
    let garrison = Table::record(entries);
    Ok((
        kind,
        match derived {
            Some((mut horde, mut slaughter)) => {
                horde.insert(0, base(GARRISON, garrison));
                slaughter.insert(0, base(0x1d7f_c977, Table::record(horde)));
                Table::record(slaughter)
            }
            None => garrison,
        },
    ))
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
