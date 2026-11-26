use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{finite_number, percentage, significant_children, validate_attributes};
use super::behaviors_basic::module_base;
use super::draws::fields;
use super::secondary_damage::encode_status;
use crate::compile::{SAGE_NS, invalid};
use roxmltree::Node;
use std::{io, sync::OnceLock};

const KIND: u32 = 0xb14a_0525;
const FIELDS: &[(&str, u32, usize)] = &[
    ("HealthTrigger", 0x6458_64f3, 8),
    ("DamagePerSecond", 0x6458_64f3, 12),
    ("DeathType", 0xc608_ed36, 16),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    validate_attributes(
        node,
        &["id", "HealthTrigger", "DamagePerSecond", "DeathType"],
        path,
    )?;

    let trigger = percentage(
        node.attribute("HealthTrigger").unwrap_or("10%"),
        "HealthTrigger",
        path,
    )?;
    let damage = finite_number(
        node.attribute("DamagePerSecond").unwrap_or("100"),
        "DamagePerSecond",
        path,
    )?;
    static DEATH_TYPES: OnceLock<Vec<String>> = OnceLock::new();
    let death_types = DEATH_TYPES.get_or_init(|| {
        super::ai_update::enum_values(
            include_str!("../../../../schemas/ra3/xsd/Includes/Death.xsd"),
            "DeathType",
        )
    });
    let death = node.attribute("DeathType").unwrap_or("NONE");
    let death_index = death_types
        .iter()
        .position(|value| value == death)
        .ok_or_else(|| invalid(path, format!("invalid DeathType {death}")))?;

    let start = data.allocate(pointer, 24)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    data.word(start + 8, trigger.to_bits());
    data.word(start + 12, damage.to_bits());
    data.word(start + 16, death_index as u32);
    entries.extend(fields(start, FIELDS));

    let mut children = significant_children(node);
    let status = match children.next() {
        Some(child) if child.has_tag_name((SAGE_NS, "ObjectStatusValidation")) => {
            Target::Table(encode_status(child, start + 20, data)?)
        }
        Some(_) => {
            return Err(invalid(
                path,
                "expected LoseHealthUpdate ObjectStatusValidation",
            ));
        }
        None => Target::Null,
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected LoseHealthUpdate child"));
    }
    entries.push(Entry::named("ObjectStatusValidation", 0x3e8f_938c, status));
    Ok((KIND, Table::record(entries)))
}
