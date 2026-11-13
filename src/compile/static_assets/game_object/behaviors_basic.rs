use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{AssetReference, asset_reference, fast_hash, hash_name, validate_attributes};
use crate::compile::{has_content, required};
use roxmltree::Node;
use std::io;

fn base(kind: u32, table: Table) -> Entry {
    Entry::new(1, kind, Target::Table(table))
}

fn inherited(kind: u32, table: Table) -> Table {
    Table::record(vec![base(kind, table)])
}

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Option<(u32, Table)>> {
    let (kind, size, extra) = match node.tag_name().name() {
        "ObjectSMCHelper" => (0x7f72_f242, 8, None),
        "ObjectDefectionHelper" => (0x18a4_208f, 8, None),
        "AttributeModifierPoolUpdate" => (0xe622_f055, 8, None),
        "StancesBehavior" => (0xc0a1_e43d, 12, Some("StanceTemplate")),
        "LUAEventList" => (0xfa6a_c8b6, 16, Some("EventListName")),
        "DestroyDie" => (0x0f7e_c801, 52, None),
        _ => return Ok(None),
    };
    let allowed: Vec<_> = std::iter::once("id").chain(extra).collect();
    validate_attributes(node, &allowed, data.path)?;
    if has_content(node) && kind != 0x0f7e_c801 {
        return Err(crate::compile::invalid(
            data.path,
            "unexpected behavior module content",
        ));
    }
    let id = required(node, "id", data.path)?;
    let offset = data.allocate(pointer, size)?;
    data.word(offset, kind);
    data.word(offset + 4, hash_name(id));
    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(offset)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(offset + 4)),
    ]);
    let behavior = inherited(0xd75d_93a2, module);
    if kind == 0x0f7e_c801 {
        let mut children = super::super::significant_children(node);
        let die = children
            .next()
            .filter(|n| n.has_tag_name((crate::compile::SAGE_NS, "DieMuxData")))
            .ok_or_else(|| crate::compile::invalid(data.path, "DestroyDie requires DieMuxData"))?;
        if children.next().is_some() || has_content(die) {
            return Err(crate::compile::invalid(
                data.path,
                "unexpected DestroyDie content",
            ));
        }
        validate_attributes(die, &["DeathTypes", "DeathTypesForbidden"], data.path)?;
        for (field, at) in [("DeathTypes", 36), ("DeathTypesForbidden", 44)] {
            let flags = death_flags(die.attribute(field).unwrap_or(""), data.path)?;
            data.word(offset + at, flags as u32);
            data.word(offset + at + 4, (flags >> 32) as u32);
        }
        for (at, value) in [(24, 0xbf80_0000), (28, 0x3c8e_fa35), (32, 0xbc8e_fa35)] {
            data.word(offset + at, value);
        }
        let mux = Table::record(vec![
            Entry::new(
                0xf317_4ddf,
                0x6d66_ae18,
                Target::Table(Table::array(offset + 8, [])),
            ),
            Entry::new(0xac7f_1e22, 0xa7bd_811e, Target::Null),
            Entry::new(0x239e_b201, 0xa7bd_811e, Target::Null),
            Entry::new(0x025c_cf9e, 0x6458_64f3, Target::Data(offset + 24)),
            Entry::new(0x3913_ca4e, 0x4e16_ea4d, Target::Data(offset + 28)),
            Entry::new(0xdfe5_9442, 0x4e16_ea4d, Target::Data(offset + 32)),
            Entry::new(0x1335_1530, 0x034d_f1a1, Target::Data(offset + 36)),
            Entry::new(0x2254_777e, 0x034d_f1a1, Target::Data(offset + 44)),
        ]);
        let die = Table::record(vec![
            base(0xa50d_729a, behavior),
            Entry::new(0x33d7_093c, 0x0084_001b, Target::Table(mux)),
        ]);
        return Ok(Some((kind, inherited(0x9e4f_2b72, die))));
    }
    if kind == 0xfa6a_c8b6 {
        let name = required(node, "EventListName", data.path)?;
        data.string(offset + 8, name)?;
        return Ok(Some((
            kind,
            Table::record(vec![
                base(0xa50d_729a, behavior),
                Entry::new(0x05c8_facb, 0x1a29_33aa, Target::Data(offset + 8)),
            ]),
        )));
    }
    let update = inherited(0x2900_211d, inherited(0xa50d_729a, behavior));
    if kind == 0xc0a1_e43d {
        let value = required(node, "StanceTemplate", data.path)?;
        references.push(asset_reference(value, "StanceTemplate", data.path)?.into());
        reference_types.push(fast_hash(b"StanceTemplate"));
        data.word(offset + 8, references.len() as u32);
        return Ok(Some((
            kind,
            Table::record(vec![
                base(0xf100_2505, update),
                Entry::new(0x9684_c743, 0x0d82_18fb, Target::Data(offset + 8)),
            ]),
        )));
    }
    let update = inherited(0xf100_2505, update);
    let table = if kind == 0xe622_f055 {
        update
    } else {
        inherited(0x29ca_d490, update)
    };
    Ok(Some((kind, table)))
}

