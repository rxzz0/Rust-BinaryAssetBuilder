use super::super::binary::BinaryData;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, percentage, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, encode_die_mux, module_base};
use super::draws::{boolean, write_model_conditions};
use super::secondary_damage::{STATUS_BYTES, write_status_flags};
use crate::compile::{SAGE_NS, has_content, invalid};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x9876_9794;
const REAL: u32 = 0x6458_64f3;
const TIME: u32 = 0x9986_69e8;
const ANGLE: u32 = 0x4e16_ea4d;
const BOOL: u32 = 0x77e3_f4f7;
const MODEL: u32 = 0x67db_bfc9;
const STATUS: u32 = 0xa7bd_811e;

const ATTRIBUTES: [(&str, u32, usize, &str); 19] = [
    ("SinkRate", 0x7a76_c60e, 0x08, "0"),
    ("ProbabilityModifier", 0x23e6_50b8, 0x0c, "10"),
    ("ModifierBonusPerOverkillPercent", REAL, 0x10, "0"),
    ("SinkDelay", TIME, 0x14, "0s"),
    ("SinkDelayVariance", TIME, 0x18, "0s"),
    ("DestructionDelay", TIME, 0x1c, "0s"),
    ("DestructionDelayVariance", TIME, 0x20, "0s"),
    ("DecayBeginTime", TIME, 0x24, "0s"),
    ("FlingForce", REAL, 0x28, "0"),
    ("FlingForceVariance", REAL, 0x2c, "0"),
    ("FlingPitch", ANGLE, 0x30, "0r"),
    ("FlingPitchVariance", ANGLE, 0x34, "0r"),
    ("DeathFlags", MODEL, 0x38, ""),
    ("ShadowWhenDead", BOOL, 0x70, "false"),
    ("Fade", BOOL, 0x71, "false"),
    ("FadeTime", TIME, 0x3c, "0s"),
    ("FadeDelay", TIME, 0x40, "0s"),
    ("DeathTypes", MODEL, 0x44, ""),
    ("DeathObjectStatusBits", STATUS, 0x48, ""),
];

