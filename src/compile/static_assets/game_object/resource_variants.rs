use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, percentage, sage_int, sage_time, sage_uint,
    significant_children, validate_attributes,
};
use super::behaviors_basic::module_base;
use super::draws::boolean;
use super::parking::encode_inline_filter;
use super::passive_area::reference;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const REAL: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const INT: u32 = 0x23e6_50b8;
const UINT: u32 = 0x7631_77a4;
const BOOL: u32 = 0x77e3_f4f7;
const FILTER: u32 = 0xe46e_a3b2;

type Field = (&'static str, u32, usize, &'static str);

const TERRAIN_FIELDS: &[Field] = &[
    ("Radius", REAL, 8, "0"),
    ("MaxIncome", INT, 0xc, "0"),
    ("IncomeInterval", UINT, 0x10, "0xffffffff"),
    ("HighPriority", BOOL, 0x28, "false"),
    ("Visible", BOOL, 0x29, "true"),
];
const CRYSTAL_FIELDS: &[Field] = &[
    ("NumBoxes", UINT, 8, "0"),
    ("ValuePerBox", UINT, 0xc, "0"),
    ("GrowthStages", UINT, 0x10, "1"),
    ("TimeBetweenGrowthStages", TIME, 0x14, "5s"),
    ("GrowthTimePerStage", TIME, 0x18, "3s"),
];
const FIELD_FIELDS: &[Field] = &[
    ("DamageUpdateFrequencySeconds", TIME, 8, "0.67s"),
    ("HealUpdateFrequencySeconds", TIME, 0xc, "0.60s"),
    ("Radius", REAL, 0x10, "0"),
    ("DamageRadiusAdd", REAL, 0x14, "20"),
    ("HealRadiusAdd", REAL, 0x18, "20"),
    ("FieldResolution", INT, 0x1c, "40"),
    ("MaxFieldMoney", INT, 0x20, "0"),
    ("StartingFieldMoney", INT, 0x24, "0"),
    ("SpawnOffset", REAL, 0x2c, "0.0"),
    ("ClusterScaleFactor", REAL, 0x30, "1.0"),
    ("ClusterPowerFactor", REAL, 0x34, "1.0"),
    ("CrystalGrowthRate", INT, 0x38, "0"),
    ("LifetimeFieldMoney", UINT, 0x3c, "0"),
    ("KillWhenEmptyAndFinishedSpawning", BOOL, 0x48, "false"),
    ("AllowStartingFieldCrystalsToGrow", BOOL, 0x49, "false"),
];

pub(super) fn encode_terrain(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x5a0e_7d7b;
    let path = data.path;
    validate_fields(
        node,
        TERRAIN_FIELDS,
        &["Upgrade", "UpgradeMustBePresent", "UpgradeBonusPercent"],
        data,
    )?;
    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, "ObjectFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TerrainResource child"));
    }

    let start = data.allocate(pointer, 0x2c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    entries.extend(scalars(node, TERRAIN_FIELDS, start, data)?);
    let upgrade = reference(
        node.attribute("Upgrade").unwrap_or(""),
        "UpgradeTemplate",
        path,
        references,
        reference_types,
    )?;
    data.word(start + 0x14, upgrade);
    entries.push(Entry::named(
        "Upgrade",
        0x4aba_1f29,
        Target::Data(start + 0x14),
    ));

    let count = start + 0x18;
    let upgrades = node
        .attribute("UpgradeMustBePresent")
        .unwrap_or("")
        .split_ascii_whitespace();
    let first = data.list(count, upgrades.clone().count(), 8)?;
    let filter_relocation = data.relocations.len();
    let mut items = Vec::new();
    for (index, upgrade) in upgrades.enumerate() {
        let offset = first + index * 8;
        data.string(offset, upgrade)?;
        items.push((0, Target::Data(offset)));
    }
    entries.push(Entry::named(
        "UpgradeMustBePresent",
        0x6d66_ae18,
        Target::Table(Table::array(count, items)),
    ));
    let bonus = percentage(
        node.attribute("UpgradeBonusPercent").unwrap_or("100"),
        "UpgradeBonusPercent",
        path,
    )?;
    data.word(start + 0x20, bonus.to_bits());
    entries.push(Entry::named(
        "UpgradeBonusPercent",
        REAL,
        Target::Data(start + 0x20),
    ));
    let filter = allocate_filter(filter, start + 0x24, filter_relocation, data)?;
    entries.push(filter_entry("ObjectFilter", filter, data, reference_types)?);
    Ok((KIND, Table::record(entries)))
}

