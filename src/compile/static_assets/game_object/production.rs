use super::super::binary::BinaryData;
use super::super::global_ui::object_filter;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x6ec7_5439;
const UINT: u32 = 0x7631_77a4;
const INT: u32 = 0x23e6_50b8;
const TIME: u32 = 0x9986_69e8;
const BOOL: u32 = 0x77e3_f4f7;
const REAL: u32 = 0x6458_64f3;
const UPGRADE: u32 = 0x4aba_1f29;
const GAME_OBJECT: u32 = 0x148e_d75d;
const FIELDS: &[(&str, u32, usize)] = &[
    ("MaxQueueEntries", UINT, 8),
    ("NumDoorAnimations", INT, 0xc),
    ("DoorOpeningTime", TIME, 0x10),
    ("DoorWaitOpenTime", TIME, 0x14),
    ("DoorCloseTime", TIME, 0x18),
    ("ConstructionCompleteDuration", UINT, 0x1c),
    ("DisabledTypesToProcess", 0x1965_70c9, 0x20),
    ("GiveNoXP", BOOL, 0x50),
    ("UnitInvulnerableTime", UINT, 0x24),
    ("SpecialPrepModelconditionTime", UINT, 0x28),
    ("VeteranUnitsFromVeteranFactory", BOOL, 0x51),
    ("BonusForType", GAME_OBJECT, 0x2c),
    ("SecondaryQueue", BOOL, 0x52),
    ("Type", 0xbc88_486a, 0x30),
    ("IgnorePreReqs", BOOL, 0x53),
    ("ProductionTimeDelayScalar", TIME, 0x34),
    ("NewObjectFadeInTime", TIME, 0x38),
];
const QUEUES: &[&str] = &[
    "MAIN_STRUCTURE",
    "OTHER_STRUCTURE",
    "INFANTRY",
    "VEHICLE",
    "AIRCRAFT",
    "UPGRADE",
    "WATERCRAFT",
];
const DISABLED: &[&str] = &[
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

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut quantities = Vec::new();
    let mut modifiers = Vec::new();
    let mut upgrades = None;
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "QuantityModifier")) && quantities.len() < 2 {
            quantities.push(child);
        } else if child.has_tag_name((SAGE_NS, "ProductionModifier")) && modifiers.len() < 2 {
            modifiers.push(child);
        } else if child.has_tag_name((SAGE_NS, "GiveUpgradeToProduced")) && upgrades.is_none() {
            upgrades = Some(child);
        } else {
            return Err(invalid(
                path,
                format!(
                    "unexpected ProductionUpdate child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }
    let start = data.allocate(pointer, 0x54)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, tag, offset) in FIELDS {
        let value = node.attribute(name);
        let word = match tag {
            BOOL => {
                data.payload[start + offset] = boolean(node, name, false, path)?;
                continue;
            }
            UINT => value
                .unwrap_or(if name == "MaxQueueEntries" { "99" } else { "0" })
                .trim()
                .parse::<u32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))?,
            INT => signed(value.unwrap_or("0"), name, data)?,
            TIME => sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
            GAME_OBJECT => weak_object(value, data)?,
            _ if name == "Type" => {
                let value = value.unwrap_or("INVALID");
                if value == "INVALID" {
                    u32::MAX
                } else {
                    enum_index(value, QUEUES, name, data)?
                }
            }
            _ => {
                let mut flags = 0;
                for flag in value.unwrap_or("").split_ascii_whitespace() {
                    flags |= 1 << enum_index(flag, DISABLED, name, data)?;
                }
                flags
            }
        };
        data.word(start + offset, word);
    }
    if node
        .attribute("BonusForType")
        .is_some_and(|value| !value.trim().is_empty())
    {
        reference_types.push(super::super::fast_hash(b"GameObject"));
    }

    let mut encoder = ProductionEncoder {
        data,
        references,
        reference_types,
    };
    let quantity_table = encoder.quantities(quantities, start + 0x3c)?;
    let modifier_table = encoder.modifiers(modifiers, start + 0x44)?;
    let upgrades = match upgrades {
        Some(node) => Target::Table(encoder.upgrades(node, start + 0x4c)?),
        None => Target::Null,
    };
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, INT, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let update = inherited(
        0x2900_211d,
        inherited(0xa50d_729a, inherited(0xd75d_93a2, module)),
    );
    let mut entries = vec![base(0xf100_2505, update)];
    entries.extend(fields(start, FIELDS));
    entries.extend([
        Entry::named(
            "QuantityModifier",
            0x249d_fb83,
            Target::Table(quantity_table),
        ),
        Entry::named(
            "ProductionModifier",
            0xef83_869a,
            Target::Table(modifier_table),
        ),
        Entry::named("GiveUpgradeToProduced", 0xb290_db31, upgrades),
    ]);
    Ok((KIND, Table::record(entries)))
}

