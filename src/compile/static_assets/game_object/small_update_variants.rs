use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, finite_number, sage_int, sage_time, sage_uint, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{encode_die_mux, module_base};
use super::draws::{boolean, fields, model_condition_index};
use super::passive_area::reference;
use super::spawn::id_list;
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    const ACTIVATE: u32 = 0xc54a_d4fb;
    const OCL: u32 = 0xb8b5_8445;
    const REPAIR: u32 = 0xc6f3_f62d;
    const EMP: u32 = 0xd330_2d7e;
    const ENVIRONMENT: u32 = 0x75fe_8d90;
    const OCL_FIELDS: &[(&str, u32, usize)] = &[
        ("OCL", 0x42e4_e9d1, 8),
        ("MinDelay", 0x6f6d_1ae7, 12),
        ("MaxDelay", 0x6f6d_1ae7, 16),
        ("CreateAtEdge", 0x77e3_f4f7, 24),
        ("Amount", 0x23e6_50b8, 20),
    ];
    const REPAIR_FIELDS: &[(&str, u32, usize)] = &[
        ("DelayTime", 0x9986_69e8, 8),
        ("MasterDeadDieFX", 0x0d1c_8e83, 12),
        ("Options", 0xb202_9371, 16),
        ("InstantRepairModelCondition", 0x1572_3e0b, 20),
        ("InstantRepairAnimDuration", 0x9986_69e8, 24),
    ];
    const EMP_FIELDS: &[(&str, u32, usize)] = &[
        ("Lifetime", 0x9986_69e8, 8),
        ("StartFadeDelay", 0x9986_69e8, 12),
        ("DisabledDuration", 0x9986_69e8, 16),
        ("StartScale", 0x6458_64f3, 20),
        ("TargetScaleMin", 0x6458_64f3, 24),
        ("TargetScaleMax", 0x6458_64f3, 28),
    ];
    const ENVIRONMENT_FIELDS: &[(&str, u32, usize)] =
        &[("Enabled", 0x77e3_f4f7, 36), ("Size", 0x37da_54ab, 8)];

    let path = data.path;
    let name = node.tag_name().name();
    let (kind, size, attributes) = match name {
        "ActivateSpecialPowerOnCreate" => (ACTIVATE, 0x10, &[][..]),
        "OCLUpdate" => (OCL, 0x1c, OCL_FIELDS),
        "InitiateRepairDie" => (REPAIR, 0x20, REPAIR_FIELDS),
        "EMPUpdate" => (EMP, 0x28, EMP_FIELDS),
        "DestructibleEnvironmentUpdate" => (ENVIRONMENT, 0x28, ENVIRONMENT_FIELDS),
        _ => return Err(invalid(path, format!("unexpected update module {name}"))),
    };
    let allowed: Vec<_> = std::iter::once("id")
        .chain(attributes.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let start = data.allocate(pointer, size)?;
    let mut entries = vec![module_base(node, kind, start, data, true)?];
    entries.extend(fields(start, attributes));

    match kind {
        ACTIVATE => {
            let mut ordinals = Vec::new();
            for child in significant_children(node) {
                if !child.has_tag_name((SAGE_NS, "SpecialPowerTemplate"))
                    || child.children().any(|child| child.is_element())
                {
                    return Err(invalid(path, format!("unexpected {name} content")));
                }
                validate_attributes(child, &[], path)?;
                let value: String = child
                    .children()
                    .filter(Node::is_text)
                    .filter_map(|item| item.text())
                    .collect();
                ordinals.push(reference(
                    value.trim(),
                    "SpecialPowerTemplate",
                    path,
                    references,
                    reference_types,
                )?);
            }
            if ordinals.is_empty() {
                return Err(invalid(
                    path,
                    format!("{name} requires SpecialPowerTemplate"),
                ));
            }
            let list = id_list(data, start + 8, ordinals)?;
            entries.push(Entry::named(
                "SpecialPowerTemplate",
                0xaee8_3888,
                Target::Table(list),
            ));
        }
        OCL => {
            if has_content(node) {
                return Err(invalid(path, format!("unexpected {name} content")));
            }
            let ordinal = reference(
                node.attribute("OCL").unwrap_or(""),
                "ObjectCreationList",
                path,
                references,
                reference_types,
            )?;
            data.word(start + 8, ordinal);
            for (field, offset) in [("MinDelay", 12), ("MaxDelay", 16)] {
                let millis = sage_uint(node.attribute(field).unwrap_or("0"), field, path)?;
                data.word(start + offset, millis.div_ceil(200));
            }
            data.word(
                start + 20,
                sage_int(node.attribute("Amount").unwrap_or("-1"), "Amount", path)?,
            );
            data.payload[start + 24] = boolean(node, "CreateAtEdge", false, path)?;
        }
        REPAIR => {
            for (field, offset) in [("DelayTime", 8), ("InstantRepairAnimDuration", 24)] {
                let time = sage_time(node.attribute(field).unwrap_or("0s"), field, path)?;
                data.word(start + offset, time.to_bits());
            }
            let ordinal = reference(
                node.attribute("MasterDeadDieFX").unwrap_or(""),
                "FXList",
                path,
                references,
                reference_types,
            )?;
            data.word(start + 12, ordinal);
            for option in node
                .attribute("Options")
                .unwrap_or("")
                .split_ascii_whitespace()
            {
                if option != "REPAIR_INSTANT" {
                    return Err(invalid(path, format!("invalid {name} option {option:?}")));
                }
                data.word(start + 16, 1);
            }
            let condition = match node
                .attribute("InstantRepairModelCondition")
                .unwrap_or("INVALID")
                .trim()
            {
                "INVALID" => u32::MAX,
                value => model_condition_index(value, path)? as u32,
            };
            data.word(start + 20, condition);
            let mut children = significant_children(node).peekable();
            let die =
                if let Some(child) = children.next_if(|item| item.has_tag_name((SAGE_NS, "Die"))) {
                    let offset = data.allocate(start + 28, 0x2c)?;
                    Target::Table(encode_die_mux(child, offset, data)?)
                } else {
                    Target::Null
                };
            if children.next().is_some() {
                return Err(invalid(path, format!("unexpected {name} child")));
            }
            entries.push(Entry::named("Die", 0x0084_001b, die));
        }
        EMP => {
            for &(field, tag, offset) in EMP_FIELDS {
                let value = if tag == 0x9986_69e8 {
                    sage_time(node.attribute(field).unwrap_or("0s"), field, path)?
                } else {
                    finite_number(node.attribute(field).unwrap_or("1"), field, path)?
                };
                data.word(start + offset, value.to_bits());
            }
            let mut children = significant_children(node).peekable();
            for (field, offset) in [("StartColor", 32), ("EndColor", 36)] {
                let target = match children.next_if(|child| child.has_tag_name((SAGE_NS, field))) {
                    Some(child) => Target::Table(emp_color(child, start + offset, data)?),
                    None => Target::Null,
                };
                entries.push(Entry::named(field, 0x0284_26a3, target));
            }
            if children.next().is_some() {
                return Err(invalid(path, format!("unexpected {name} child")));
            }
        }
        ENVIRONMENT => {
            let size = super::super::global_ui::enum_value(
                node.attribute("Size").unwrap_or("LIGHT"),
                &["LIGHT", "MEDIUM", "HEAVY"],
                path,
            )?;
            data.word(start + 8, size);
            data.payload[start + 36] = boolean(node, "Enabled", false, path)?;
            let mut children = significant_children(node);
            for (field, offset) in [("LeftOffset", 12), ("RightOffset", 24)] {
                let child = children
                    .next()
                    .filter(|child| child.has_tag_name((SAGE_NS, field)))
                    .ok_or_else(|| invalid(path, format!("{name} requires {field}")))?;
                super::super::fx_list::vector(child, false, start + offset, data)?;
                entries.push(Entry::named(
                    field,
                    0x2936_8077,
                    Target::Data(start + offset),
                ));
            }
            if children.next().is_some() {
                return Err(invalid(path, format!("unexpected {name} child")));
            }
        }
        _ => unreachable!("small update module"),
    }
    Ok((kind, Table::record(entries)))
}

fn emp_color(node: Node<'_, '_>, pointer: usize, data: &mut BinaryData<'_>) -> io::Result<Table> {
    const CHANNELS: &[(&str, u32, usize)] = &[
        ("r", 0x6458_64f3, 0),
        ("g", 0x6458_64f3, 4),
        ("b", 0x6458_64f3, 8),
    ];
    let path = data.path;
    validate_attributes(node, &["r", "g", "b"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected EMP color content"));
    }
    let start = data.allocate(pointer, 12)?;
    for &(name, _, offset) in CHANNELS {
        let value = finite_number(node.attribute(name).unwrap_or("0"), name, path)?;
        data.word(start + offset, value.to_bits());
    }
    Ok(Table::record(fields(start, CHANNELS).collect()))
}