pub(super) fn encode_crystal(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x0f2d_5192;
    let path = data.path;
    validate_fields(node, CRYSTAL_FIELDS, &[], data)?;
    let mut children = significant_children(node);
    let color = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "RadarColor")))
        .ok_or_else(|| invalid(path, "TiberiumCrystal requires RadarColor"))?;
    if children.next().is_some() || has_content(color) {
        return Err(invalid(path, "unexpected TiberiumCrystal content"));
    }
    let channels = ["R", "G", "B"];
    validate_attributes(color, &channels, path)?;
    let mut packed = 0xff00_0000;
    for (channel, shift) in channels.into_iter().zip([16, 8, 0]) {
        let value = color
            .attribute(channel)
            .unwrap_or("0")
            .trim()
            .parse::<u8>()
            .map_err(|_| invalid(path, format!("invalid RadarColor {channel}")))?;
        packed |= u32::from(value) << shift;
    }

    let start = data.allocate(pointer, 0x20)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    entries.extend(scalars(node, CRYSTAL_FIELDS, start, data)?);
    data.word(start + 0x1c, packed);
    entries.push(Entry::named(
        "RadarColor",
        0x9e3e_975b,
        Target::Data(start + 0x1c),
    ));
    Ok((KIND, Table::record(entries)))
}

pub(super) fn encode_field(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const KIND: u32 = 0x6bc9_fd0e;
    let path = data.path;
    validate_fields(node, FIELD_FIELDS, &["ThingToSpawn"], data)?;
    let mut children = significant_children(node).peekable();
    let damage = children.next_if(|child| child.has_tag_name((SAGE_NS, "DamageFilter")));
    let heal = children.next_if(|child| child.has_tag_name((SAGE_NS, "HealFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected TiberiumField child"));
    }

    let start = data.allocate(pointer, 0x4c)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    let mut scalar_fields = scalars(node, FIELD_FIELDS, start, data)?;
    if let Some(value) = node
        .attribute("ThingToSpawn")
        .filter(|value| !value.trim().is_empty())
    {
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let thing = asset_reference(value, "GameObject", path)?;
        reference_types.push(thing.type_id);
        data.word(start + 0x28, thing.instance_id);
    }
    scalar_fields.insert(
        8,
        Entry::named("ThingToSpawn", 0x148e_d75d, Target::Data(start + 0x28)),
    );
    entries.extend(scalar_fields);
    let damage = allocate_filter(damage, start + 0x40, data.relocations.len(), data)?;
    let heal_relocation = data.relocations.len();
    entries.push(filter_entry("DamageFilter", damage, data, reference_types)?);
    let heal = allocate_filter(heal, start + 0x44, heal_relocation, data)?;
    entries.push(filter_entry("HealFilter", heal, data, reference_types)?);
    Ok((KIND, Table::record(entries)))
}

fn validate_fields(
    node: Node<'_, '_>,
    fields: &[Field],
    extra: &[&str],
    data: &BinaryData<'_>,
) -> io::Result<()> {
    let allowed: Vec<_> = ["id"]
        .into_iter()
        .chain(fields.iter().map(|&(name, ..)| name))
        .chain(extra.iter().copied())
        .collect();
    validate_attributes(node, &allowed, data.path)
}

fn scalars(
    node: Node<'_, '_>,
    fields: &[Field],
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Vec<Entry>> {
    let path = data.path;
    fields
        .iter()
        .map(|&(name, tag, offset, default)| {
            let offset = start + offset;
            if tag == BOOL {
                data.payload[offset] = boolean(node, name, default == "true", path)?;
            } else {
                let value = node.attribute(name).unwrap_or(default);
                let word = match tag {
                    REAL => finite_number(value, name, path)?.to_bits(),
                    TIME => sage_time(value, name, path)?.to_bits(),
                    INT => sage_int(value, name, path)?,
                    UINT => sage_uint(value, name, path)?,
                    _ => unreachable!(),
                };
                data.word(offset, word);
            }
            Ok(Entry::named(name, tag, Target::Data(offset)))
        })
        .collect()
}

fn allocate_filter<'a, 'input>(
    node: Option<Node<'a, 'input>>,
    pointer: usize,
    relocation_index: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Option<(Node<'a, 'input>, usize)>> {
    node.map(|node| {
        let start = data.allocate(pointer, 0x70)?;
        // EA records sibling relocations before their descendants, but lays out their data in order.
        data.relocations[relocation_index..].rotate_right(1);
        Ok((node, start))
    })
    .transpose()
}

fn filter_entry(
    name: &str,
    filter: Option<(Node<'_, '_>, usize)>,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Entry> {
    let target = match filter {
        Some((node, start)) => {
            Target::Table(encode_inline_filter(node, start, data, reference_types)?)
        }
        None => Target::Null,
    };
    Ok(Entry::named(name, FILTER, target))
}
