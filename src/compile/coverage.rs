use super::static_assets::game_object::{SCRIPTED_FLAGS, Schema};
use super::{SAGE_NS, invalid, static_assets};
use crate::manifest::Manifest;
use roxmltree::Document;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

// Dispatch coverage, not a claim of complete field support. Draw field coverage
// comes from the same flag table used by the encoder.
const MODULES: &[(&str, &[&str])] = &[
    (
        "Draws",
        &[
            "LaserDraw",
            "ScriptedModelDraw",
            "StreamDraw",
            "TracerModelDraw",
        ],
    ),
    ("AI", &["AIUpdate"]),
    ("Body", &["ActiveBody", "ImmortalBody"]),
    (
        "ClientBehaviors",
        &[
            "ModelConditionAudioLoopClientBehavior",
            "ModelConditionSoundSelectorClientBehavior",
        ],
    ),
    (
        "Behaviors",
        &[
            "WeaponSetUpdate",
            "LaserState",
            "AttackAssistResponseBehavior",
            "NotifyWhenAttackingBehavior",
            "TechUpgrade",
            "CommandSetUpgrade",
            "ProductionUpdateInfo",
            "SpecialPower",
            "ReplaceSelfSpecialAbility",
            "SlowDeath",
            "SubStructureBehavior",
            "AssignSlavesTargetObjectSpecialPower",
            "AttributeModifierUpgrade",
            "ModelConditionUpgrade",
            "SlavedUpdate",
            "PassiveAreaEffectBehavior",
            "AttachUpdate",
            "QueueDockBehavior",
            "AssistedPlacementBehavior",
            "StructurePlacementBehavior",
            "TeleportStoredObjectsSpecialPower",
            "StoreObjectsSpecialPower",
            "TeleportStoredObjectsUpdate",
            "MonitorSpecialPowerTimerUpdate",
            "WeaponFireSpecialAbilityUpdate",
            "OCLSpecialPower",
            "SquishCollide",
            "WallBuilderBehavior",
            "StreamState",
            "StatusBitsUpgrade",
            "SecondaryObjectListenerModule",
            "LiftObjectUpdate",
            "GarrisonContain",
            "InvisibilityBehavior",
            "AttributeModifierAuraUpdate",
            "SlaughterHordeContain",
            "PowerUpgrade",
            "DynamicsUpdate",
            "AutoHealBehavior",
            "AttributeModifierPoolUpdate",
            "BuildingBehavior",
            "BuildingDestructionBehavior",
            "CreateObjectDie",
            "DefaultProductionExit",
            "DestroyDie",
            "EngineerContain",
            "FXListBehavior",
            "FireWeaponWhenDead",
            "InfiltratorContain",
            "LUAEventList",
            "ObjectDefectionHelper",
            "ObjectSMCHelper",
            "ParkingPlaceBehavior",
            "Physics",
            "ProductionUpdate",
            "QueueProductionExitUpdate",
            "RecallUnitsSpecialPower",
            "RepairUpdate",
            "SecondaryDamageTracker",
            "SpawnBehavior",
            "StancesBehavior",
            "StructureSellUpdate",
            "StructureUnpackUpdate",
            "SubObjectsUpgrade",
            "TechUpgradeReceiver",
        ],
    ),
];

/// Writes an inventory of the SDK's XML declarations and the Rust encoder coverage.
pub fn sdk_coverage(schema_root: &Path, mut out: impl Write) -> io::Result<()> {
    let schema = Schema::load(schema_root)?;
    if schema.assets.is_empty() {
        return Err(invalid(schema_root, "no AssetDeclaration choices found"));
    }
    writeln!(out, "category\tname\tschema_type\tcoverage")?;
    for (name, kind) in schema.assets.iter().cloned().collect::<BTreeSet<_>>() {
        let coverage = if static_assets::has_encoder(&name) {
            "encoder-present"
        } else {
            "missing"
        };
        writeln!(out, "Asset\t{name}\t{kind}\t{coverage}")?;
    }
    let game_object = schema.shape("GameObject", schema_root)?;
    for &(group, supported) in MODULES {
        let kind = &game_object
            .children
            .iter()
            .find(|field| field.name == group)
            .ok_or_else(|| invalid(schema_root, format!("missing GameObject group {group}")))?
            .kind;
        let shape = schema.shape(kind, schema_root)?;
        for (name, kind) in shape
            .children
            .iter()
            .map(|f| (&f.name, &f.kind))
            .collect::<BTreeSet<_>>()
        {
            let coverage = match (group, name.as_str()) {
                ("Draws", "ScriptedModelDraw") => "partial",
                _ if supported.contains(&name.as_str()) => "encoder-present",
                _ => "missing",
            };
            writeln!(out, "{group}\t{name}\t{kind}\t{coverage}")?;
            if group == "Draws" && name == "ScriptedModelDraw" {
                let draw = schema.shape(kind, schema_root)?;
                for field in draw.fields {
                    let coverage = if matches!(field.name.as_str(), "id" | "ExtraPublicBone")
                        || SCRIPTED_FLAGS
                            .iter()
                            .any(|&(name, _, _)| name == field.name)
                    {
                        "encoded"
                    } else {
                        "missing"
                    };
                    writeln!(
                        out,
                        "DrawAttribute\t{}\t{}\t{coverage}",
                        field.name, field.kind
                    )?;
                }
                for field in draw.children {
                    let coverage = if matches!(
                        field.name.as_str(),
                        "ModelConditionState" | "AnimationState"
                    ) {
                        "partial"
                    } else {
                        "missing"
                    };
                    writeln!(out, "DrawChild\t{}\t{}\t{coverage}", field.name, field.kind)?;
                }
            }
        }
    }
    Ok(())
}

