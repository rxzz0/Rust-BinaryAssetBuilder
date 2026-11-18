//! Native binary encoding for the first verified GameObject subset.
//!
//! The token layout is the fixed RA3 GameObject field table. Unsupported
//! fields are rejected until their binary representation is verified.

use super::super::binary::BinaryData;
use super::super::tokenized::{self, Entry, Table, Target};
use super::super::{Marshaled, hash_name, significant_children, validate_attributes};
use super::{
    active_body, ai_update, behaviors_basic, children, client_behaviors, client_updates, draws,
    fields, physics, structures,
};
use crate::compile::{SAGE_NS, has_content, invalid, required};
use roxmltree::Node;
use std::io;
use std::path::Path;

const TOKENS: &[u8; 0x7b4] = include_bytes!("token_layout.bin");
const ROOT_SIZE: usize = 0x24c;
const BODY_SIZE: usize = 0x4c;

const ROOT_DEFAULTS: &[(usize, u32)] = &[
    (0x4c, 0xffff_ffff),
    (0xa8, 0xbf80_0000),
    (0xb4, 0x4416_0000),
    (0xb8, 0x3f80_0000),
    (0xbc, 0x4120_0000),
    (0xc4, 1),
    (0xc8, 1),
    (0xd8, 0x3f80_0000),
    (0xe8, 0x96),
    (0xec, 0x40a0_0000),
    (0xf0, 0x40a0_0000),
    (0xf4, 0x41a0_0000),
    (0xf8, 0x4040_0000),
    (0xfc, 0x4316_0000),
    (0x100, 0x1e),
    (0x104, 0x3f80_0000),
    (0x10c, 0xbf80_0000),
    (0x11c, 0x43b4_0000),
    (0x120, 1),
    (0x124, 0x7fff_ffff),
    (0x128, 0x19),
    (0x12c, 0xffff_ffff),
    (0x158, 0xffff_ffff),
    (0x15c, 0xffff_ffff),
    (0x160, 0x4000_0000),
    (0x168, 0x3f00_0000),
    (0x16c, 0x3ecc_cccd),
    (0x170, 0x3f66_6666),
    (0x178, 0x1f4),
    (0x17c, 0x4348_0000),
    (0x180, 0x9b),
    (0x240, 0x100),
    (0x248, 0x10100),
];

const BODY_DEFAULTS: &[(usize, u32)] = &[
    (0, 0x88e9_4f7a),
    (0xc, 0x3f2a_aaaa),
    (0x10, 0x3eaa_aaaa),
    (0x14, 0xbf80_0000),
    (0x38, 0x4348_0000),
];

