use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, hash_name, significant_children, validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::boolean;
use super::parking::encode_filter;
use super::secondary_damage::write_status_flags;
use crate::compile::{SAGE_NS, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x0ff7_60e0;
const VOICES: [&str; 2] = [
    "NameOfVoiceToUseForFriendlyEnter",
    "NameOfVoiceToUseForHostileEnter",
];
const FX: [&str; 4] = [
    "FXForRepair",
    "FXForCapture",
    "FXForReplace",
    "FXForCaptureAndReplace",
];
const EVA: [&str; 4] = [
    "EvaEventForRepair",
    "EvaEventForCapture",
    "EvaEventForReplace",
    "EvaEventForCaptureAndReplace",
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = [
        "id",
        "ImmediatelyEnabled",
        "ReplaceWith",
        "ObjectStatusToSet",
    ]
    .into_iter()
    .chain(VOICES)
    .chain(FX)
    .chain(EVA)
    .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|child| child.has_tag_name((SAGE_NS, "CanEnterFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected EngineerContain child"));
    }

    let start = data.allocate(pointer, 0x58)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    write_status_flags(
        node.attribute("ObjectStatusToSet").unwrap_or("NO_REFUND"),
        start + 0x14,
        data,
    )?;
    data.payload[start + 0x54] = boolean(node, "ImmediatelyEnabled", false, path)?;

    // EA assigns reference ordinals in XML attribute order, including repeated targets.
    let mut fx_refs = [None; 4];
    for attribute in node.attributes() {
        let value = attribute.value().trim();
        if value.is_empty() {
            continue;
        }
        let name = attribute.name();
        let (kind, offset) = if name == "ReplaceWith" {
            ("ObjectCreationList", Some(start + 8))
        } else if let Some(i) = EVA.iter().position(|&field| field == name) {
            ("EvaEvent", Some(start + 0x40 + i * 4))
        } else if FX.contains(&name) {
            ("FXList", None)
        } else {
            continue;
        };
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, kind, path)?;
        references.push(id.into());
        reference_types.push(id.type_id);
        let ordinal = references.len() as u32;
        if let Some(offset) = offset {
            data.word(offset, ordinal);
        } else if let Some(i) = FX.iter().position(|&field| field == name) {
            fx_refs[i] = Some(ordinal);
        }
    }

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut entries = vec![
        base(
            0x2900_211d,
            inherited(0xa50d_729a, inherited(0xd75d_93a2, module)),
        ),
        Entry::named(
            "ImmediatelyEnabled",
            0x77e3_f4f7,
            Target::Data(start + 0x54),
        ),
        Entry::named("ReplaceWith", 0x42e4_e9d1, Target::Data(start + 8)),
    ];
    for (i, name) in VOICES.into_iter().enumerate() {
        let target = if let Some(value) = node.attribute(name) {
            let offset = data.allocate(start + 0xc + i * 4, 4)?;
            data.word(offset, super::super::fast_hash(value.as_bytes()));
            Target::Data(offset)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, 0x58be_c727, target));
    }
    entries.push(Entry::named(
        "ObjectStatusToSet",
        0xa7bd_811e,
        Target::Data(start + 0x14),
    ));
    for (i, (name, ordinal)) in FX.into_iter().zip(fx_refs).enumerate() {
        let target = if let Some(ordinal) = ordinal {
            let offset = data.allocate(start + 0x30 + i * 4, 4)?;
            data.word(offset, ordinal);
            Target::Data(offset)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, 0x0d1c_8e83, target));
    }
    for (i, name) in EVA.into_iter().enumerate() {
        entries.push(Entry::named(
            name,
            0xc4ec_43b1,
            Target::Data(start + 0x40 + i * 4),
        ));
    }
    let target = match filter {
        Some(node) => Target::Table(encode_filter(node, start + 0x50, data, reference_types)?),
        None => Target::Null,
    };
    entries.push(Entry::named("CanEnterFilter", 0xe46e_a3b2, target));
    Ok((KIND, Table::record(entries)))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn engineer_contain_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-engineer.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-engineer.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-engineer.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-engineer.relo");
        let manifest = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-engineer.manifest"
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
