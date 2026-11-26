use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, required_element, sage_time, significant_children, text_value,
    validate_attributes,
};
use super::ai_update::locomotor_condition;
use super::draws::{boolean, fields};
use super::garrison::{named_leaf, object_id, signed};
use super::passive_area::reference;
use super::secondary_damage::write_status_flags;
use super::spawn::{id_list, weak_reference};
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::io;
use std::iter::Peekable;

pub(super) const KIND: u32 = 0x4758_fed0;
pub(super) const SIZE: usize = 0x1f4;

const FORMATION: u32 = 0xf00f_b0cf;
const OBJECT: u32 = 0x148e_d75d;
const LOCOMOTOR: u32 = 0x845d_12b2;
const BOOL: u32 = 0x77e3_f4f7;
const FLOAT: u32 = 0x6458_64f3;
const EVA: u32 = 0xc4ec_43b1;
const UINT: u32 = 0x7631_77a4;
const INT: u32 = 0x23e6_50b8;
const TIME: u32 = 0x9986_69e8;
const STATUS: u32 = 0xa7bd_811e;
const VECTOR: u32 = 0x4ae3_1f24;

pub(super) const FIELDS: &[(&str, u32, usize, &str)] = &[
    ("Formation", FORMATION, 0x14c, "MAIN"),
    ("AlternateFormation", OBJECT, 0x150, ""),
    ("ForcedLocomotorSet", LOCOMOTOR, 0x154, "NORMAL"),
    ("UseSlowHordeMovement", BOOL, 0x1f0, "true"),
    ("MeleeAttackLeashDistance", FLOAT, 0x158, "60"),
    ("GeometryFrontAngleRadians", FLOAT, 0x15c, "0"),
    ("EvaEventLastMemberDeath", EVA, 0x160, ""),
    ("FrontAngle", FLOAT, 0x164, "6.283"),
    ("FlankedDelaySeconds", FLOAT, 0x168, "0"),
    ("FlankedDurationSeconds", FLOAT, 0x16c, "5"),
    ("MinimumHordeSize", UINT, 0x170, "0"),
    ("BackupMinDelayTime", TIME, 0x174, "0s"),
    ("BackupMaxDelayTime", TIME, 0x178, "0s"),
    ("BackupMinDistance", FLOAT, 0x17c, "0"),
    ("BackupMaxDistance", FLOAT, 0x180, "0"),
    ("BackupPercentage", FLOAT, 0x184, "0"),
    ("RadiusCowerOverride", FLOAT, 0x188, "0"),
    ("VisionOverrideRear", FLOAT, 0x18c, "0"),
    ("VisionOverrideSide", FLOAT, 0x190, "0"),
    ("SpawnBannerCarrierImmediately", BOOL, 0x1f1, "false"),
    ("BannerCarrierByUpgradeOnly", BOOL, 0x1f2, "false"),
    ("ForbiddenCoverStatus", STATUS, 0x194, ""),
];

const CHILDREN: &[(&str, u32, usize)] = &[
    ("RandomOffset", VECTOR, 0x1b0),
    ("RankInfo", 0x3dc1_e0f9, 0x1b4),
    ("RankThatStopsAdvance", INT, 0x1bc),
    ("RankToReleaseWhenAttacking", INT, 0x1c4),
    ("LeaderPosition", VECTOR, 0x1cc),
    ("BannerCarrierPosition", 0x9399_63df, 0x1d0),
    ("BannerCarriersAllowed", OBJECT, 0x1d8),
    ("LeadersAllowed", OBJECT, 0x1e0),
    ("AttributeModifier", 0x09dd_cff3, 0x1e8),
];

pub(super) fn is_child(node: &Node<'_, '_>) -> bool {
    CHILDREN
        .iter()
        .any(|&(name, ..)| node.has_tag_name((SAGE_NS, name)))
}

pub(super) struct HordeFields<'a> {
    entries: Vec<Entry>,
    reference_types: Vec<u32>,
    modifiers: Vec<&'a str>,
    modifier_start: usize,
}

