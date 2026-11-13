use super::super::binary::BinaryData;
use super::super::global_ui::enum_value;
use super::super::tokenized::{Entry, Table, Target};
use super::super::{
    AssetReference, asset_reference, finite_number, hash_name, sage_time, significant_children,
    validate_attributes,
};
use super::behaviors_basic::{base, inherited};
use super::draws::{boolean, fields, model_condition_index, write_model_conditions};
use super::parking::encode_inline_filter;
use super::recall::filter_table;
use super::spawn::{UPGRADE, id_list, weak_reference};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;

const KIND: u32 = 0x1773_bb2e;
const REAL: u32 = 0x6458_64f3;
const UINT: u32 = 0x7631_77a4;
const ANGLE: u32 = 0x4e16_ea4d;

const MODEL_FIELDS: &[(&str, u32, usize)] = &[
    ("AddConditionFlags", 0x67db_bfc9, 0x24),
    ("RemoveConditionFlags", 0x67db_bfc9, 0x60),
    ("AddTempConditionFlag", 0x1572_3e0b, 0x9c),
    ("TempConditionTime", 0x9986_69e8, 0xa0),
];

pub(super) fn encode(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
    references: &mut Vec<AssetReference>,
    reference_types: &mut Vec<u32>,
) -> io::Result<(u32, Table)> {
    let path = data.path;
    let receiver = node.has_tag_name((SAGE_NS, "TechUpgradeReceiver"));
    let command = node.has_tag_name((SAGE_NS, "CommandSetUpgrade"));
    let modifier = node.has_tag_name((SAGE_NS, "AttributeModifierUpgrade"));
    let model = node.has_tag_name((SAGE_NS, "ModelConditionUpgrade"));
    let basic = command || modifier || model;
    let kind = if model {
        0x4ddd_5c7f
    } else if modifier {
        0x74a2_4c45
    } else if command {
        0x7eb7_f219
    } else if receiver {
        KIND
    } else {
        0xff7b_cdb3
    };
    let radar = ["RadarDrawModule", "RadarRotateSpeed", "RadarRotateOffset"];
    let allowed: Vec<_> = ["id", "Radius", "RadiusY", "DistanceType", "FXLineBoneName"]
        .into_iter()
        .filter(|&name| !basic || name == "id")
        .chain(command.then_some("CommandSet"))
        .chain(modifier.then_some("AttributeModifier"))
        .chain(
            MODEL_FIELDS
                .iter()
                .filter(|_| model)
                .map(|&(name, _, _)| name),
        )
        .chain(radar.into_iter().filter(|_| receiver))
        .chain(UPGRADE.iter().map(|&(name, _, _)| name))
        .collect();
    validate_attributes(node, &allowed, path)?;
    let extra_field = if modifier {
        Some(("AttributeModifier", "AttributeModifier", 0x09dd_cff3))
    } else if command {
        Some(("CommandSet", "LogicCommandSet", 0x85d4_4b5d))
    } else {
        None
    };
    let extra_value = if let Some((name, target_type, _)) = extra_field
        && let Some(value) = node
            .attribute(name)
            .filter(|value| !value.trim().is_empty())
    {
        let value = value.split_once('\\').map_or(value, |(name, _)| name);
        let id = asset_reference(value, target_type, path)?;
        reference_types.push(id.type_id);
        if modifier {
            references.push(id.into());
            references.len() as u32
        } else {
            id.instance_id
        }
    } else {
        0
    };

    let mut children = significant_children(node).peekable();
    let anim = children.next_if(|child| child.has_tag_name((SAGE_NS, "CustomAnimAndDuration")));
    let mut ids = [Vec::new(), Vec::new()];
    for (name, list) in ["TriggeredBy", "ConflictsWith"].into_iter().zip(&mut ids) {
        while let Some(child) = children.next_if(|child| child.has_tag_name((SAGE_NS, name))) {
            list.push(weak_reference(
                child,
                "UpgradeTemplate",
                data,
                reference_types,
            )?);
        }
    }
    let filter = children.next_if(|child| !basic && child.has_tag_name((SAGE_NS, "ObjectFilter")));
    if children.next().is_some() {
        return Err(invalid(path, "unexpected tech upgrade child"));
    }

    let start = data.allocate(
        pointer,
        if model {
            0xa4
        } else if basic {
            0x28
        } else if receiver {
            0xb4
        } else {
            0xa8
        },
    )?;
    data.word(start, kind);
    data.word(start + 4, hash_name(required(node, "id", path)?));
    for &(name, _, offset) in UPGRADE {
        data.payload[start + offset] = boolean(node, name, name == "WaypointModeTerminal", path)?;
    }
    let radius_y_and_filter = if basic {
        None
    } else {
        let radius = finite_number(node.attribute("Radius").unwrap_or("0"), "Radius", path)?;
        data.word(start + 0x24, radius.to_bits());
        let radius_y = if let Some(value) = node.attribute("RadiusY") {
            let value = finite_number(value, "RadiusY", path)?;
            let offset = data.allocate(start + 0x28, 4)?;
            data.word(offset, value.to_bits());
            Target::Data(offset)
        } else {
            Target::Null
        };
        let distance = enum_value(
            node.attribute("DistanceType").unwrap_or("CIRCULAR"),
            &["CIRCULAR", "RECTANGULAR"],
            path,
        )?;
        data.word(start + 0x2c, distance);
        if let Some(value) = node.attribute("FXLineBoneName") {
            data.string(start + 0x30, value)?;
        }
        if receiver {
            if let Some(value) = node.attribute("RadarDrawModule") {
                data.word(start + 0x38, hash_name(value));
            }
            for (name, default, offset) in [
                ("RadarRotateSpeed", "0.1r", 0x3c),
                ("RadarRotateOffset", "0r", 0x40),
            ] {
                let value = node.attribute(name).unwrap_or(default).trim();
                let radians = if let Some(number) = value.strip_suffix('d') {
                    finite_number(number, name, path)?.to_radians()
                } else if let Some(number) = value.strip_suffix('r') {
                    finite_number(number, name, path)?
                } else {
                    return Err(invalid(path, format!("invalid {name}")));
                };
                data.word(start + offset, radians.to_bits());
            }
        }
        let filter_start = start + if receiver { 0x44 } else { 0x38 };
        let filter = match filter {
            Some(node) => encode_inline_filter(node, filter_start, data, reference_types)?,
            None => filter_table(filter_start, [Vec::new(), Vec::new()]),
        };
        Some((radius_y, filter))
    };
    let anim = match anim {
        Some(node) => Target::Table(encode_anim(node, start + 8, data)?),
        None => Target::Null,
    };
    let [triggers, conflicts] = ids;
    let triggers = id_list(data, start + 0xc, triggers)?;
    let conflicts = id_list(data, start + 0x14, conflicts)?;

    let module = Table::record(vec![
        Entry::new(0x19b0_42a0, 0x23e6_50b8, Target::Data(start)),
        Entry::new(0x8cdc_9898, 0xbdf0_7965, Target::Data(start + 4)),
    ]);
    let mut upgrade = vec![base(0xa50d_729a, inherited(0xd75d_93a2, module))];
    upgrade.extend(fields(start, UPGRADE));
    upgrade.extend([
        Entry::named("CustomAnimAndDuration", 0x6b74_6af6, anim),
        Entry::named("TriggeredBy", 0x26fa_cfc7, Target::Table(triggers)),
        Entry::named("ConflictsWith", 0x26fa_cfc7, Target::Table(conflicts)),
    ]);
    let mut entries = vec![base(0xae60_ae98, Table::record(upgrade))];
    let Some((radius_y, filter)) = radius_y_and_filter else {
        if model {
            for &(name, _, offset) in &MODEL_FIELDS[..2] {
                write_model_conditions(node.attribute(name).unwrap_or(""), start + offset, data)?;
            }
            if let Some(flag) = node.attribute("AddTempConditionFlag") {
                let value = if flag == "INVALID" {
                    u32::MAX
                } else {
                    model_condition_index(flag, path)? as u32
                };
                data.word(start + 0x9c, value);
            }
            let time = sage_time(
                node.attribute("TempConditionTime").unwrap_or("0s"),
                "TempConditionTime",
                path,
            )?;
            data.word(start + 0xa0, time.to_bits());
            entries.extend(fields(start, MODEL_FIELDS));
            return Ok((kind, Table::record(entries)));
        }
        let (name, _, token) = extra_field.expect("basic upgrade has one field");
        data.word(start + 0x24, extra_value);
        entries.push(Entry::named(name, token, Target::Data(start + 0x24)));
        return Ok((kind, Table::record(entries)));
    };
    entries.extend([
        Entry::named("Radius", REAL, Target::Data(start + 0x24)),
        Entry::named("RadiusY", REAL, radius_y),
    ]);
    entries.extend(fields(
        start,
        &[
            (
                "DistanceType",
                if receiver { 0x3412_7322 } else { 0x01c9_f540 },
                0x2c,
            ),
            ("FXLineBoneName", 0x1a29_33aa, 0x30),
        ],
    ));
    if receiver {
        entries.extend(fields(
            start,
            &[
                ("RadarDrawModule", 0xbdf0_7965, 0x38),
                ("RadarRotateSpeed", ANGLE, 0x3c),
                ("RadarRotateOffset", ANGLE, 0x40),
            ],
        ));
    }
    entries.push(Entry::named(
        "ObjectFilter",
        0xe46e_a3b2,
        Target::Table(filter),
    ));
    Ok((kind, Table::record(entries)))
}

