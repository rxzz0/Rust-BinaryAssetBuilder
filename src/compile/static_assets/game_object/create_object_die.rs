use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, hash_name, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, inherited};
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x3b68_b768;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(node, &["id", "CreationList"], path)?;
    let mut children = significant_children(node);
    let die = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, "DieMuxData")))
        .ok_or_else(|| invalid(path, "CreateObjectDie requires DieMuxData"))?;
    let mut required_upgrades = Vec::new();
    let mut forbidden_upgrades = Vec::new();
    for child in children {
        if child.has_tag_name((SAGE_NS, "UpgradeRequired")) && forbidden_upgrades.is_empty() {
            required_upgrades.push(child);
        } else if child.has_tag_name((SAGE_NS, "UpgradeForbidden")) {
            forbidden_upgrades.push(child);
        } else {
            return Err(invalid(
                path,
                format!(
                    "unexpected CreateObjectDie child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }
    let start = data.allocate(pointer, 0x48)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    if let Some(value) = node
        .attribute("CreationList")
        .filter(|value| !value.trim().is_empty())
    {
        let ordinal = reference(
            value,
            "ObjectCreationList",
            data,
            references,
            reference_types,
        )?;
        data.word(start + 0x34, ordinal);
    }
    let upgrades = upgrade_list(
        required_upgrades,
        start + 0x38,
        data,
        references,
        reference_types,
    )?;
    let forbidden = upgrade_list(
        forbidden_upgrades,
        start + 0x40,
        data,
        references,
        reference_types,
    )?;
    let mux = encode_die_mux(die, start + 8, data)?;
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let die = Table::record(vec![
        base(0xa50d_729a, inherited(0xd75d_93a2, module)),
        Entry::named("DieMuxData", 0x0084_001b, Target::Table(mux)),
    ]);
    Ok((
        KIND,
        Table::record(vec![
            base(0x9e4f_2b72, die),
            Entry::named("CreationList", 0x42e4_e9d1, Target::Data(start + 0x34)),
            Entry::named("UpgradeRequired", 0x4aba_1f29, Target::Table(upgrades)),
            Entry::named("UpgradeForbidden", 0x4aba_1f29, Target::Table(forbidden)),
        ]),
    ))
}

fn upgrade_list(
    nodes: Vec<Node<'_, '_>>,
    count: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let start = data.list(count, nodes.len(), 4)?;
    let mut items = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.into_iter().enumerate() {
        validate_attributes(node, &[], data.path)?;
        if node.children().any(|child| child.is_element()) {
            return Err(invalid(data.path, "CreateObjectDie upgrade requires text"));
        }
        let value: String = node
            .children()
            .filter(Node::is_text)
            .filter_map(|child| child.text())
            .collect();
        let offset = start + index * 4;
        let ordinal = reference(&value, "UpgradeTemplate", data, references, reference_types)?;
        data.word(offset, ordinal);
        items.push((0, Target::Data(offset)));
    }
    Ok(Table::array(count, items))
}

fn reference(
    value: &str,
    kind: &str,
    data: &BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<u32> {
    let plain = value.split_once('\\').map_or(value, |(name, _)| name);
    let id = asset_reference(plain, kind, data.path)?;
    references.push(id.into());
    reference_types.push(id.type_id);
    Ok(references.len() as u32)
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn create_object_die_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-create-object-die.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-create-object-die.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-create-object-die.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-create-object-die.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-create-object-die.manifest"
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