/// Compares a self-contained XML batch against an unlinked EA SDK stream.
/// This exercises native encoders directly, with each asset compared by ID.
pub fn verify_ea_batch(
    input: &Path,
    oracle: &Path,
    assets: &Path,
    mut out: impl Write,
) -> io::Result<()> {
    let xml = fs::read_to_string(input)?;
    let document = Document::parse(&xml).map_err(|error| invalid(input, error))?;
    let root = document.root_element();
    if !root.has_tag_name((SAGE_NS, "AssetDeclaration")) {
        return Err(invalid(input, "expected AssetDeclaration"));
    }
    let bytes = fs::read(oracle)?;
    let manifest = Manifest::parse(&bytes).map_err(|error| invalid(oracle, error))?;
    if manifest.is_linked || manifest.version != 6 {
        return Err(invalid(oracle, "expected a loose RA3 SDK manifest"));
    }
    let mut expected = manifest
        .assets
        .iter()
        .map(|asset| ((asset.id.type_id, asset.id.instance_id), asset))
        .collect::<BTreeMap<_, _>>();
    let mut count = 0;
    let mut failed = 0;
    for node in root.children().filter(roxmltree::Node::is_element) {
        let (name, id) = static_assets::declaration_id(node, input)?;
        let label = format!("{}:{name}", node.tag_name().name());
        let reference = expected
            .remove(&(id.type_id, id.instance_id))
            .ok_or_else(|| invalid(oracle, format!("EA output missing {label}")))?;
        let file = assets.join(format!(
            "{:08x}.{:08x}.{:08x}.{:08x}.asset",
            id.type_id, reference.type_hash, id.instance_id, reference.instance_hash
        ));
        let result = static_assets::compile_asset(node, input, "").and_then(|actual| {
            let bytes = fs::read(&file)?;
            let sizes = reference.chunk_sizes.as_array();
            let header = [
                id.type_id,
                id.instance_id,
                reference.type_hash,
                reference.instance_hash,
                sizes[0],
                sizes[1],
                sizes[2],
            ];
            let (words, _) = bytes.as_chunks::<4>();
            if words.len() < 8
                || !words
                    .iter()
                    .zip(header)
                    .all(|(&word, value)| manifest.byte_order.u32(word) == value)
            {
                return Err(invalid(&file, "asset header differs from manifest"));
            }
            let mut data = &bytes[32..];
            for (name, rust, length) in [
                ("payload", actual.payload, sizes[0]),
                ("relocations", actual.relocations, sizes[1]),
                ("imports", actual.imports, sizes[2]),
            ] {
                let (ea, rest) = data
                    .split_at_checked(length as usize)
                    .ok_or_else(|| invalid(&file, format!("truncated {name}")))?;
                if rust != ea {
                    let offset = rust
                        .iter()
                        .zip(ea)
                        .position(|(left, right)| left != right)
                        .unwrap_or(rust.len().min(ea.len()));
                    return Err(invalid(
                        input,
                        format!(
                            "{name} differs at byte {offset:#x} (Rust {}, EA {})",
                            rust.len(),
                            ea.len()
                        ),
                    ));
                }
                data = rest;
            }
            if !data.is_empty() {
                return Err(invalid(&file, "trailing asset bytes"));
            }
            if actual.type_hash != reference.type_hash
                || actual.instance_hash != reference.instance_hash
                || Some(u32::from(actual.tokenized)) != reference.tokenized
            {
                return Err(invalid(
                    input,
                    format!(
                        "asset metadata differs (Rust instance {:08x}, EA {:08x})",
                        actual.instance_hash, reference.instance_hash
                    ),
                ));
            }
            if !actual
                .references
                .iter()
                .map(|item| item.id)
                .eq(reference.references.iter())
            {
                return Err(invalid(input, "asset references differ"));
            }
            Ok(())
        });
        count += 1;
        match result {
            Ok(()) => writeln!(out, "PASS {label}")?,
            Err(error) => {
                failed += 1;
                writeln!(out, "FAIL {label} {error}")?;
            }
        }
    }
    if count == 0 || failed != 0 || !expected.is_empty() {
        return Err(invalid(
            input,
            format!(
                "batch compared {count} assets, {failed} failed, {} unexpected EA assets",
                expected.len()
            ),
        ));
    }
    writeln!(out, "Matched {count} EA assets")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_flag_layout_matches_sdk_schema_and_ea_batch() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let schema_root = root.join("schemas/ra3/xsd");
        let schema = Schema::load(&schema_root).unwrap();
        let draw = schema
            .shape("W3DScriptedModelDrawModuleData", &schema_root)
            .unwrap();
        let sdk_flags: BTreeSet<_> = draw
            .fields
            .iter()
            .filter(|field| field.kind == "SageBool")
            .map(|field| (field.name.as_str(), field.default.as_deref()))
            .collect();
        let rust_flags = SCRIPTED_FLAGS
            .iter()
            .map(|&(name, _, default)| (name, Some(if default { "true" } else { "false" })))
            .collect();
        assert_eq!(sdk_flags, rust_flags);

        assert_ea_fixture("scripted-model-draw", 2);
    }

    #[test]
    fn production_exit_family_matches_ea_batch() {
        assert_ea_fixture("production-exit", 2);
    }

    #[test]
    fn geometry_shape_modes_and_defaults_match_ea_batch() {
        assert_ea_fixture("geometry-shapes", 2);
    }

    #[test]
    fn weapon_categories_match_ea_batch() {
        assert_ea_fixture("weapon-category", 7);
    }

    #[test]
    fn queue_and_placement_types_match_ea_batch() {
        assert_ea_fixture("queue-placement", 9);
    }

    #[test]
    fn game_dependency_prerequisites_match_ea_batch() {
        assert_ea_fixture("game-dependency", 2);
    }

    #[test]
    fn game_object_base_selection_matches_ea_batch() {
        assert_ea_fixture("base-selection", 3);
    }

    #[test]
    fn weapon_bone_lists_match_ea_batch() {
        assert_ea_fixture("weapon-bones", 1);
    }

    #[test]
    fn laser_draw_matches_ea_batch() {
        assert_ea_fixture("laser-draw", 1);
    }

    #[test]
    fn weapon_set_matches_ea_batch() {
        assert_ea_fixture("weapon-set", 1);
    }

    #[test]
    fn laser_state_matches_ea_batch() {
        assert_ea_fixture("laser-state", 1);
    }

    #[test]
    fn attack_assist_pair_matches_ea_batch() {
        assert_ea_fixture("attack-assist", 1);
    }

    #[test]
    fn projected_buildability_matches_ea_batch() {
        assert_ea_fixture("buildability", 1);
    }

    #[test]
    fn model_texture_replacements_match_ea_batch() {
        assert_ea_fixture("model-texture", 1);
    }

    #[test]
    fn tech_upgrade_matches_ea_batch() {
        assert_ea_fixture("tech-upgrade", 1);
    }

    #[test]
    fn command_upgrade_matches_ea_batch() {
        assert_ea_fixture("command-upgrade", 1);
    }

    #[test]
    fn production_update_info_matches_ea_batch() {
        assert_ea_fixture("production-info", 1);
    }

    #[test]
    fn special_power_matches_ea_batch() {
        assert_ea_fixture("special-power", 1);
    }

    #[test]
    fn replace_self_matches_ea_batch() {
        assert_ea_fixture("replace-self", 1);
    }

    #[test]
    fn object_limit_matches_ea_batch() {
        assert_ea_fixture("object-limit", 1);
    }

    #[test]
    fn slow_death_matches_ea_batch() {
        assert_ea_fixture("slow-death", 1);
    }

    #[test]
    fn substructure_matches_ea_batch() {
        assert_ea_fixture("substructure", 1);
    }

    #[test]
    fn geometry_contact_points_match_ea_batch() {
        assert_ea_fixture("geometry-contact-points", 1);
    }

    #[test]
    fn assign_slaves_target_matches_ea_batch() {
        assert_ea_fixture("assign-slaves-target", 1);
    }

    #[test]
    fn attribute_modifier_upgrade_matches_ea_batch() {
        assert_ea_fixture("attribute-modifier-upgrade", 1);
    }

    #[test]
    fn structure_unpack_sound_matches_ea_batch() {
        assert_ea_fixture("structure-unpack", 1);
    }

    #[test]
    fn just_built_time_matches_ea_batch() {
        assert_ea_fixture("game-object-just-built", 1);
    }

    #[test]
    fn energy_bonus_matches_ea_batch() {
        assert_ea_fixture("game-object-energy-bonus", 1);
    }

    #[test]
    fn model_condition_upgrade_matches_ea_batch() {
        assert_ea_fixture("model-condition-upgrade", 1);
    }

    #[test]
    fn locomotor_sets_match_ea_batch() {
        assert_ea_fixture("locomotor-sets", 1);
    }

    #[test]
    fn shockwave_resistance_matches_ea_batch() {
        assert_ea_fixture("game-object-shockwave", 1);
    }

    #[test]
    fn voice_priority_matches_ea_batch() {
        assert_ea_fixture("game-object-voice-priority", 1);
    }

    #[test]
    fn radar_priority_matches_ea_batch() {
        assert_ea_fixture("game-object-radar", 5);
    }

    #[test]
    fn editor_sorting_matches_ea_batch() {
        assert_ea_fixture("game-object-editor-sorting", 1);
    }

    #[test]
    fn empty_game_object_references_match_ea_batch() {
        assert_ea_fixture("game-object-empty-references", 1);
    }

    #[test]
    fn unit_categories_match_ea_batch() {
        assert_ea_fixture("game-object-unit-categories", 5);
    }

    #[test]
    fn slaved_update_matches_ea_batch() {
        assert_ea_fixture("slaved-update", 1);
    }

    #[test]
    fn passive_area_effect_matches_ea_batch() {
        assert_ea_fixture("passive-area-effect", 1);
    }

    #[test]
    fn attach_update_matches_ea_batch() {
        assert_ea_fixture("attach-update", 1);
    }

    #[test]
    fn physics_fields_match_ea_batch() {
        assert_ea_fixture("physics-fields", 1);
    }

    #[test]
    fn vision_fields_match_ea_batch() {
        assert_ea_fixture("vision-fields", 1);
    }

    #[test]
    fn scripted_draw_scalars_match_ea_batch() {
        assert_ea_fixture("scripted-draw-scalars", 1);
    }

    #[test]
    fn model_state_retain_subobjects_matches_ea_batch() {
        assert_ea_fixture("model-state-retain", 1);
    }

    #[test]
    fn construction_fields_match_ea_batch() {
        assert_ea_fixture("game-object-construction-fields", 1);
    }

    #[test]
    fn skirmish_resource_node_matches_ea_batch() {
        assert_ea_fixture("skirmish-resource", 1);
    }

    #[test]
    fn queue_dock_matches_ea_batch() {
        assert_ea_fixture("queue-dock", 1);
    }

    #[test]
    fn assisted_placement_matches_ea_batch() {
        assert_ea_fixture("assisted-placement", 1);
    }

    #[test]
    fn structure_placement_matches_ea_batch() {
        assert_ea_fixture("structure-placement", 1);
    }

    #[test]
    fn animation_sound_matches_ea_batch() {
        assert_ea_fixture("animation-sound", 1);
    }

    #[test]
    fn client_update_eva_matches_ea_batch() {
        assert_ea_fixture("client-update-eva", 1);
    }

    #[test]
    fn stored_objects_powers_match_ea_batch() {
        assert_ea_fixture("stored-objects-powers", 1);
    }

    #[test]
    fn teleport_stored_update_matches_ea_batch() {
        assert_ea_fixture("teleport-stored-update", 1);
    }

    #[test]
    fn monitor_power_matches_ea_batch() {
        assert_ea_fixture("monitor-power", 1);
    }

    #[test]
    fn model_state_turrets_match_ea_batch() {
        assert_ea_fixture("model-state-turrets", 1);
    }

    #[test]
    fn weapon_bone_slots_match_ea_batch() {
        assert_ea_fixture("weapon-bone-slots", 1);
    }

    #[test]
    fn weapon_slot_turrets_match_ea_batch() {
        assert_ea_fixture("weapon-slot-turrets", 1);
    }

    #[test]
    fn weapon_fire_ability_matches_ea_batch() {
        assert_ea_fixture("weapon-fire-ability", 1);
    }

    #[test]
    fn ocl_special_power_matches_ea_batch() {
        assert_ea_fixture("ocl-special-power", 1);
    }

    #[test]
    fn crusher_info_matches_ea_batch() {
        assert_ea_fixture("crusher-info", 1);
    }

    #[test]
    fn squish_collide_matches_ea_batch() {
        assert_ea_fixture("squish-collide", 1);
    }

    #[test]
    fn wall_builder_matches_ea_batch() {
        assert_ea_fixture("wall-builder", 1);
    }

    #[test]
    fn stream_draw_matches_ea_batch() {
        assert_ea_fixture("stream-draw", 1);
    }

    #[test]
    fn stream_state_matches_ea_batch() {
        assert_ea_fixture("stream-state", 1);
    }

    #[test]
    fn status_bits_upgrade_matches_ea_batch() {
        assert_ea_fixture("status-bits-upgrade", 1);
    }

    #[test]
    fn secondary_object_listener_matches_ea_batch() {
        assert_ea_fixture("secondary-object-listener", 1);
    }

    #[test]
    fn lift_object_matches_ea_batch() {
        assert_ea_fixture("lift-object", 1);
    }

    #[test]
    fn garrison_contain_matches_ea_batch() {
        assert_ea_fixture("garrison-contain", 1);
    }

    #[test]
    fn upgrade_template_matches_ea_batch() {
        assert_ea_fixture("upgrade-template", 2);
    }

    #[test]
    fn w3d_mesh_matches_ea_batch() {
        assert_ea_fixture("w3d-mesh", 1);
    }

    #[test]
    fn w3d_container_matches_ea_batch() {
        assert_ea_fixture("w3d-container", 1);
    }

    #[test]
    fn transport_slot_count_matches_ea_batch() {
        assert_ea_fixture("transport-slot-count", 1);
    }

    #[test]
    fn tracer_model_draw_matches_ea_batch() {
        assert_ea_fixture("tracer-model-draw", 1);
    }

    #[test]
    fn invisibility_behavior_matches_ea_batch() {
        assert_ea_fixture("invisibility-behavior", 1);
    }

    #[test]
    fn attribute_modifier_aura_matches_ea_batch() {
        assert_ea_fixture("attribute-modifier-aura", 1);
    }

    #[test]
    fn slaughter_contain_matches_ea_batch() {
        assert_ea_fixture("slaughter-contain", 1);
    }

    #[test]
    fn empty_object_sound_matches_ea_batch() {
        assert_ea_fixture("empty-object-sound", 1);
    }

    #[test]
    fn power_upgrade_matches_ea_batch() {
        assert_ea_fixture("power-upgrade", 1);
    }

    #[test]
    fn animation_fx_events_match_ea_batch() {
        assert_ea_fixture("animation-fx-events", 1);
    }

    #[test]
    fn game_object_command_points_match_ea_batch() {
        assert_ea_fixture("game-object-command-points", 1);
    }

    #[test]
    fn voice_under_fire_timeout_matches_ea_batch() {
        assert_ea_fixture("voice-under-fire", 1);
    }

    #[test]
    fn path_priority_matches_ea_batch() {
        assert_ea_fixture("path-priority", 1);
    }

    #[test]
    fn eva_damage_threshold_matches_ea_batch() {
        assert_ea_fixture("eva-damage-threshold", 2);
    }

    #[test]
    fn armor_set_conditions_match_ea_batch() {
        assert_ea_fixture("armor-set-conditions", 1);
    }

    #[test]
    fn dynamics_update_matches_ea_batch() {
        assert_ea_fixture("dynamics-update", 1);
    }

    #[test]
    fn auto_heal_matches_ea_batch() {
        assert_ea_fixture("auto-heal", 1);
    }

    fn assert_ea_fixture(name: &str, count: usize) {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ea-batches");
        let mut report = Vec::new();
        verify_ea_batch(
            &fixtures.join(format!("{name}.xml")),
            &fixtures.join(format!("{name}.manifest")),
            &fixtures.join(format!("{name}-assets")),
            &mut report,
        )
        .unwrap_or_else(|error| panic!("{error}\n{}", String::from_utf8_lossy(&report)));
        assert!(
            String::from_utf8(report)
                .unwrap()
                .ends_with(&format!("Matched {count} EA assets\n"))
        );
    }
}
