use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, fast_hash, finite_number, hash_name, sage_time,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::boolean;
use super::secondary_damage::write_status_flags;
use crate::compile::{has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x5047_272e;
const TIME: u32 = 0x9986_69e8;
const STATUS: u32 = 0xa7bd_811e;
const FILTER: u32 = 0x1e59_2b76;
const VOICE: u32 = 0x58be_c727;
const EVA: u32 = 0xc4ec_43b1;
const FIELDS: &[(&str, u32, usize)] = &[
    ("BlockedDuration", TIME, 8),
    ("InfiltratedStatusFilter", STATUS, 0xc),
    ("CanEnterFilter", FILTER, 0x28),
    ("ImmediatelyEnabled", 0x77e3_f4f7, 0x7c),
    ("ReplaceWith", 0x42e4_e9d1, 0x2c),
    ("NameOfVoiceToUseForHostileEnter", VOICE, 0x30),
    ("NameOfVoiceToUseForInfiltrationDeath", VOICE, 0x34),
    ("EvaEventForInfiltratingPlayer", EVA, 0x38),
    ("EvaEventForInfiltratedPlayer", EVA, 0x3c),
    ("FXForInfiltrate", 0x0d1c_8e83, 0x40),
    ("Duration", TIME, 0x44),
    ("UnitDuration", TIME, 0x48),
    ("Amount", 0x6458_64f3, 0x4c),
    ("Effect", 0xd0ce_7f56, 0x50),
    ("ObjectRef", 0x148e_d75d, 0x54),
    ("UnitFilter", FILTER, 0x58),
    ("StructureFilter", FILTER, 0x5c),
    ("ObjectStatusToSet", STATUS, 0x60),
];
const REFERENCES: &[(&str, &str, &str, usize)] = &[
    (
        "CanEnterFilter",
        "ObjectFilterAsset",
        "InfiltrationCanEnterObjectFilter",
        0x28,
    ),
    ("ReplaceWith", "ObjectCreationList", "", 0x2c),
    (
        "EvaEventForInfiltratingPlayer",
        "EvaEvent",
        "EnemyBuildingInfiltrated",
        0x38,
    ),
    (
        "EvaEventForInfiltratedPlayer",
        "EvaEvent",
        "OurBuildingInfiltrated",
        0x3c,
    ),
    (
        "FXForInfiltrate",
        "FXList",
        "FX_Building_Infiltrated_Generic",
        0x40,
    ),
    ("ObjectRef", "GameObject", "", 0x54),
    ("UnitFilter", "ObjectFilterAsset", "", 0x58),
    ("StructureFilter", "ObjectFilterAsset", "", 0x5c),
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
    if has_content(node) {
        return Err(invalid(path, "unexpected InfiltratorContain child content"));
    }

    let start = data.allocate(pointer, 0x80)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    data.payload[start + 0x7c] = boolean(node, "ImmediatelyEnabled", false, path)?;
    for (name, default, offset) in [
        ("BlockedDuration", "2s", 8),
        ("Duration", "0s", 0x44),
        ("UnitDuration", "0s", 0x48),
    ] {
        let time = sage_time(node.attribute(name).unwrap_or(default), name, path)?;
        data.word(start + offset, time.to_bits());
    }
    for (name, default, offset) in [
        (
            "InfiltratedStatusFilter",
            "DESTROYED UNDER_CONSTRUCTION SOLD UNPACKING",
            0xc,
        ),
        ("ObjectStatusToSet", "NO_REFUND", 0x60),
    ] {
        write_status_flags(
            node.attribute(name).unwrap_or(default),
            start + offset,
            data,
        )?;
    }
    for (name, default, offset) in [
        ("NameOfVoiceToUseForHostileEnter", "VoiceInfiltrate", 0x30),
        (
            "NameOfVoiceToUseForInfiltrationDeath",
            "VoiceInfiltrateDeath",
            0x34,
        ),
    ] {
        data.word(
            start + offset,
            fast_hash(node.attribute(name).unwrap_or(default).as_bytes()),
        );
    }
    let amount = finite_number(node.attribute("Amount").unwrap_or("0"), "Amount", path)?;
    data.word(start + 0x4c, amount.to_bits());
    let effect = match node.attribute("Effect").unwrap_or("RADAR_FREEZE").trim() {
        "INVALID" => u32::MAX,
        value => enum_value(
            value,
            &[
                "RADAR_FREEZE",
                "DISABLE",
                "STEAL_MONEY",
                "VISION",
                "ENERGY",
                "RESET",
                "VISION_UNITS",
                "PARALYZE",
                "KILL",
            ],
            path,
        )?,
    };
    data.word(start + 0x50, effect);

    // EA assigns ordinals to explicit references first, then fills schema defaults.
    let explicit = node.attributes().filter_map(|attr| {
        REFERENCES
            .iter()
            .find(|&&(name, _, _, _)| name == attr.name())
            .map(|&(_, kind, _, offset)| (attr.value(), kind, offset))
    });
    let defaults = REFERENCES
        .iter()
        .filter(|&&(name, _, _, _)| node.attribute(name).is_none())
        .map(|&(_, kind, default, offset)| (default, kind, offset));
    let mut filters = [None; 2];
    for (value, kind, offset) in explicit.chain(defaults) {
        if value.trim().is_empty() {
            continue;
        }
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, kind, path)?;
        reference_types.push(id.type_id);
        if kind == "GameObject" {
            data.word(start + offset, id.instance_id);
            continue;
        }
        references.push(id.into());
        let ordinal = references.len() as u32;
        if offset == 0x58 || offset == 0x5c {
            filters[(offset - 0x58) / 4] = Some(ordinal);
        } else {
            data.word(start + offset, ordinal);
        }
    }

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut entries = vec![base(
        0x2900_211d,
        inherited(0xa50d_729a, inherited(0xd75d_93a2, module)),
    )];
    for &(name, kind, offset) in FIELDS {
        let target = if offset == 0x58 || offset == 0x5c {
            match filters[(offset - 0x58) / 4] {
                Some(ordinal) => {
                    let address = data.allocate(start + offset, 4)?;
                    data.word(address, ordinal);
                    Target::Data(address)
                }
                None => Target::Null,
            }
        } else {
            Target::Data(start + offset)
        };
        entries.push(Entry::named(name, kind, target));
    }
    Ok((KIND, Table::record(entries)))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn infiltrator_contain_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-infiltrator.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-infiltrator.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-infiltrator.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-infiltrator.relo");
        let manifest = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-infiltrator.manifest"
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
