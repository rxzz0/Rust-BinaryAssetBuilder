use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, inherited};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xb071_19e4;
const REAL: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;
const EVENT_SIZE: usize = 0x18;
const EVENT_FIELDS: &[(&str, u32, usize)] = &[
    ("Index", 0x03af_8d2f, 0),
    ("FX", 0x0d1c_8e83, 4),
    ("MinThreshold", REAL, 8),
    ("MaxThreshold", REAL, 0xc),
    ("OrientToObject", BOOL, 0x14),
    ("ForceUseDoFXObj", BOOL, 0x15),
    ("Direction", 0x5e15_1008, 0x10),
];
const INDICES: &[&str] = &[
    "onDeath",
    "onDamaged",
    "onCreate",
    "onTransitionToDamaged",
    "onTransitionToReallyDamaged",
    "onTransitionToRubble",
    "onBecomingTemporarySlave",
    "onBecomingTemporaryOwner",
    "onEndingTemporarySlave",
    "onEndingTemporaryOwner",
    "onTerrainCollision",
    "onShieldSphereCollision",
    "onSurfacing",
    "onSubmerging",
];
const DIRECTIONS: &[&str] = &["Front", "Rear", "Side", "Top", "Default"];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id"], path)?;
    let mut children = significant_children(node);
    let die = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(path, "FXListBehavior requires DieMuxData"))?;
    let events: Vec<_> = children.collect();
    if events.is_empty()
        || events
            .iter()
            .any(|node| !node.has_tag_name((SAGE_NS, "Event")))
    {
        return Err(invalid(
            path,
            "FXListBehavior requires Event children after DieMuxData",
        ));
    }
    let start = data.allocate(pointer, 0x3c)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    let die = encode_die_mux(die, start + 8, data)?;
    let count = start + 0x34;
    let first = data.list(count, events.len(), EVENT_SIZE)?;
    let mut items = Vec::with_capacity(events.len());
    for (index, event) in events.into_iter().enumerate() {
        let offset = first + index * EVENT_SIZE;
        let table = encode_event(event, offset, data, references, reference_types)?;
        items.push((0, Target::Table(table)));
    }
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let die = Table::record(vec![
        base(0xa50d_729a, inherited(0xd75d_93a2, module)),
        Entry::named("DieMuxData", 0x0084_001b, Target::Table(die)),
    ]);
    Ok((
        KIND,
        Table::record(vec![
            base(0x9e4f_2b72, die),
            Entry::named(
                "Event",
                0x65be_8011,
                Target::Table(Table::array(count, items)),
            ),
        ]),
    ))
}

fn encode_event(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    let allowed: Vec<_> = EVENT_FIELDS.iter().map(|&(name, _, _)| name).collect();
    validate_attributes(node, &allowed, path)?;
    if has_content(node) {
        return Err(invalid(path, "FXListBehavior Event cannot contain content"));
    }
    for (name, values, offset) in [("Index", INDICES, 0), ("Direction", DIRECTIONS, 0x10)] {
        let value = node.attribute(name).unwrap_or(if name == "Index" {
            "onDeath"
        } else {
            "Default"
        });
        let index = values
            .iter()
            .position(|&candidate| candidate == value)
            .ok_or_else(|| invalid(path, format!("invalid FXListBehavior Event {name} {value}")))?;
        data.word(start + offset, index as u32);
    }
    if let Some(value) = node
        .attribute("FX")
        .filter(|value| !value.trim().is_empty())
    {
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, "FXList", path)?;
        references.push(id.into());
        reference_types.push(id.type_id);
        data.word(start + 4, references.len() as u32);
    }
    for (name, offset) in [("MinThreshold", 8), ("MaxThreshold", 0xc)] {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    data.payload[start + 0x14] = boolean(node, "OrientToObject", true, path)?;
    data.payload[start + 0x15] = boolean(node, "ForceUseDoFXObj", false, path)?;
    Ok(Table::record(fields(start, EVENT_FIELDS).collect()))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn fx_list_behavior_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-fx-list.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-fx-list.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-fx-list.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-fx-list.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-fx-list.manifest"
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
