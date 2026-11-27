//! Native binary encoding for the first verified GameObject subset.
//!
//! The token layout is the fixed RA3 GameObject field table. Unsupported
//! fields are rejected until their binary representation is verified.

use super::super::binary::BinaryData;
use super::super::tokenized::{self, Entry, Table, Target};
use super::super::{Marshaled, significant_children, validate_attributes};
use super::{
    active_body, ai_update, behaviors_basic, children, client_behaviors, client_updates, draws,
    fields, physics, structures,
};
use crate::compile::{SAGE_NS, has_content, invalid};
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
            | "DisplayNameTransformed"
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
        .ok_or_else(|| invalid(path, "Body requires a module"))?;
    if modules.next().is_some() || module.tag_name().namespace() != Some(SAGE_NS) {
        return Err(invalid(path, "Body requires a single supported module"));
    }
    let body_name = module.tag_name().name();
    let variant = active_body::variant(body_name);
    let (tokens, body_bytes): (&[u8], [u8; BODY_SIZE]) = match variant {
        Some((kind, _)) => (
            active_body::TOKEN_LAYOUT,
            active_body::encode(module, kind, path)?,
        ),
        None if matches!(body_name, "PropBody" | "InactiveBody") => (TOKENS, [0; BODY_SIZE]),
        None => {
            return Err(invalid(
                path,
                format!("unsupported Body module {body_name}"),
            ));
        }
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
    let mut eject_passenger_modules = Vec::new();
    let mut movement_power_modules = Vec::new();
    let mut early_module_fields = Vec::new();
    let mut spawn_modules = Vec::new();
    let mut upgrade_relocation_anchors = Vec::new();
    let mut chooser_relocations = Vec::new();
    let mut subobject_modules = Vec::new();
    let mut aura_modules = Vec::new();
    let mut auto_heal_modules = Vec::new();
    let mut slaughter_modules = Vec::new();
    let mut transport_modules = Vec::new();
    let mut derived_draw_relocations = super::draws::DrawRelocations::default();
    let mut child_targets = Vec::new();
    if let Some(browser) = node.attribute("Browser") {
        let categories: Vec<_> = browser.split_ascii_whitespace().collect();
        if !categories.is_empty() {
            let start = data.list(0x24, categories.len(), 8)?;
            let mut items = Vec::with_capacity(categories.len());
            for (index, category) in categories.into_iter().enumerate() {
                let offset = start + index * 8;
                data.string(offset, category)?;
                items.push((0, Target::Data(offset)));
            }
            child_targets.push(("Browser", Target::Table(Table::array(0x24, items))));
        }
    }
    fields::encode(node, &mut data, &mut reference_types, &mut references)?;
    child_targets.extend(children::encode(
        node,
        &mut data,
        &mut references,
        &mut reference_types,
    )?);
    if let Some(group) = draws {
        let target = draws::encode(
            group,
            &mut data,
            &mut references,
            &mut reference_types,
            &mut material_float_names,
            &mut derived_draw_relocations,
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
                    | "GeometryUpgrade"
                    | "RemoveUpgradeUpgrade"
                    | "ArmorUpgrade"
            ) {
                let start = data.payload.len() as u32;
                let result = super::tech_upgrade::encode(
                    module,
                    pointer,
                    &mut data,
                    &mut references,
                    &mut reference_types,
                )?;
                let anchor = match module.tag_name().name() {
                    "GeometryUpgrade" | "RemoveUpgradeUpgrade" => start + 0x31,
                    _ => data.payload.len() as u32 - 1,
                };
                upgrade_relocation_anchors.push((start, anchor));
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
                let transport_size = match module.tag_name().name() {
                    "TransportContain" => Some(super::garrison::TRANSPORT_SIZE),
                    "HordeContain" => Some(super::horde_contain::SIZE),
                    _ => None,
                };
                if let Some(size) = transport_size {
                    transport_modules.push((data.payload.len() as u32, size as u32));
                }
                if module.has_tag_name((SAGE_NS, "SlaughterHordeContain")) {
                    slaughter_modules.push(data.payload.len() as u32);
                }
                let start = data.payload.len() as u32;
                if module.has_tag_name((SAGE_NS, "EjectPassengersSpecialAbilityUpdate")) {
                    eject_passenger_modules.push(start);
                }
                if module.has_tag_name((SAGE_NS, "MovementSpecialPower")) {
                    movement_power_modules.push(start);
                }
                let early_fields: &[u32] = match module.tag_name().name() {
                    "SpecialDisguiseUpdate" => &[0x100],
                    "MoneyGainAttachUpdate" => &[0x158],
                    "DominateEnemySpecialPower" => &[super::dominate_enemy::FILTER_POINTER as u32],
                    "BeamSpecialAbilityUpdate" => &[0xf8, 0x108],
                    "RadarFreezeSpecialPower" | "RadarSpySpecialPower" => &[0x1c8],
                    "AssembleStructureSpecialAbilityUpdate" => &[0xfc],
                    _ => &[],
                };
                if !early_fields.is_empty() {
                    early_module_fields.push((start, early_fields));
                }
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
                if module.has_tag_name((SAGE_NS, "CostModifierUpgrade")) {
                    upgrade_relocation_anchors.push((start, start + 0x3d));
                }
                if module.has_tag_name((SAGE_NS, "ReplaceSelfUpgrade")) {
                    upgrade_relocation_anchors.push((start, start + 0x31));
                }
                if module.has_tag_name((SAGE_NS, "SubStructureBuild")) {
                    upgrade_relocation_anchors.push((start, start + 0x34));
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
    if variant.is_none() {
        validate_attributes(module, &["id"], path)?;
        if has_content(module) {
            return Err(invalid(path, format!("{body_name} cannot contain content")));
        }
        let start = data.allocate(0x1dc, 8)?;
        let kind = if body_name == "InactiveBody" {
            0x2923_4e6e
        } else {
            0xff88_404b
        };
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
        let shield = body_name == "ShieldBody";
        let body_size = if shield {
            active_body::SHIELD_SIZE
        } else {
            BODY_SIZE
        };
        let body_start = data.allocate(0x1dc, body_size)?;
        data.payload[body_start..body_start + BODY_SIZE].copy_from_slice(&body_bytes);
        if let Some(target) = active_body::damage_ranges(
            module,
            body_start,
            &mut data,
            &mut references,
            &mut reference_types,
        )? {
            let body = table
                .variant_fields("Body")
                .ok_or_else(|| invalid(path, "GameObject layout is missing Body variant"))?;
            if !body.replace_target("DamageRangeModifier", target) {
                return Err(invalid(
                    path,
                    "GameObject layout is missing DamageRangeModifier",
                ));
            }
        }
        if shield {
            let fields = table
                .variant_fields("Body")
                .ok_or_else(|| invalid(path, "GameObject layout is missing Body variant"))?;
            let inherited = std::mem::replace(fields, Table::record(Vec::new()));
            *fields = active_body::shield(
                module,
                body_start,
                inherited,
                &mut data,
                &mut references,
                &mut reference_types,
            )?;
        }
    }
    if let Some((kind, bases)) = variant
        && !table.derive_variant("Body", kind, bases)
    {
        return Err(invalid(path, "GameObject layout is missing Body variant"));
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
        // Model draw extensions emit derived pointers before inherited pointers.
        if let Some(key) = derived_draw_relocations.key(pointer) {
            return key;
        }
        // EjectPassengers allocates its bone fields before inherited ability pointers.
        let index = eject_passenger_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = eject_passenger_modules[index - 1];
            let offset = pointer - start;
            if matches!(offset, 0x44 | 0xc8 | 0xe8) {
                return (start + 0x110, offset);
            }
        }
        // MovementSpecialPower emits the derived masks before inherited filter pointers.
        let index = movement_power_modules.partition_point(|&start| start <= pointer);
        if index > 0 {
            let start = movement_power_modules[index - 1];
            let offset = pointer - start;
            if matches!(offset, 0x1cc | 0x1d0) {
                return (start, offset);
            }
        }
        // Some abilities emit derived pointers before inherited pointers.
        let index = early_module_fields.partition_point(|&(start, _)| start <= pointer);
        if index > 0 {
            let (start, offsets) = early_module_fields[index - 1];
            let offset = pointer - start;
            if offsets.contains(&offset) {
                return (start, offset);
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
        // The containment hierarchy emits derived pointers before inherited pointers.
        let index = transport_modules.partition_point(|&(start, _)| start <= pointer);
        if index > 0 {
            let (start, size) = transport_modules[index - 1];
            let offset = pointer - start;
            let transport_size = super::garrison::TRANSPORT_SIZE as u32;
            if (0x94..size).contains(&offset) {
                let rank = if offset < transport_size && size > transport_size {
                    offset + size
                } else {
                    offset
                };
                return (start, rank);
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
