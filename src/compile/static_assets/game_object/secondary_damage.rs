use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, percentage, sage_time,
    significant_children, validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::sync::OnceLock;

const KIND: u32 = 0xb21c_7e67;
const REAL: u32 = 0x6458_64f3;
const BOOL: u32 = 0x77e3_f4f7;
const STATUS: u32 = 0xa7bd_811e;
const STATUS_BYTES: usize = 0x1c;

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
            "UpdateDuration",
            "DissipateAmount",
            "MaxDamageClampScaler",
        ],
        path,
    )?;
    let mut color = None;
    let mut status = None;
    let mut effects = Vec::new();
    for child in significant_children(node) {
        if child.has_tag_name((SAGE_NS, "Color")) && color.is_none() {
            color = Some(child);
        } else if child.has_tag_name((SAGE_NS, "EffectInfo")) {
            effects.push(child);
        } else if child.has_tag_name((SAGE_NS, "ObjectStatusValidation")) && status.is_none() {
            status = Some(child);
        } else {
            return Err(invalid(
                path,
                format!(
                    "unsupported or repeated SecondaryDamageTracker child {}",
                    child.tag_name().name()
                ),
            ));
        }
    }

    let start = data.allocate(pointer, 0x24)?;
    data.word(start, KIND);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    data.word(
        start + 8,
        sage_time(
            node.attribute("UpdateDuration").unwrap_or("1s"),
            "UpdateDuration",
            path,
        )?
        .to_bits(),
    );
    let amount = node
        .attribute("DissipateAmount")
        .unwrap_or("0")
        .trim()
        .parse::<u32>()
        .map_err(|_| invalid(path, "invalid DissipateAmount"))?;
    data.word(start + 0xc, amount);
    data.word(
        start + 0x10,
        percentage(
            node.attribute("MaxDamageClampScaler").unwrap_or("125%"),
            "MaxDamageClampScaler",
            path,
        )?
        .to_bits(),
    );

    let color = match color {
        Some(node) => Target::Table(encode_color(node, start + 0x14, data)?),
        None => Target::Null,
    };
    let count = start + 0x18;
    let first = data.list(count, effects.len(), 0x10)?;
    let mut items = Vec::with_capacity(effects.len());
    for (index, node) in effects.into_iter().enumerate() {
        let table = encode_effect(
            node,
            first + index * 0x10,
            data,
            references,
            reference_types,
        )?;
        items.push((0, Target::Table(table)));
    }
    let status = match status {
        Some(node) => Target::Table(encode_status(node, start + 0x20, data)?),
        None => Target::Null,
    };
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
            ("UpdateDuration", 0x9986_69e8, 8),
            ("DissipateAmount", 0x7631_77a4, 0xc),
            ("MaxDamageClampScaler", REAL, 0x10),
        ],
    ));
    entries.extend([
        Entry::named("Color", 0xaee7_62a5, color),
        Entry::named(
            "EffectInfo",
            0x25f5_379f,
            Target::Table(Table::array(count, items)),
        ),
        Entry::named("ObjectStatusValidation", 0x3e8f_938c, status),
    ]);
    Ok((KIND, Table::record(entries)))
}

fn encode_color(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    check_leaf(node, &["R", "G", "B"], data)?;
    let start = data.allocate(pointer, 12)?;
    for (name, offset) in [("R", 0), ("G", 4), ("B", 8)] {
        let value = finite_number(required(node, name, data.path)?, name, data.path)?;
        data.word(start + offset, value.to_bits());
    }
    Ok(Table::record(
        fields(start, &[("R", REAL, 0), ("G", REAL, 4), ("B", REAL, 8)]).collect(),
    ))
}

fn encode_effect(
    node: Node<'_, '_>,
    start: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<Table> {
    const FLAGS: &[(&str, u32, usize)] = &[
        ("OnEnterDisable", BOOL, 0xc),
        ("OnEnterCancelSpecialAbilities", BOOL, 0xd),
        ("SetUpgradedArmor", BOOL, 0xe),
    ];
    let path = data.path;
    check_leaf(
        node,
        &[
            "Fraction",
            "AttributeModifier",
            "OnEnterDisable",
            "OnEnterCancelSpecialAbilities",
            "SetUpgradedArmor",
            "Options",
        ],
        data,
    )?;
    data.word(
        start,
        finite_number(required(node, "Fraction", path)?, "Fraction", path)?.to_bits(),
    );
    if let Some(value) = node
        .attribute("AttributeModifier")
        .filter(|value| !value.trim().is_empty())
    {
        let plain = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(plain, "AttributeModifier", path)?;
        references.push(id.into());
        reference_types.push(id.type_id);
        data.word(start + 4, references.len() as u32);
    }
    let mut options = 0;
    for flag in node
        .attribute("Options")
        .unwrap_or("")
        .split_ascii_whitespace()
    {
        if flag != "DIE_ON_ENTER" {
            return Err(invalid(path, format!("unknown EffectInfo option {flag}")));
        }
        options = 1;
    }
    data.word(start + 8, options);
    for &(name, _, offset) in FLAGS {
        data.payload[start + offset] = boolean(node, name, false, path)?;
    }
    let mut entries: Vec<_> = fields(
        start,
        &[("Fraction", REAL, 0), ("AttributeModifier", 0x09dd_cff3, 4)],
    )
    .collect();
    entries.extend(fields(start, FLAGS));
    entries.push(Entry::named(
        "Options",
        0xa2f3_7a52,
        Target::Data(start + 8),
    ));
    Ok(Table::record(entries))
}

fn encode_status(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    check_leaf(node, &["ForbiddenStatus", "RequiredStatus"], data)?;
    let start = data.allocate(pointer, 2 * STATUS_BYTES)?;
    static FLAGS: OnceLock<Vec<String>> = OnceLock::new();
    let flags = FLAGS.get_or_init(|| {
        Document::parse(include_str!(
            "../../../../schemas/ra3/xsd/Includes/ObjectStatus.xsd"
        ))
        .unwrap()
        .descendants()
        .find(|node| node.attribute("name") == Some("ObjectStatusType"))
        .unwrap()
        .descendants()
        .filter(|node| node.tag_name().name() == "enumeration")
        .filter_map(|node| node.attribute("value").map(str::to_owned))
        .collect()
    });
    for (name, offset) in [("ForbiddenStatus", 0), ("RequiredStatus", STATUS_BYTES)] {
        for flag in node.attribute(name).unwrap_or("").split_ascii_whitespace() {
            let index = flags
                .iter()
                .take(STATUS_BYTES * 8)
                .position(|value| value == flag)
                .ok_or_else(|| invalid(data.path, format!("unknown object status {flag}")))?;
            data.payload[start + offset + index / 8] |= 1 << (index % 8);
        }
    }
    Ok(Table::record(
        fields(
            start,
            &[
                ("ForbiddenStatus", STATUS, 0),
                ("RequiredStatus", STATUS, STATUS_BYTES),
            ],
        )
        .collect(),
    ))
}

fn check_leaf(node: Node<'_, '_>, attributes: &[&str], data: &BinaryData<'_>) -> io::Result<()> {
    validate_attributes(node, attributes, data.path)?;
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
    fn secondary_damage_tracker_matches_ea() {
        let document = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-secondary-damage.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            document.root_element().first_element_child().unwrap(),
            Path::new("game-object-secondary-damage.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-secondary-damage.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-secondary-damage.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-secondary-damage.manifest"
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