impl HordeFields<'_> {
    pub(super) fn finish(
        self,
        inherited: Entry,
        data: &mut BinaryData<'_>,
        references: &mut Vec<AssetReference>,
        reference_types: &mut Vec<u32>,
    ) -> io::Result<Table> {
        reference_types.extend(self.reference_types);
        for (index, value) in self.modifiers.into_iter().enumerate() {
            let ordinal = reference(
                value,
                "AttributeModifier",
                data.path,
                references,
                reference_types,
            )?;
            data.word(self.modifier_start + index * 4, ordinal);
        }
        let mut entries = self.entries;
        entries.insert(0, inherited);
        Ok(Table::record(entries))
    }
}

pub(super) fn encode<'a>(
    node: Node<'a, '_>,
    children: Vec<Node<'a, '_>>,
    start: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<HordeFields<'a>> {
    let path = data.path;
    let mut entries = Vec::with_capacity(FIELDS.len() + CHILDREN.len() + 1);
    for &(name, tag, offset, default) in FIELDS {
        let value = node.attribute(name).unwrap_or(default);
        let field = start + offset;
        match tag {
            OBJECT | EVA => {}
            FORMATION if value.trim() == "MAIN" => {}
            FORMATION => return Err(invalid(path, "invalid HordeContain Formation")),
            LOCOMOTOR => data.word(field, locomotor_condition(value.trim(), name, path)?),
            BOOL => data.payload[field] = boolean(node, name, default == "true", path)?,
            FLOAT => data.word(field, finite_number(value, name, path)?.to_bits()),
            UINT => data.word(
                field,
                value
                    .trim()
                    .parse()
                    .map_err(|_| invalid(path, format!("invalid {name}")))?,
            ),
            TIME => data.word(field, sage_time(value, name, path)?.to_bits()),
            STATUS => write_status_flags(value, field, data)?,
            _ => unreachable!("HordeContain field type"),
        }
        entries.push(Entry::named(name, tag, Target::Data(field)));
    }

    let mut children = children.into_iter().peekable();
    let mut reference_types = Vec::new();
    let mut modifiers = Vec::new();
    let mut modifier_start = 0;
    for &(name, tag, offset) in CHILDREN {
        let field = start + offset;
        let target = match name {
            "RandomOffset" | "LeaderPosition" => {
                match children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
                    Some(child) => {
                        let vector = data.allocate(field, 8)?;
                        vector2(child, vector, data)?;
                        Target::Data(vector)
                    }
                    None => Target::Null,
                }
            }
            "RankInfo" => {
                let ranks = take_children(&mut children, name);
                Target::Table(rank_list(ranks, field, data, &mut reference_types)?)
            }
            "RankThatStopsAdvance" | "RankToReleaseWhenAttacking" => {
                let values = take_children(&mut children, name)
                    .into_iter()
                    .map(|child| signed(text_value(child, path)?, name, path))
                    .collect::<io::Result<Vec<_>>>()?;
                Target::Table(id_list(data, field, values)?)
            }
            "BannerCarrierPosition" => {
                let banners = take_children(&mut children, name);
                Target::Table(banner_list(banners, field, data, &mut reference_types)?)
            }
            "BannerCarriersAllowed" | "LeadersAllowed" => {
                let values = take_children(&mut children, name)
                    .into_iter()
                    .map(|child| weak_reference(child, "GameObject", data, &mut reference_types))
                    .collect::<io::Result<Vec<_>>>()?;
                Target::Table(id_list(data, field, values)?)
            }
            "AttributeModifier" => {
                modifiers = take_children(&mut children, name)
                    .into_iter()
                    .map(|child| text_value(child, path))
                    .collect::<io::Result<_>>()?;
                // EA allocates derived lists first, then resolves references in XML order.
                modifier_start = data.list(field, modifiers.len(), 4)?;
                let items: Vec<_> = (0..modifiers.len())
                    .map(|index| (0, Target::Data(modifier_start + index * 4)))
                    .collect();
                Target::Table(Table::array(field, items))
            }
            _ => unreachable!("HordeContain child type"),
        };
        entries.push(Entry::named(name, tag, target));
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected HordeContain child"));
    }
    Ok(HordeFields {
        entries,
        reference_types,
        modifiers,
        modifier_start,
    })
}