pub(super) fn encode_anim(
    node: Node<'_, '_>,
    pointer: usize,
    data: &mut BinaryData<'_>,
) -> io::Result<Table> {
    let path = data.path;
    validate_attributes(node, &["AnimState", "Frames", "Trigger"], path)?;
    if has_content(node) {
        return Err(invalid(path, "unexpected CustomAnimAndDuration content"));
    }
    let start = data.allocate(pointer, 12)?;
    let state = match node.attribute("AnimState").unwrap_or("INVALID") {
        "INVALID" => u32::MAX,
        value => model_condition_index(value, path)? as u32,
    };
    data.word(start, state);
    for (name, offset) in [("Frames", 4), ("Trigger", 8)] {
        let value = node
            .attribute(name)
            .unwrap_or("0")
            .trim()
            .parse::<u32>()
            .map_err(|_| invalid(path, format!("invalid {name}")))?;
        data.word(start + offset, value);
    }
    Ok(Table::record(
        fields(
            start,
            &[
                ("AnimState", 0x1572_3e0b, 0),
                ("Frames", UINT, 4),
                ("Trigger", UINT, 8),
            ],
        )
        .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use crate::compile::static_assets::compile_asset;
    use crate::manifest::Manifest;
    use std::path::Path;

    #[test]
    fn tech_upgrade_receiver_matches_ea() {
        let doc = roxmltree::Document::parse(include_str!(
            "../../../../tests/fixtures/game-object-tech.xml"
        ))
        .unwrap();
        let actual = compile_asset(
            doc.root_element().first_element_child().unwrap(),
            Path::new("game-object-tech.xml"),
            "",
        )
        .unwrap();
        let bin = include_bytes!("../../../../tests/fixtures/game-object-tech.bin");
        let relo = include_bytes!("../../../../tests/fixtures/game-object-tech.relo");
        let expected = Manifest::parse(include_bytes!(
            "../../../../tests/fixtures/game-object-tech.manifest"
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
