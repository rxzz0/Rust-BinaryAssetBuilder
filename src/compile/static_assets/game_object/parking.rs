use super::super::binary::BinaryData;
use super::super::global_ui::{enum_value, kind_of_flags};
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    asset_reference, finite_number, hash_name, sage_time, significant_children, text_value,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields};
use super::secondary_damage::{STATUS_BYTES, write_status_flags};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0xe9bb_f43b;
const INT: u32 = 0x23e6_50b8;
const REAL: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;
const TIME: u32 = 0x9986_69e8;
const FIELDS: &[(&str, u32, usize)] = &[
    ("NumRows", INT, 8),
    ("NumCols", INT, 0xc),
    ("ApproachHeight", REAL, 0x10),
    ("ParkInHangars", BOOL, 0x34),
    ("DoorOpenTime", TIME, 0x14),
    ("DoorCloseTime", TIME, 0x18),
    ("LowersOntoParkingPlaceOnProduction", BOOL, 0x35),
    ("CanSetRallyPoint", BOOL, 0x36),
    ("DoorsUnpackOpen", BOOL, 0x37),
    ("QueueDistance", REAL, 0x1c),
    ("LimitedProductionAircraftSpots", INT, 0x20),
    ("HelipadBoneName", 0x1a29_33aa, 0x24),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let allowed: Vec<_> = std::iter::once("id")
        .chain(FIELDS.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let mut children = significant_children(node).peekable();
    let filter = children.next_if(|n| n.has_tag_name((SAGE_NS, "CanParkHereFilter")));
    let rally = children.next_if(|n| n.has_tag_name((SAGE_NS, "DefaultRallyPointOffset")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ParkingPlaceBehavior child"));
    }

    let start = data.allocate(pointer, 0x38)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, kind, offset) in FIELDS {
        let value = node.attribute(name);
        let word = match kind {
            BOOL => {
                let default = matches!(name, "CanSetRallyPoint" | "DoorsUnpackOpen");
                data.payload[start + offset] = boolean(node, name, default, path)?;
                continue;
            }
            INT => value
                .unwrap_or("0")
                .trim()
                .parse::<i32>()
                .map_err(|_| invalid(path, format!("invalid {name}")))? as u32,
            REAL => {
                let default = if name == "QueueDistance" { "100" } else { "0" };
                finite_number(value.unwrap_or(default), name, path)?.to_bits()
            }
            TIME => sage_time(value.unwrap_or("0s"), name, path)?.to_bits(),
            _ => {
                data.string(start + offset, value.unwrap_or("HeliPark01"))?;
                continue;
            }
        };
        data.word(start + offset, word);
    }

    let filter = match filter {
        Some(node) => Target::Table(encode_filter(node, start + 0x2c, data, reference_types)?),
        None => Target::Null,
    };
    let rally = match rally {
        Some(node) => {
            let axes = ["x", "y", "z"];
            validate_attributes(node, &axes, path)?;
            if has_content(node) {
                return Err(invalid(path, "unexpected DefaultRallyPointOffset content"));
            }
            let start = data.allocate(start + 0x30, 12)?;
            for (i, axis) in axes.into_iter().enumerate() {
                let value = finite_number(node.attribute(axis).unwrap_or("0"), axis, path)?;
                data.word(start + i * 4, value.to_bits());
            }
            Target::Data(start)
        }
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
        Entry::named("CanParkHereFilter", 0xe46e_a3b2, filter),
        Entry::named("DefaultRallyPointOffset", 0x2936_8077, rally),
    ]);
    Ok((KIND, Table::record(entries)))
}

pub(in crate::compile::static_assets) fn encode_filter(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let start = data.allocate(pointer, 0x70)?;
    encode_inline_filter(node, start, data, reference_types)
}

pub(in crate::compile::static_assets) fn encode_inline_filter(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(
        node,
        &[
            "id",
            "Rule",
            "Relationship",
            "Alignment",
            "Include",
            "Exclude",
            "StatusBitFlags",
            "StatusBitFlagsExclude",
        ],
        path,
    )?;
    if let Some(id) = node.attribute("id") {
        data.word(start, hash_name(id));
    }
    let rule = enum_value(
        node.attribute("Rule").unwrap_or("NONE"),
        &["UNPARSED", "ALL", "ANY", "NONE"],
        path,
    )?;
    let alignment = enum_value(
        node.attribute("Alignment").unwrap_or("NONE"),
        &["NONE", "EVIL", "GOOD"],
        path,
    )?;
    let mut relationships = 0;
    for value in node
        .attribute("Relationship")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        relationships |= 1
            << enum_value(
                value,
                &["ALLIES", "ENEMIES", "NEUTRAL", "SAME_PLAYER"],
                path,
            )?;
    }
    data.word(start + 4, rule);
    data.word(start + 8, relationships);
    data.word(start + 12, alignment);
    for (name, offset) in [("Include", 0x10), ("Exclude", 0x34)] {
        let flags = kind_of_flags(node.attribute(name).unwrap_or(""), path)?;
        for (i, word) in flags.into_iter().enumerate() {
            data.word(start + offset + i * 4, word);
        }
    }

    let mut entries: Vec<_> = fields(
        start,
        &[
            ("id", 0x4d3b_3f19, 0),
            ("Rule", 0x0027_bb58, 4),
            ("Relationship", 0x5c89_81a9, 8),
            ("Alignment", 0x9ffd_43eb, 0xc),
            ("Include", 0x9afc_9193, 0x10),
            ("Exclude", 0x9afc_9193, 0x34),
        ],
    )
    .collect();
    for (name, offset) in [("StatusBitFlags", 0x58), ("StatusBitFlagsExclude", 0x5c)] {
        let target = if let Some(value) = node.attribute(name) {
            let flags = data.allocate(start + offset, STATUS_BYTES)?;
            write_status_flags(value, flags, data)?;
            Target::Data(flags)
        } else {
            Target::Null
        };
        entries.push(Entry::named(name, 0xa7bd_811e, target));
    }

    let mut children = significant_children(node).peekable();
    for (name, offset) in [("IncludeThing", 0x60), ("ExcludeThing", 0x68)] {
        let mut ids = Vec::new();
        while let Some(child) = children.next_if(|n| n.has_tag_name((SAGE_NS, name))) {
            let text = text_value(child, path)?;
            let value = text.split_once('\\').map_or(text, |(name, _)| name);
            let id = asset_reference(value, "GameObject", path)?;
            reference_types.push(id.type_id);
            ids.push(id.instance_id);
        }
        let count = start + offset;
        let first = data.list(count, ids.len(), 4)?;
        let items = ids.into_iter().enumerate().map(|(i, id)| {
            let offset = first + i * 4;
            data.word(offset, id);
            (0, Target::Data(offset))
        });
        entries.push(Entry::named(
            name,
            0x9b19_049f,
            Target::Table(Table::array(count, items)),
        ));
    }
    if children.next().is_some() {
        return Err(invalid(path, "unexpected ObjectFilter child"));
    }
    Ok(Table::record(entries))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn parking_place_behavior_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-parking.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-parking.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-parking.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-parking.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-parking.manifest"
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
        assert!(actual.references.is_empty());
        assert_eq!(actual.instance_hash, expected.assets[0].instance_hash);
    }
}