fn take_children<'a, 'input>(
    children: &mut Peekable<impl Iterator<Item = Node<'a, 'input>>>,
    name: &str,
) -> Vec<Node<'a, 'input>> {
    std::iter::from_fn(|| children.next_if(|child| child.has_tag_name((SAGE_NS, name)))).collect()
}

fn rank_list(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let first = data.list(count, nodes.len(), 0x10)?;
    let mut items = Vec::with_capacity(nodes.len());
    let path = data.path;
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &["RankID", "UnitType"], path)?;
        let rank = first + index * 0x10;
        data.word(
            rank,
            signed(node.attribute("RankID").unwrap_or("0"), "RankID", path)?,
        );
        if let Some(value) = node
            .attribute("UnitType")
            .filter(|value| !value.trim().is_empty())
        {
            data.word(rank + 4, object_id(value, path, reference_types)?);
        }
        let positions: Vec<_> = significant_children(node).collect();
        let position_count = rank + 8;
        let first_position = data.list(position_count, positions.len(), 0x10)?;
        let mut position_items = Vec::with_capacity(positions.len());
        for (index, node) in positions.into_iter().enumerate() {
            if !node.has_tag_name((SAGE_NS, "Position")) {
                return Err(invalid(path, "unexpected HordeContain RankInfo child"));
            }
            let position = first_position + index * 0x10;
            position_items.push((0, Target::Table(rank_position(node, position, data)?)));
        }
        let mut entries: Vec<_> =
            fields(rank, &[("RankID", INT, 0), ("UnitType", OBJECT, 4)]).collect();
        entries.push(Entry::named(
            "Position",
            0x6103_a9a6,
            Target::Table(Table::array(position_count, position_items)),
        ));
        items.push((0, Target::Table(Table::record(entries))));
    }
    Ok(Table::array(count, items))
}

fn rank_position(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<Table> {
    let path = data.path;
    named_leaf(node, &["X", "Y", "LeaderRank", "LeaderIndex"], path)?;
    for (name, tag, offset, default) in [
        ("X", FLOAT, 0, "0"),
        ("Y", FLOAT, 4, "0"),
        ("LeaderRank", INT, 8, "-1"),
        ("LeaderIndex", INT, 12, "0"),
    ] {
        let value = node.attribute(name).unwrap_or(default);
        let word = if tag == FLOAT {
            finite_number(value, name, path)?.to_bits()
        } else {
            signed(value, name, path)?
        };
        data.word(start + offset, word);
    }
    Ok(Table::record(
        fields(
            start,
            &[
                ("X", FLOAT, 0),
                ("Y", FLOAT, 4),
                ("LeaderRank", INT, 8),
                ("LeaderIndex", INT, 12),
            ],
        )
        .collect(),
    ))
}

fn banner_list(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let first = data.list(count, nodes.len(), 12)?;
    let mut items = Vec::with_capacity(nodes.len());
    let path = data.path;
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &[], path)?;
        let mut children = significant_children(node);
        let unit = required_element(&mut children, "UnitType", path)?;
        let pos = required_element(&mut children, "Pos", path)?;
        if children.next().is_some() {
            return Err(invalid(path, "unexpected BannerCarrierPosition child"));
        }
        let item = first + index * 12;
        let unit_type = weak_reference(unit, "GameObject", data, reference_types)?;
        data.word(item, unit_type);
        vector2(pos, item + 4, data)?;
        let entries = fields(item, &[("UnitType", OBJECT, 0), ("Pos", VECTOR, 4)]).collect();
        items.push((0, Target::Table(Table::record(entries))));
    }
    Ok(Table::array(count, items))
}

fn vector2(node: Node<'_, '_>, start: usize, data: &mut BinaryData<'_>) -> io::Result<()> {
    let path = data.path;
    named_leaf(node, &["x", "y"], path)?;
    for (index, name) in ["x", "y"].into_iter().enumerate() {
        let value = node.attribute(name).unwrap_or("0");
        data.word(
            start + index * 4,
            finite_number(value, name, path)?.to_bits(),
        );
    }
    Ok(())
}
