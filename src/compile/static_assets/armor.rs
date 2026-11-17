use super::binary::BinaryData;
use super::tokenized::{self, Entry, FLOAT_TYPE, Table, Target};
use super::{Marshaled, fast_hash, percentage, significant_children, validate_attributes};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::{Document, Node};
use std::io;
use std::path::Path;
use std::sync::OnceLock;

const FIELDS: [&str; 5] = [
    "Default",
    "DamageScalar",
    "SideDamageScalar",
    "RearDamageScalar",
    "FlankedPenalty",
];

pub(super) fn compile(node: Node<'_, '_>, path: &Path) -> io::Result<Marshaled> {
    let allowed: Vec<_> = std::iter::once("id").chain(FIELDS).collect();
    validate_attributes(node, &allowed, path)?;
    let mut data = BinaryData {
        payload: vec![0; 28],
        relocations: Vec::new(),
        path,
    };
    let mut fields = Vec::new();
    for (index, field) in FIELDS.into_iter().enumerate() {
        let value = node.attribute(field).unwrap_or("100%");
        data.word(4 * index, percentage(value, field, path)?.to_bits());
        fields.push(Entry::named(field, FLOAT_TYPE, Target::Data(4 * index)));
    }
    let items: Vec<_> = significant_children(node).collect();
    let start = data.list(20, items.len(), 8)?;
    let mut tokens = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        if !item.has_tag_name((SAGE_NS, "Armor")) || has_content(item) {
            return Err(invalid(path, "expected ArmorTemplate Armor entry"));
        }
        validate_attributes(item, &["Damage", "Percent"], path)?;
        let damage = required(item, "Damage", path)?;
        let index_value = damage_types()
            .iter()
            .position(|value| value == damage)
            .ok_or_else(|| invalid(path, format!("unknown armor damage type {damage:?}")))?;
        let percent = required(item, "Percent", path)?;
        let offset = start + index * 8;
        data.word(offset, index_value as u32);
        data.word(offset + 4, percentage(percent, "Percent", path)?.to_bits());
        tokens.push((
            0,
            Target::Table(Table::record(vec![
                Entry::named("Damage", fast_hash(b"DamageType"), Target::Data(offset)),
                Entry::named("Percent", FLOAT_TYPE, Target::Data(offset + 4)),
            ])),
        ));
    }
    fields.push(Entry::named(
        "Armor",
        fast_hash(b"ArmorListType"),
        Target::Table(Table::array(20, tokens)),
    ));
    tokenized::finish(Table::record(fields), data, Marshaled::default())
}

pub(super) fn damage_types() -> &'static [String] {
    static TYPES: OnceLock<Vec<String>> = OnceLock::new();
    TYPES.get_or_init(|| {
        let schema = Document::parse(include_str!(
            "../../../schemas/ra3/xsd/Includes/DamageDef.xsd"
        ))
        .expect("bundled damage schema must be valid");
        let xsd = "http://www.w3.org/2001/XMLSchema";
        schema
            .root_element()
            .children()
            .find(|node| {
                node.has_tag_name((xsd, "simpleType"))
                    && node.attribute("name") == Some("DamageType")
            })
            .expect("bundled DamageType")
            .descendants()
            .filter(|node| node.has_tag_name((xsd, "enumeration")))
            .filter_map(|node| node.attribute("value").map(str::to_owned))
            .collect()
    })
}