fn death_flags(value: &str, path: &std::path::Path) -> io::Result<u64> {
    let schema = roxmltree::Document::parse(include_str!(
        "../../../../schemas/ra3/xsd/Includes/Death.xsd"
    ))
    .unwrap();
    let names: Vec<_> = schema
        .descendants()
        .filter(|n| n.tag_name().name() == "enumeration")
        .filter_map(|n| n.attribute("value"))
        .collect();
    let mut flags = 0;
    for name in value.split_ascii_whitespace() {
        if name == "ALL" {
            flags = u64::MAX;
        } else {
            let bit = names.iter().position(|&n| n == name).ok_or_else(|| {
                crate::compile::invalid(path, format!("unknown death type {name}"))
            })?;
            flags |= 1u64 << bit;
        }
    }
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::super::super::{Marshaled, tokenized};
    use super::*;
    use roxmltree::Document;
    use std::path::Path;

    fn u32_at(bytes: &[u8], offset: usize) -> usize {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize
    }

    #[test]
    fn basic_behavior_data_and_tokens_match_ea() {
        for name in ["smc", "defection", "pool", "stances", "lua", "destroy"] {
            let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            let xml = std::fs::read_to_string(
                base.join(format!("game-object-behavior-basic-{name}.xml")),
            )
            .unwrap();
            let bytes =
                std::fs::read(base.join(format!("game-object-behavior-basic-{name}.bin"))).unwrap();
            let oracle = &bytes[4..];
            let root = u32_at(oracle, 8);
            let start = u32_at(oracle, root + 0x254);
            let end = u32_at(oracle, root + 0x1dc);
            let document = Document::parse(&xml).unwrap();
            let node = document
                .descendants()
                .find(|node| {
                    node.parent()
                        .is_some_and(|p| p.tag_name().name() == "Behaviors")
                })
                .unwrap();
            let mut data = BinaryData {
                payload: vec![0; 4],
                relocations: Vec::new(),
                path: Path::new("probe"),
            };
            let mut references = Vec::new();
            let mut reference_types = Vec::new();
            let (kind, table) = encode(node, 0, &mut data, &mut references, &mut reference_types)
                .unwrap()
                .unwrap();
            assert_eq!(kind as usize, u32_at(oracle, start));
            let mut expected = oracle[start..end].to_vec();
            if name == "lua" {
                let pointer = u32_at(&expected, 12) - start + 4;
                expected[12..16].copy_from_slice(&(pointer as u32).to_le_bytes());
            }
            assert_eq!(&data.payload[4..], expected, "{name} module data");
            let token_start = 0x648;
            let module_fields = (token_start..root)
                .step_by(12)
                .find(|&at| u32_at(oracle, at) == 0x19b0_42a0)
                .unwrap();
            let token_end = if name == "destroy" {
                0x738
            } else {
                module_fields + 36
            };
            let encoded = tokenized::finish(table, data, Marshaled::default()).unwrap();
            let data_start = token_end - token_start + 4;
            let mut expected_tokens = oracle[token_start..token_end].to_vec();
            for entry in expected_tokens.as_chunks_mut::<12>().0 {
                let target = u32_at(entry, 8);
                let adjusted = if target >= root {
                    target - start + 4 + data_start
                } else if target != 0 {
                    target - token_start
                } else {
                    0
                };
                entry[8..12].copy_from_slice(&(adjusted as u32).to_le_bytes());
            }
            assert_eq!(
                &encoded.payload[..expected_tokens.len()],
                expected_tokens,
                "{name} module tokens"
            );
        }
    }
}
