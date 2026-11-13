use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, hash_name, percentage, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::client_behaviors::audio_reference;
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x0a2b_8743;
const REAL: u32 = 0x6458_64f3;
const COST: &str = "PercentOfBuildCostToRebuildInfo";
const AUDIO: &str = "SelfAudio";
const STATES: [&str; 4] = ["Pristine", "Damaged", "ReallyDamaged", "Rubble"];
const LOOPS: [&str; 2] = ["SelfRepairFromDamageLoop", "SelfRepairFromRubbleLoop"];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "Toggleable",
            "RepairableWhenDead",
            "RepopThreshold",
            "HealAmountPerSecond",
        ],
        path,
    )?;
    let mut children = significant_children(node);
    let cost = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, COST)))
        .ok_or_else(|| invalid(path, format!("RepairUpdate requires {COST}")))?;
    let audio = children
        .next()
        .filter(|child| child.has_tag_name((SAGE_NS, AUDIO)))
        .ok_or_else(|| invalid(path, format!("RepairUpdate requires {AUDIO}")))?;
    if children.next().is_some() || has_content(cost) || has_content(audio) {
        return Err(invalid(path, "unexpected RepairUpdate child content"));
    }
    validate_attributes(cost, &STATES, path)?;
    validate_attributes(audio, &LOOPS, path)?;

    let start = data.allocate(pointer, 0x2c)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    let threshold = finite_number(
        node.attribute("RepopThreshold").unwrap_or("0.2"),
        "RepopThreshold",
        path,
    )?;
    data.word(start + 8, threshold.to_bits());
    let heal = node
        .attribute("HealAmountPerSecond")
        .unwrap_or("10")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid HealAmountPerSecond"))?;
    data.word(start + 0xc, heal);
    data.payload[start + 0x28] = boolean(node, "Toggleable", true, path)?;
    data.payload[start + 0x29] = boolean(node, "RepairableWhenDead", false, path)?;

    let mut costs = Vec::new();
    for (i, name) in STATES.into_iter().enumerate() {
        let offset = start + 0x10 + i * 4;
        let value = percentage(cost.attribute(name).unwrap_or("50"), name, path)?;
        data.word(offset, value.to_bits());
        costs.push(Entry::named(name, REAL, Target::Data(offset)));
    }
    let mut sounds = Vec::new();
    for (i, name) in LOOPS.into_iter().enumerate() {
        let offset = start + 0x20 + i * 4;
        if let Some(value) = audio.attribute(name) {
            let ordinal = audio_reference(value, path, references, reference_types)?;
            data.word(offset, ordinal);
        }
        sounds.push(Entry::named(name, 0x63d7_b468, Target::Data(offset)));
    }

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let update = inherited(
        0x2900_211d,
        inherited(0xa50d_729a, inherited(0xd75d_93a2, module)),
    );
    let mut entries = vec![base(0xf100_2505, update)];
    entries.extend(fields(
        start,
        &[
            ("Toggleable", 0x77e3_f4f7, 0x28),
            ("RepairableWhenDead", 0x77e3_f4f7, 0x29),
            ("RepopThreshold", REAL, 8),
            ("HealAmountPerSecond", 0x7631_77a4, 0xc),
        ],
    ));
    entries.extend([
        Entry::named(COST, 0xe7b2_3828, Target::Table(Table::record(costs))),
        Entry::named(AUDIO, 0x2f61_ffc1, Target::Table(Table::record(sounds))),
    ]);
    Ok((KIND, Table::record(entries)))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn repair_update_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-repair.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-repair.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-repair.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-repair.relo");
        let manifest = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-repair.manifest"
        ))
        .unwrap();
        let expected = manifest
            .assets
            .iter()
            .find(|asset| asset.id.type_id == 0x942f_ff2d)
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
        assert!(
            actual
                .references
                .iter()
                .map(|reference| reference.id)
                .eq(expected.references.iter())
        );
        assert_eq!(actual.instance_hash, expected.instance_hash);
    }
}