struct ProductionEncoder<'a, 'path> {
    data: &'a mut BinaryData<'path>,
    references: &'a mut Vec<AssetReference>,
    reference_types: &'a mut Vec<u32>,
}

impl ProductionEncoder<'_, '_> {
    fn quantities(&mut self, nodes: Vec<Node<'_, '_>>, count: usize) -> io::Result<Table> {
        let start = self.data.list(count, nodes.len(), 8)?;
        let mut items = Vec::with_capacity(nodes.len());
        for (index, node) in nodes.into_iter().enumerate() {
            leaf(node, &["Name", "Count"], self.data)?;
            let offset = start + index * 8;
            let id = weak_object(node.attribute("Name"), self.data)?;
            if node
                .attribute("Name")
                .is_some_and(|value| !value.trim().is_empty())
            {
                self.reference_types
                    .push(super::super::fast_hash(b"GameObject"));
            }
            self.data.word(offset, id);
            self.data.word(
                offset + 4,
                signed(node.attribute("Count").unwrap_or("0"), "Count", self.data)?,
            );
            items.push((
                0,
                Target::Table(Table::record(
                    fields(offset, &[("Name", GAME_OBJECT, 0), ("Count", INT, 4)]).collect(),
                )),
            ));
        }
        Ok(Table::array(count, items))
    }

    fn modifiers(&mut self, nodes: Vec<Node<'_, '_>>, count: usize) -> io::Result<Table> {
        let start = self.data.list(count, nodes.len(), 0x7c)?;
        let mut items = Vec::with_capacity(nodes.len());
        let path = self.data.path;
        for (index, node) in nodes.into_iter().enumerate() {
            validate_attributes(
                node,
                &["RequiredUpgrade", "CostMultiplier", "TimeMultiplier"],
                path,
            )?;
            let mut children = significant_children(node);
            let filter = children
                .next()
                .filter(|child| child.has_tag_name((SAGE_NS, "ModifierFilter")))
                .ok_or_else(|| invalid(path, "ProductionModifier requires ModifierFilter"))?;
            if children.next().is_some() {
                return Err(invalid(path, "unexpected ProductionModifier child"));
            }
            let offset = start + index * 0x7c;
            if let Some(value) = node.attribute("RequiredUpgrade") {
                self.upgrade(value, offset)?;
            }
            for (name, field) in [("CostMultiplier", 4), ("TimeMultiplier", 8)] {
                let value = finite_number(node.attribute(name).unwrap_or("1"), name, path)?;
                self.data.word(offset + field, value.to_bits());
            }
            let filter_start = offset + 12;
            object_filter(filter, filter_start, self.data)?;
            let lists = [0x60, 0x68].map(|field| {
                let word = |offset| {
                    u32::from_le_bytes(self.data.payload[offset..offset + 4].try_into().unwrap())
                        as usize
                };
                let count = word(filter_start + field);
                let first = word(filter_start + field + 4);
                (0..count)
                    .map(|i| (0, Target::Data(first + i * 4)))
                    .collect()
            });
            for child in significant_children(filter) {
                if child.text().is_some_and(|value| !value.trim().is_empty()) {
                    self.reference_types
                        .push(super::super::fast_hash(b"GameObject"));
                }
            }
            let mut entries: Vec<_> = fields(
                offset,
                &[
                    ("RequiredUpgrade", UPGRADE, 0),
                    ("CostMultiplier", REAL, 4),
                    ("TimeMultiplier", REAL, 8),
                ],
            )
            .collect();
            entries.push(Entry::named(
                "ModifierFilter",
                0xe46e_a3b2,
                Target::Table(super::recall::filter_table(filter_start, lists)),
            ));
            items.push((0, Target::Table(Table::record(entries))));
        }
        Ok(Table::array(count, items))
    }