const GROUPS: [(&str, u32, u32, &str); 4] = [
    ("FX", 0x33d0_dfae, 0x0d1c_8e83, "FXList"),
    ("OCL", 0x9747_4300, 0x42e4_e9d1, "ObjectCreationList"),
    ("Weapon", 0x9081_07ec, 0xd046_dd22, "WeaponTemplate"),
    ("Sound", 0xb8e8_7c25, 0xe111_7f93, ""),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let mut names = vec!["id"];
    names.extend(ATTRIBUTES.map(|(name, _, _, _)| name));
    validate_attributes(node, &names, path)?;
    let start = data.allocate(pointer, 0x74)?;
    let mut entries = vec![module_base(node, KIND, start, data, true)?];
    for (name, kind, relative, default) in ATTRIBUTES {
        let value = node.attribute(name).unwrap_or(default);
        let offset = start + relative;
        let target = match kind {
            MODEL | STATUS => {
                if node.attribute(name).is_none() {
                    Target::Null
                } else {
                    let bytes = if kind == MODEL { 0x3c } else { STATUS_BYTES };
                    let flags = data.allocate(offset, bytes)?;
                    if kind == MODEL {
                        write_model_conditions(value, flags, data)?;
                    } else {
                        write_status_flags(value, flags, data)?;
                    }
                    Target::Data(flags)
                }
            }
            BOOL => {
                data.payload[offset] = boolean(node, name, false, path)? as u8;
                Target::Data(offset)
            }
            _ => {
                let bits = match kind {
                    TIME => sage_time(value, name, path)?.to_bits(),
                    ANGLE => angle(value, name, data)?.to_bits(),
                    0x23e6_50b8 => value
                        .trim()
                        .parse::<i32>()
                        .map_err(|_| invalid(path, format!("invalid {name}")))?
                        as u32,
                    0x7a76_c60e => (finite_number(value, name, path)? * 0.2).to_bits(),
                    REAL if name == "ModifierBonusPerOverkillPercent" => {
                        percentage(value, name, path)?.to_bits()
                    }
                    _ => finite_number(value, name, path)?.to_bits(),
                };
                data.word(offset, bits);
                Target::Data(offset)
            }
        };
        entries.push(Entry::named(name, kind, target));
    }
    let mut children = significant_children(node).peekable();
    for (index, (name, array_kind, item_kind, reference_kind)) in GROUPS.into_iter().enumerate() {
        let mut group = Vec::new();
        while children
            .peek()
            .is_some_and(|node| node.has_tag_name((SAGE_NS, name)))
        {
            group.push(children.next().unwrap());
        }
        if group.len() > 4 {
            return Err(invalid(path, format!("too many SlowDeath {name} elements")));
        }
        let count = start + 0x4c + index * 8;
        let array = data.list(count, group.len(), 12)?;
        let mut items = Vec::with_capacity(group.len());
        for (index, child) in group.into_iter().enumerate() {
            let offset = array + index * 12;
            let phase = super::super::global_ui::enum_value(
                child.attribute("Type").unwrap_or("INITIAL"),
                &["INITIAL", "MIDPOINT", "FINAL", "HIT_GROUND"],
                path,
            )?;
            data.word(offset, phase);
            let list = if name == "Sound" {
                validate_attributes(child, &["Type", "List"], path)?;
                if has_content(child) {
                    return Err(invalid(path, "SlowDeath Sound cannot contain content"));
                }
                let sounds = child
                    .attribute("List")
                    .unwrap_or("")
                    .split_ascii_whitespace();
                let strings = data.list(offset + 4, sounds.clone().count(), 8)?;
                let mut values = Vec::new();
                for (index, sound) in sounds.enumerate() {
                    let string = strings + index * 8;
                    data.string(string, sound)?;
                    values.push((0, Target::Data(string)));
                }
                Table::array(offset + 4, values)
            } else {
                validate_attributes(child, &["Type"], path)?;
                let refs: Vec<_> = significant_children(child).collect();
                let first = data.list(offset + 4, refs.len(), 4)?;
                let mut values = Vec::new();
                for (index, reference) in refs.into_iter().enumerate() {
                    validate_attributes(reference, &[], path)?;
                    if !reference.has_tag_name((SAGE_NS, name))
                        || reference.children().any(|node| node.is_element())
                    {
                        return Err(invalid(path, format!("invalid SlowDeath {name} reference")));
                    }
                    let text: String = reference
                        .children()
                        .filter(Node::is_text)
                        .filter_map(|node| node.text())
                        .collect();
                    let id = asset_reference(text.trim(), reference_kind, path)?;
                    references.push(id.into());
                    reference_types.push(id.type_id);
                    let target = first + index * 4;
                    data.word(target, references.len() as u32);
                    values.push((0, Target::Data(target)));
                }
                Table::array(offset + 4, values)
            };
            items.push((
                0,
                Target::Table(Table::record(vec![
                    base(
                        0x2f1c_af62,
                        Table::record(vec![Entry::named(
                            "Type",
                            0x350a_89a5,
                            Target::Data(offset),
                        )]),
                    ),
                    Entry::named(
                        if name == "Sound" { "List" } else { name },
                        item_kind,
                        Target::Table(list),
                    ),
                ])),
            ));
        }
        entries.push(Entry::named(
            name,
            array_kind,
            Target::Table(Table::array(count, items)),
        ));
    }
    let mux = if children
        .peek()
        .is_some_and(|node| node.has_tag_name((SAGE_NS, "DieMuxData")))
    {
        let offset = data.allocate(start + 0x6c, 0x2c)?;
        Target::Table(encode_die_mux(children.next().unwrap(), offset, data)?)
    } else {
        Target::Null
    };
    if children.next().is_some() {
        return Err(invalid(path, "unexpected SlowDeath child"));
    }
    entries.push(Entry::named("DieMuxData", 0x0084_001b, mux));
    Ok((KIND, Table::record(entries)))
}

fn angle(value: &str, name: &str, data: &BinaryData<'_>) -> io::Result<f32> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix('d') {
        Ok(finite_number(number, name, data.path)?.to_radians())
    } else if let Some(number) = value.strip_suffix('r') {
        finite_number(number, name, data.path)
    } else {
        Err(invalid(data.path, format!("invalid {name}")))
    }
}