fn word(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(in crate::compile::static_assets) fn compile(
    node: Node<'_, '_>,
    path: &Path,
) -> io::Result<Marshaled> {
    let mut body = None;
    let mut behaviors = None;
    let mut draws = None;
    let mut ai = None;
    let mut client = None;
    let mut updates = None;
    let mut voice = None;
    let mut sound = None;
    let mut eva = None;
    let mut shadow = None;
    for child in significant_children(node) {
        let slot = match child.tag_name().name() {
            "Body" => &mut body,
            "Behaviors" => &mut behaviors,
            "Draws" => &mut draws,
            "AI" => &mut ai,
            "ClientBehaviors" => &mut client,
            "ClientUpdates" => &mut updates,
            "AudioArrayVoice" => &mut voice,
            "AudioArraySound" => &mut sound,
            "EvaEventArray" => &mut eva,
            "ShadowInfo" if child.tag_name().namespace() == Some(SAGE_NS) => {
                if shadow.replace(child).is_some() {
                    return Err(invalid(path, "repeated GameObject ShadowInfo"));
                }
                continue;
            }
            "DisplayName"
            | "GameDependency"
            | "ProjectedBuildabilityInfo"
            | "ObjectResourceInfo"
            | "ArmorSet"
            | "LocomotorSet"
            | "EquivalentTo"
            | "SkirmishAIInformation"
            | "VisionInfo"
            | "CrusherInfo"
            | "Geometry"
                if child.tag_name().namespace() == Some(SAGE_NS) =>
            {
                continue;
            }
            kind => {
                return Err(invalid(
                    path,
                    format!("unsupported GameObject child {kind}"),
                ));
            }
        };
        if child.tag_name().namespace() != Some(SAGE_NS)
            || child.attributes().len() != 0
            || slot.replace(child).is_some()
        {
            return Err(invalid(path, "invalid or repeated GameObject module group"));
        }
    }
    let body = body.ok_or_else(|| invalid(path, "GameObject requires Body"))?;
    let mut modules = significant_children(body);
    let module = modules
        .next()
        .ok_or_else(|| invalid(path, "Body requires ImmortalBody"))?;
    if modules.next().is_some() || module.tag_name().namespace() != Some(SAGE_NS) {
        return Err(invalid(path, "Body requires a single supported module"));
    }
    let (tokens, body_bytes): (&[u8], [u8; BODY_SIZE]) = match module.tag_name().name() {
        "ActiveBody" => (
            active_body::TOKEN_LAYOUT,
            active_body::encode(module, path)?,
        ),
        "ImmortalBody" => (TOKENS, immortal_body(module, path)?),
        "PropBody" => (TOKENS, [0; BODY_SIZE]),
        kind => return Err(invalid(path, format!("unsupported Body module {kind}"))),
    };

    let mut data = BinaryData {
        payload: vec![0; ROOT_SIZE],
        relocations: Vec::new(),
        path,
    };
    for &(offset, value) in ROOT_DEFAULTS {
        data.word(offset, value);
    }
    let mut template_data = data.payload.clone();
    template_data.extend_from_slice(&body_bytes);
    let mut table = Table::from_layout(tokens, &template_data, path)?;
    let mut reference_types = Vec::new();
    let mut references = Vec::new();
    let mut material_float_names = Vec::new();
    let mut create_object_modules = Vec::new();
    let mut spawn_modules = Vec::new();
    let mut upgrade_relocation_anchors = Vec::new();
    let mut chooser_relocations = Vec::new();
    let mut subobject_modules = Vec::new();
    let mut aura_modules = Vec::new();
    let mut auto_heal_modules = Vec::new();
    let mut slaughter_modules = Vec::new();
    let mut transport_modules = Vec::new();
    let mut truck_relocations = Vec::new();
    fields::encode(node, &mut data, &mut reference_types, &mut references)?;
    let mut child_targets =
        children::encode(node, &mut data, &mut references, &mut reference_types)?;
    if let Some(group) = draws {
        let target = draws::encode(
            group,
            &mut data,
            &mut references,
            &mut reference_types,
            &mut material_float_names,
            &mut truck_relocations,
        )?;
        child_targets.push(("Draws", target));
    }
    let behavior_tokens = if let Some(group) = behaviors {
        let modules: Vec<_> = significant_children(group).collect();
        let list = data.allocate(0x1d0, 8)?;
        let pointers = data.list(list, modules.len(), 4)?;
        let mut items = Vec::new();
        for (index, module) in modules.into_iter().enumerate() {
            if module.tag_name().namespace() != Some(SAGE_NS) {
                return Err(invalid(path, "unexpected behavior namespace"));
            }
            let pointer = pointers + index * 4;
            let encoded = if module.has_tag_name((SAGE_NS, "CreateObjectDie")) {
                let start = data.payload.len() as u32;
                let result = super::create_object_die::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?;
                create_object_modules.push(start);
                Some(result)
            } else if module.has_tag_name((SAGE_NS, "WeaponSetUpdate")) {
                Some(super::weapon_set::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                    &mut chooser_relocations,
                )?)
            } else if module.has_tag_name((SAGE_NS, "SpawnBehavior")) {
                let start = data.payload.len() as u32;
                let result =
                    super::spawn::encode(module, pointer, &mut data, &mut reference_types)?;
                spawn_modules.push(start);
                Some(result)
            } else if matches!(
                module.tag_name().name(),
                "TechUpgrade"
                    | "TechUpgradeReceiver"
                    | "CommandSetUpgrade"
                    | "AttributeModifierUpgrade"
                    | "ModelConditionUpgrade"
            ) {
                let start = data.payload.len() as u32;
                let result = super::tech_upgrade::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?;
                upgrade_relocation_anchors.push((start, data.payload.len() as u32 - 1));
                Some(result)
            } else if module.has_tag_name((SAGE_NS, "SubObjectsUpgrade")) {
                let start = data.payload.len() as u32;
                let result = super::subobjects::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?;
                subobject_modules.push(start);
                Some(result)
            } else {
                if module.has_tag_name((SAGE_NS, "AttributeModifierAuraUpdate")) {
                    aura_modules.push(data.payload.len() as u32);
                }
                if module.has_tag_name((SAGE_NS, "AutoHealBehavior")) {
                    auto_heal_modules.push(data.payload.len() as u32);
                }
                if module.has_tag_name((SAGE_NS, "TransportContain")) {
                    transport_modules.push(data.payload.len() as u32);
                }
                if module.has_tag_name((SAGE_NS, "SlaughterHordeContain")) {
                    slaughter_modules.push(data.payload.len() as u32);
                }
                let start = data.payload.len() as u32;
                let result = behaviors_basic::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?;
                if matches!(
                    module.tag_name().name(),
                    "StealthDetectorUpdate" | "ObjectCreationUpgrade"
                ) {
                    upgrade_relocation_anchors.push((start, data.payload.len() as u32 - 1));
                }
                if module.has_tag_name((SAGE_NS, "FireWeaponWhenDead")) {
                    upgrade_relocation_anchors.push((start, start + 0x88));
                }
                result
            };
            let (kind, fields) = match encoded {
                Some(encoded) => encoded,
                None if module.has_tag_name((SAGE_NS, "Physics")) => physics::encode(
                    module,
                    pointers + index * 4,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?,
                None => structures::encode(
                    module,
                    pointers + index * 4,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?
                .ok_or_else(|| {
                    invalid(
                        path,
                        format!("unsupported behavior {}", module.tag_name().name()),
                    )
                })?,
            };
            items.push((
                0,
                Target::Table(Table::sequence(vec![Entry::new(
                    2,
                    kind,
                    Target::Table(fields),
                )])),
            ));
        }
        Some(Target::Table(Table::array(list, items)))
    } else {
        None
    };
    if let Some(group) = ai {
        let target = ai_update::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("AI", target));
    }
    let body_shift = data.payload.len() - ROOT_SIZE;
    table.shift_data_from(ROOT_SIZE, body_shift);
    if module.has_tag_name((SAGE_NS, "PropBody")) {
        validate_attributes(module, &["id"], path)?;
        if has_content(module) {
            return Err(invalid(path, "PropBody cannot contain content"));
        }
        let start = data.allocate(0x1dc, 8)?;
        let kind = 0xff88_404b;
        let inherited = behaviors_basic::module_base(module, kind, start, &mut data, false)?;
        let body = Table::sequence(vec![Entry::new(
            2,
            kind,
            Target::Table(behaviors_basic::inherited(
                0xf491_98c2,
                Table::record(vec![inherited]),
            )),
        )]);
        child_targets.push(("Body", Target::Table(body)));
    } else {
        let body_start = data.allocate(0x1dc, BODY_SIZE)?;
        data.payload[body_start..body_start + BODY_SIZE].copy_from_slice(&body_bytes);
    }
    if let Some(group) = updates {
        let target =
            client_updates::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("ClientUpdates", target));
    }
    if let Some(group) = client {
        let target =
            client_behaviors::encode(group, &mut data, &mut references, &mut reference_types)?;
        child_targets.push(("ClientBehaviors", target));
    }
    if let Some(target) = children::encode_geometry(node, &mut data)? {
        child_targets.push(("Geometry", target));
    }
    for node in [voice, sound, eva].into_iter().flatten() {
        let target = client_behaviors::encode_object_audio(
            node,
            &mut data,
            &mut references,
            &mut reference_types,
        )?;
        child_targets.push((node.tag_name().name(), target));
    }
    if let Some(node) = shadow {
        let table = super::shadow::encode(
            node,
            0x21c,
            &mut data,
            &mut references,
            &mut reference_types,
        )?;
        child_targets.push(("ShadowInfo", Target::Table(table)));
    }
    if let Some(target) = children::encode_vision(node, &mut data)? {
        child_targets.push(("VisionInfo", target));
    }
    if let Some(target) =
        children::encode_crusher(node, &mut data, &mut references, &mut reference_types)?
    {
        child_targets.push(("CrusherInfo", target));
    }
    if let Some(target) = children::encode_buildability(node, &mut data, &mut reference_types)? {
        child_targets.push(("ProjectedBuildabilityInfo", target));
    }
    if let Some(target) = behavior_tokens
        && !table.replace_target("Behaviors", target)
    {
        return Err(invalid(path, "GameObject layout is missing Behaviors"));
    }
    for (name, target) in child_targets {
        if !table.replace_target(name, target) {
            return Err(invalid(
                path,
                format!("GameObject layout is missing {name}"),
            ));
        }
    }
    let relocation_key = |pointer| {
        // TruckDraw emits its derived pointers before the inherited draw pointers.
        let index = truck_relocations.partition_point(|&(start, _)| start <= pointer);
        if index > 0 {
            let (start, end) = truck_relocations[index - 1];
            if (0xd8..0x18c).contains(&(pointer - start)) {
                return (start, pointer - start);
            }
            if (start + 0x18c..end).contains(&pointer) {
                return (end, pointer - start);
            }
        }
        // EA emits the inherited status pointers after the two upgrade lists.
        let index = create_object_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = create_object_modules[index - 1];
            match pointer - start {
                0x10 => return (start + 0x48, 0),
                0x14 => return (start + 0x48, 1),
                _ => {}
            }
        }
        // SpawnBehavior lists its inherited upgrade pointers after its own fields.
        let index = spawn_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = spawn_modules[index - 1];
            match pointer - start {
                0x10 => return (start + 0x5c, 0),
                0x18 => return (start + 0x5c, 1),
                _ => {}
            }
        }
        // These modules emit derived relocations before inherited upgrade pointers.
        let index = upgrade_relocation_anchors.partition_point(|&(start, _)| start <= pointer);
        if index > 0 {
            let (start, anchor) = upgrade_relocation_anchors[index - 1];
            match pointer - start {
                8 => return (anchor, 0),
                0x10 => return (anchor, 1),
                0x18 => return (anchor, 2),
                _ => {}
            }
        }
        // EA lists the nugget array pointer before the inherited upgrade pointers.
        let index = subobject_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = subobject_modules[index - 1];
            match pointer - start {
                8 => return (start + 0x2c, 0),
                0x10 => return (start + 0x2c, 1),
                0x18 => return (start + 0x2c, 2),
                _ => {}
            }
        }
        // Aura modules list the filter pointer before inherited upgrade pointers.
        let index = aura_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = aura_modules[index - 1];
            match pointer - start {
                8 => return (start + 0x5c, 3),
                0x10 => return (start + 0x5c, 4),
                0x18 => return (start + 0x5c, 5),
                _ => {}
            }
        }
        let index = auto_heal_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = auto_heal_modules[index - 1];
            match pointer - start {
                8 => return (start + 0x64, 3),
                0x10 => return (start + 0x64, 4),
                0x18 => return (start + 0x64, 5),
                _ => {}
            }
        }
        // TransportContain lists the derived pointers before its OpenContain pointers.
        let index = transport_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = transport_modules[index - 1];
            if (0x94..0x14c).contains(&(pointer - start)) {
                return (start, pointer - start);
            }
        }
        let index = slaughter_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = slaughter_modules[index - 1];
            let rank = match pointer - start {
                0x130 => 0,
                0x138 => 1,
                0x13c => 2,
                0xa4 => 3,
                0xa8 => 4,
                0xac => 5,
                0x68 => 6,
                0x6c => 7,
                0x74 => 8,
                0x7c => 9,
                0x84 => 10,
                _ => return (pointer, 2),
            };
            return (start, rank);
        }
        (pointer, 2)
    };
    data.relocations
        .sort_unstable_by_key(|&pointer| relocation_key(pointer));
    let float_relocations: Vec<_> = material_float_names
        .into_iter()
        .map(|name| {
            data.relocations
                .binary_search_by_key(&relocation_key(name), |&pointer| relocation_key(pointer))
                .expect("float name relocation")
        })
        .collect();
    for index in float_relocations {
        // The SDK lists a float constant's value pointer before its name pointer.
        debug_assert_eq!(data.relocations[index + 1], data.relocations[index] + 8);
        data.relocations.swap(index, index + 1);
    }
    for (weapons, chooser) in chooser_relocations {
        // EA emits the chooser relocation before the inherited weapon-list pointer.
        let first = data
            .relocations
            .iter()
            .position(|&pointer| pointer == weapons)
            .expect("weapon list relocation");
        let last = data
            .relocations
            .iter()
            .position(|&pointer| pointer == chooser)
            .expect("turret chooser relocation");
        data.relocations[first..=last].rotate_right(1);
    }
    tokenized::finish(
        table,
        data,
        Marshaled {
            reference_types,
            references,
            ..Marshaled::default()
        },
    )
}

fn immortal_body(module: Node<'_, '_>, path: &Path) -> io::Result<[u8; BODY_SIZE]> {
    validate_attributes(module, &["id", "MaxHealth"], path)?;
    if has_content(module) {
        return Err(invalid(path, "ImmortalBody cannot contain content"));
    }
    let module_id = required(module, "id", path)?;
    let health = module
        .attribute("MaxHealth")
        .unwrap_or("0")
        .parse::<f32>()
        .map_err(|_| invalid(path, "invalid MaxHealth"))?;
    if !health.is_finite() {
        return Err(invalid(path, "invalid MaxHealth"));
    }
    let mut body = [0; BODY_SIZE];
    for &(offset, value) in BODY_DEFAULTS {
        word(&mut body, offset, value);
    }
    word(&mut body, 4, hash_name(module_id));
    word(&mut body, 8, health.to_bits());
    Ok(body)
}