    fn upgrades(&mut self, node: Node<'_, '_>, pointer: usize) -> io::Result<Table> {
        let path = self.data.path;
        validate_attributes(node, &[], path)?;
        let translations: Vec<_> = significant_children(node).collect();
        if translations.is_empty()
            || translations
                .iter()
                .any(|node| !node.has_tag_name((SAGE_NS, "UpgradeTranslate")))
        {
            return Err(invalid(
                path,
                "GiveUpgradeToProduced requires UpgradeTranslate children",
            ));
        }
        let count = self.data.allocate(pointer, 8)?;
        let start = self.data.list(count, translations.len(), 8)?;
        let mut items = Vec::with_capacity(translations.len());
        for (index, node) in translations.into_iter().enumerate() {
            leaf(node, &["RequiredUpgrade", "ProvidedUpgrade"], self.data)?;
            let offset = start + index * 8;
            for attr in node.attributes() {
                match attr.name() {
                    "RequiredUpgrade" => self.upgrade(attr.value(), offset)?,
                    "ProvidedUpgrade" => self.upgrade(attr.value(), offset + 4)?,
                    _ => {}
                }
            }
            items.push((
                0,
                Target::Table(Table::record(
                    fields(
                        offset,
                        &[
                            ("RequiredUpgrade", UPGRADE, 0),
                            ("ProvidedUpgrade", UPGRADE, 4),
                        ],
                    )
                    .collect(),
                )),
            ));
        }
        Ok(Table::record(vec![Entry::named(
            "UpgradeTranslate",
            0xb38a_b380,
            Target::Table(Table::array(count, items)),
        )]))
    }

    fn upgrade(&mut self, value: &str, offset: usize) -> io::Result<()> {
        if !value.trim().is_empty() {
            let value = value.split_once('\\').map_or(value, |(name, _)| name);
            let id = asset_reference(value, "UpgradeTemplate", self.data.path)?;
            self.references.push(id.into());
            self.reference_types.push(id.type_id);
            self.data.word(offset, self.references.len() as u32);
        }
        Ok(())
    }
}

fn weak_object(value: Option<&str>, data: &BinaryData<'_>) -> io::Result<u32> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| asset_reference(value, "GameObject", data.path).map(|id| id.instance_id))
        .unwrap_or(Ok(0))
}

fn signed(value: &str, name: &str, data: &BinaryData<'_>) -> io::Result<u32> {
    value
        .trim()
        .parse::<i32>()
        .map(|value| value as u32)
        .map_err(|_| invalid(data.path, format!("invalid {name}")))
}

fn enum_index(value: &str, values: &[&str], name: &str, data: &BinaryData<'_>) -> io::Result<u32> {
    values
        .iter()
        .position(|&item| item == value)
        .map(|index| index as u32)
        .ok_or_else(|| invalid(data.path, format!("invalid {name} {value}")))
}

fn leaf(node: Node<'_, '_>, allowed: &[&str], data: &BinaryData<'_>) -> io::Result<()> {
    validate_attributes(node, allowed, data.path)?;
    if has_content(node) {
        return Err(invalid(
            data.path,
            format!("{} cannot contain content", node.tag_name().name()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn production_update_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-production-update.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-production-update.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-production-update.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-production-update.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-production-update.manifest"
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
}
