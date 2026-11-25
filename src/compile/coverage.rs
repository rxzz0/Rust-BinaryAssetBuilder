use super::static_assets::game_object::{SCRIPTED_FLAGS, Schema};
use super::{SAGE_NS, invalid, static_assets};
use crate::manifest::Manifest;
use roxmltree::Document;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

// Dispatch coverage, not a claim of complete field support. Draw field coverage
// comes from the same flag table used by the encoder.
const MODULES: &[(&str, &[&str])] = &[
    (
        "Draws",
        &[
            "DynamicsDraw",
            "LaserDraw",
            "PropDraw",
            "ScriptedModelDraw",
            "StreamDraw",
            "TankDraw",
            "TracerModelDraw",
            "TruckDraw",
        ],
    ),
    ("AI", &["AIUpdate", "JetAIUpdate"]),
    (
        "Body",
        &[
            "ActiveBody",
            "HighlanderBody",
            "ImmortalBody",
            "ManagedActiveBody",
            "ManagedImmortalBody",
            "ManagerBody",
            "PropBody",
        ],
    ),
    (
        "ClientBehaviors",
        &[
            "AnimationSoundClientBehavior",
            "ModelConditionAudioLoopClientBehavior",
            "ModelConditionSoundSelectorClientBehavior",
            "UpgradeSoundSelectorClientBehavior",
        ],
    ),
    (
        "Behaviors",
        &[
            "BridgeBuilder",
            "CaptureAndGiveUpgradeBehavior",
            "ToppleUpdate",
            "WeaponSetUpdate",
            "LaserState",
            "ConvergingLaserState",
            "SweepingLaserState",
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
            "GeometryUpgrade",
            "RemoveUpgradeUpgrade",
            "SlavedUpdate",
            "PassiveAreaEffectBehavior",
            "AttachUpdate",
            "QueueDockBehavior",
            "AssistedPlacementBehavior",
            "AssistedPlacementInfoBehavior",
            "StructurePlacementBehavior",
            "TeleportStoredObjectsSpecialPower",
            "StoreObjectsSpecialPower",
            "TeleportStoredObjectsUpdate",
            "MonitorSpecialPowerTimerUpdate",
            "WeaponFireSpecialAbilityUpdate",
            "EjectPassengersSpecialAbilityUpdate",
            "OCLSpecialPower",
            "SquishCollide",
            "EnableDynamicsCollide",
            "WallBuilderBehavior",
            "StreamState",
            "StatusBitsUpgrade",
            "SecondaryObjectListenerModule",
            "LiftObjectUpdate",
            "GrapplingHookUpdate",
            "GarrisonContain",
            "ContestableGarrisonContain",
            "OpenContain",
            "TransportContain",
            "InvisibilityBehavior",
            "AttributeModifierAuraUpdate",
            "SlaughterHordeContain",
            "PowerUpgrade",
            "DynamicsUpdate",
            "AutoHealBehavior",
            "LevelUpUpgrade",
            "RunOffMapBehavior",
            "ToggleStatusSpecialAbilityUpdate",
            "ChargeWeaponUpdate",
            "PrematureDischargeSpecialPower",
            "RadarBehavior",
            "FireWeaponUpdate",
            "PlayerPowerManager",
            "GiveProductionBonusSpecialPower",
            "PlayerUpgradeSpecialPower",
            "StoreTemplateIDBehavior",
            "BezierProjectile",
            "LineSegmentDamageUpdate",
            "LinearDamageUpdate",
            "LifetimeUpdate",
            "SuckUpObjectsUpdate",
            "SqueezeFlingUpdate",
            "GatherEjectUpdate",
            "AttributeModifierPoolUpdate",
            "BuildingBehavior",
            "BuildingDestructionBehavior",
            "CreateObjectDie",
            "DefaultProductionExit",
            "DestroyDie",
            "StealthDetectorUpdate",
            "EngineerContain",
            "FXListBehavior",
            "FireWeaponWhenDead",
            "FireWeaponCollide",
            "MonitorConditionUpdate",
            "OreMinerBehavior",
            "OreNodeBehavior",
            "AutoDepositUpdate",
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
            "ObjectCreationUpgrade",
            "SlaveWatcherBehavior",
            "AudioLoopUpgrade",
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
    let local_ids = manifest
        .assets
        .iter()
        .map(|asset| asset.id)
        .collect::<HashSet<_>>();
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
                .map(|item| item.output_id(&local_ids))
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
        assert_ea_fixture("laser-state-variants", 1);
    }

    #[test]
    fn upgrade_spawn_modules_match_ea() {
        assert_ea_fixture("upgrade-spawn-modules", 1);
    }

    #[test]
    fn death_weapon_probability_matches_ea() {
        assert_ea_fixture("death-weapon-probability", 1);
    }

    #[test]
    fn audio_loop_upgrade_matches_ea() {
        assert_ea_fixture("audio-loop-upgrade", 1);
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
    fn return_to_producer_special_power_matches_ea() {
        assert_ea_fixture("return-to-producer-power", 2);
    }

    #[test]
    fn die_on_producer_death_update_matches_ea() {
        assert_ea_fixture("producer-death-update", 2);
    }

    #[test]
    fn magnetic_effect_update_matches_ea() {
        assert_ea_fixture("magnetic-effect-update", 2);
    }

    #[test]
    fn movement_special_power_matches_ea() {
        assert_ea_fixture("movement-special-power", 2);
    }

    #[test]
    fn reconstitute_state_matches_ea() {
        assert_ea_fixture("reconstitute-state", 2);
    }

    #[test]
    fn switch_locomotor_matches_ea() {
        assert_ea_fixture("switch-locomotor", 2);
    }

    #[test]
    fn special_disguise_update_matches_ea() {
        assert_ea_fixture("special-disguise-update", 2);
    }

    #[test]
    fn store_current_state_update_matches_ea() {
        assert_ea_fixture("store-current-state-update", 2);
    }

    #[test]
    fn aim_weapon_matches_ea() {
        assert_ea_fixture("aim-weapon", 2);
    }

    #[test]
    fn lose_health_update_matches_ea() {
        assert_ea_fixture("lose-health-update", 2);
    }

    #[test]
    fn hit_reaction_matches_ea() {
        assert_ea_fixture("hit-reaction-behavior", 2);
    }

    #[test]
    fn model_state_skeleton_matches_ea() {
        assert_ea_fixture("model-state-skeleton", 1);
    }

    #[test]
    fn dominate_enemy_matches_ea() {
        assert_ea_fixture("dominate-enemy-special-power", 2);
    }

    #[test]
    fn ocl_monitor_update_matches_ea() {
        assert_ea_fixture("ocl-monitor-update", 2);
    }

    #[test]
    fn sub_structure_storage_matches_ea() {
        assert_ea_fixture("sub-structure-storage-behavior", 1);
    }

    #[test]
    fn strafe_area_target_chooser_matches_ea() {
        assert_ea_fixture("strafe-area-target-chooser", 3);
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
    fn upgrade_sound_selector_matches_ea_batch() {
        assert_ea_fixture("upgrade-sound-selector", 1);
    }

    #[test]
    fn transport_contain_matches_ea_batch() {
        assert_ea_fixture("transport-contain", 1);
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
        assert_ea_fixture("mesh-layout", 1);
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

    #[test]
    fn level_upgrade_matches_ea_batch() {
        assert_ea_fixture("level-upgrade", 1);
    }

    #[test]
    fn run_off_map_matches_ea_batch() {
        assert_ea_fixture("run-off-map", 1);
    }

    #[test]
    fn subgroup_priority_matches_ea_batch() {
        assert_ea_fixture("subgroup-priority", 1);
    }

    #[test]
    fn game_object_unit_intro_matches_ea_batch() {
        assert_ea_fixture("game-object-unit-intro", 1);
    }

    #[test]
    fn truck_draw_matches_ea_batch() {
        assert_ea_fixture("truck-draw", 1);
        assert_ea_fixture("scripted-draw-fields", 1);
    }

    #[test]
    fn named_voice_entries_match_ea() {
        assert_ea_fixture("voice-named-entry", 1);
    }

    #[test]
    fn target_vision_ranges_match_ea() {
        assert_ea_fixture("target-vision-range", 1);
    }

    #[test]
    fn toggle_status_ability_matches_ea() {
        assert_ea_fixture("toggle-status-ability", 1);
        assert_ea_fixture("toggle-ai-fields", 1);
    }

    #[test]
    fn charge_weapon_matches_ea() {
        assert_ea_fixture("charge-weapon-update", 1);
    }

    #[test]
    fn premature_discharge_matches_ea() {
        assert_ea_fixture("premature-discharge", 1);
    }

    #[test]
    fn radar_behavior_matches_ea() {
        assert_ea_fixture("radar-behavior", 1);
    }

    #[test]
    fn game_object_equivalents_match_ea() {
        assert_ea_fixture("game-object-equivalent", 1);
    }

    #[test]
    fn ambient_streams_match_ea() {
        assert_ea_fixture("ambient-stream", 2);
    }

    #[test]
    fn audio_event_matches_ea() {
        assert_ea_fixture("audio-event", 1);
    }

    #[test]
    fn prop_body_matches_ea() {
        assert_ea_fixture("prop-body", 1);
    }

    #[test]
    fn fire_weapon_update_matches_ea() {
        assert_ea_fixture("fire-weapon-update", 1);
    }

    #[test]
    fn on_demand_image_matches_ea() {
        assert_ea_fixture("on-demand-image", 1);
    }

    #[test]
    fn ai_directive_feedback_matches_ea() {
        assert_ea_fixture("ai-directive-feedback", 1);
    }

    #[test]
    fn ai_skirmish_feedback_matches_ea() {
        assert_ea_fixture("ai-skirmish-feedback", 1);
    }

    #[test]
    fn ai_in_trouble_feedback_matches_ea() {
        assert_ea_fixture("ai-in-trouble-feedback", 1);
    }

    #[test]
    fn ai_unit_modifiers_match_ea() {
        assert_ea_fixture("ai-unit-modifiers", 1);
    }

    #[test]
    fn ai_opening_choice_matches_ea() {
        assert_ea_fixture("ai-opening-choice", 1);
    }

    #[test]
    fn ai_feedback_entry_matches_ea() {
        assert_ea_fixture("ai-feedback-entry", 1);
    }

    #[test]
    fn damage_fx_matches_ea() {
        assert_ea_fixture("damage-fx", 1);
    }

    #[test]
    fn player_power_manager_matches_ea() {
        assert_ea_fixture("player-power-manager", 1);
    }

    #[test]
    fn production_bonus_power_matches_ea() {
        assert_ea_fixture("production-bonus-power", 1);
    }

    #[test]
    fn player_upgrade_power_matches_ea() {
        assert_ea_fixture("player-upgrade-power", 1);
    }

    #[test]
    fn store_template_id_matches_ea() {
        assert_ea_fixture("store-template-id", 1);
    }

    #[test]
    fn bezier_projectile_matches_ea() {
        assert_ea_fixture("bezier-projectile", 1);
    }

    #[test]
    fn skirmish_dodge_matches_ea() {
        assert_ea_fixture("skirmish-dodge", 1);
    }

    #[test]
    fn line_segment_damage_matches_ea() {
        assert_ea_fixture("line-segment-damage", 1);
    }

    #[test]
    fn lifetime_update_matches_ea() {
        assert_ea_fixture("lifetime-update", 1);
    }

    #[test]
    fn suck_up_objects_matches_ea() {
        assert_ea_fixture("suck-up-objects", 1);
    }

    #[test]
    fn squeeze_fling_matches_ea() {
        assert_ea_fixture("squeeze-fling", 1);
    }

    #[test]
    fn gather_eject_matches_ea() {
        assert_ea_fixture("gather-eject", 1);
    }

    #[test]
    fn object_filter_asset_matches_ea() {
        assert_ea_fixture("object-filter-asset", 1);
    }

    #[test]
    fn player_tech_matches_ea() {
        assert_ea_fixture("player-tech", 1);
    }

    #[test]
    fn purchase_player_tech_button_matches_ea() {
        assert_ea_fixture("purchase-player-tech-button", 2);
    }

    #[test]
    fn player_tech_store_matches_ea() {
        assert_ea_fixture("player-tech-store", 1);
    }

    #[test]
    fn special_power_layout_matches_ea() {
        assert_ea_fixture("special-power-layout-probe", 2);
    }

    #[test]
    fn weapon_root_matches_ea() {
        assert_ea_fixture("weapon-template-root", 2);
    }

    #[test]
    fn weapon_timing_matches_ea() {
        assert_ea_fixture("weapon-timing", 1);
    }

    #[test]
    fn weapon_fire_on_objects_matches_ea() {
        assert_ea_fixture("weapon-fire-on-objects", 1);
    }

    #[test]
    fn weapon_tint_objects_matches_ea() {
        assert_ea_fixture("weapon-tint-objects", 1);
    }

    #[test]
    fn tank_draw_matches_ea_batch() {
        assert_ea_fixture("tank-draw", 1);
    }

    #[test]
    fn radius_cursor_matches_ea_batch() {
        assert_ea_fixture("radius-cursor", 2);
    }

    #[test]
    fn experience_level_matches_ea_batch() {
        assert_ea_fixture("experience-level", 4);
    }

    #[test]
    fn unit_ability_button_templates_match_ea_batch() {
        assert_ea_fixture("unit-ability-buttons", 12);
    }

    #[test]
    fn button_single_state_matches_ea_batch() {
        assert_ea_fixture("button-state", 3);
    }

    #[test]
    fn single_sound_fields_match_ea_batch() {
        assert_ea_fixture("single-sound-fields", 6);
    }

    #[test]
    fn particle_template_matches_ea_batches() {
        assert_ea_fixture("particle-template", 3);
        assert_ea_fixture("particle-emission", 9);
        assert_ea_fixture("particle-effects", 18);
    }

    #[test]
    fn multisound_matches_ea_batch() {
        assert_ea_fixture("multisound", 3);
    }

    #[test]
    fn object_specific_voice_matches_ea_batch() {
        assert_ea_fixture("object-specific-voice", 1);
    }

    #[test]
    fn fx_nugget_variants_match_ea_batch() {
        assert_ea_fixture("fx-nugget-variants", 2);
    }

    #[test]
    fn fx_list_matches_ea_batch() {
        assert_ea_fixture("fx-list", 3);
    }

    #[test]
    fn weapon_status_effects_match_ea_batch() {
        assert_ea_fixture("status-effects", 2);
    }

    #[test]
    fn projectile_variants_match_ea_batch() {
        assert_ea_fixture("projectile-variants", 1);
    }

    #[test]
    fn damage_tracking_matches_ea_batch() {
        assert_ea_fixture("damage-tracking", 1);
    }

    #[test]
    fn ability_opacity_updates_match_ea_batch() {
        assert_ea_fixture("ability-opacity-updates", 1);
    }

    #[test]
    fn module_activation_matches_ea_batch() {
        assert_ea_fixture("module-activation", 1);
    }

    #[test]
    fn draw_particle_bools_match_ea_batch() {
        assert_ea_fixture("draw-particle-bools", 1);
    }

    #[test]
    fn draw_particle_actions_match_ea() {
        assert_ea_fixture("particle-actions", 1);
    }

    #[test]
    fn simple_weapon_effects_match_ea_batch() {
        assert_ea_fixture("simple-weapon-effects", 2);
    }

    #[test]
    fn weapon_activation_nuggets_match_ea_batch() {
        assert_ea_fixture("weapon-activation-nuggets", 1);
    }

    #[test]
    fn weapon_ai_hints_match_ea_batch() {
        assert_ea_fixture("weapon-ai-hints", 2);
    }

    #[test]
    fn line_damage_nugget_matches_ea_batch() {
        assert_ea_fixture("line-damage-nugget", 1);
    }

    #[test]
    fn damage_and_reference_weapon_nuggets_match_ea() {
        for name in ["weapon-dot", "weapon-projectile", "weapon-ocl"] {
            assert_ea_fixture(name, 1);
        }
    }

    #[test]
    fn weapon_damage_matches_ea() {
        assert_ea_fixture("weapon-damage", 1);
    }

    #[test]
    fn weapon_damage_subtypes_match_ea() {
        assert_ea_fixture("damage-subtypes", 1);
    }

    #[test]
    fn weapon_damage_references_match_ea() {
        assert_ea_fixture("damage-reference-fields", 3);
    }

    #[test]
    fn weapon_damage_invalid_target_status_matches_ea() {
        assert_ea_fixture("damage-invalid-target-status", 4);
    }

    #[test]
    fn weapon_contained_matches_ea() {
        assert_ea_fixture("weapon-contained", 1);
    }

    #[test]
    fn weapon_damage_scalars_match_ea() {
        assert_ea_fixture("weapon-damage-scalars", 1);
    }

    #[test]
    fn logic_command_matches_ea() {
        assert_ea_fixture("logic-command", 1);
        assert_ea_fixture("logic-command-ai", 3);
    }

    #[test]
    fn player_template_matches_ea() {
        assert_ea_fixture("player-template", 3);
    }

    #[test]
    fn attribute_modifier_matches_ea() {
        assert_ea_fixture("attribute-modifier", 2);
    }

    #[test]
    fn game_object_unit_icon_matches_ea() {
        assert_ea_fixture("game-object-unit-icon", 4);
    }

    #[test]
    fn crush_die_matches_ea() {
        assert_ea_fixture("crush-die", 5);
    }

    #[test]
    fn suppression_and_freefall_match_ea() {
        assert_ea_fixture("suppression-freefall", 4);
    }

    #[test]
    fn emotions_match_ea() {
        assert_ea_fixture("emotion-tracker", 5);
    }

    #[test]
    fn capture_special_ability_matches_ea() {
        assert_ea_fixture("capture-special-ability", 3);
    }

    #[test]
    fn capture_and_give_upgrade_behavior_matches_ea() {
        assert_ea_fixture("capture-and-give-upgrade", 4);
    }

    #[test]
    fn large_group_audio_matches_ea() {
        assert_ea_fixture("large-group-audio", 2);
    }

    #[test]
    fn object_creation_actions_match_ea() {
        assert_ea_fixture("object-creation-list", 7);
        assert_ea_fixture("object-creation-actions", 2);
    }

    #[test]
    fn skirmish_ai_fields_match_ea() {
        assert_ea_fixture("skirmish-fields", 1);
    }

    #[test]
    fn stealth_detectors_match_ea() {
        assert_ea_fixture("stealth-detector", 5);
    }

    #[test]
    fn weapon_attributes_match_ea() {
        assert_ea_fixture("weapon-fields", 3);
    }

    #[test]
    fn weapon_nugget_conditions_match_ea() {
        assert_ea_fixture("weapon-nugget-filters", 5);
    }

    #[test]
    fn w3d_animation_compression_matches_ea() {
        for (fixture, count) in [
            ("animation-runtime", 3),
            ("timecoded", 3),
            ("timecoded-details", 2),
            ("animation-delta", 4),
            ("delta-thresholds", 27),
            ("delta-special", 4),
        ] {
            assert_ea_fixture(fixture, count);
        }
    }

    #[test]
    fn weapon_scatter_radii_match_ea() {
        assert_ea_fixture("weapon-scatter-radii", 2);
    }

    #[test]
    fn meta_impact_nugget_matches_ea() {
        assert_ea_fixture("weapon-meta-impact", 2);
    }

    #[test]
    fn remaining_weapon_nuggets_match_ea() {
        assert_ea_fixture("weapon-damage-variants", 3);
        assert_ea_fixture("weapon-misc-complete", 10);
    }

    #[test]
    fn damage_scalar_filters_match_ea() {
        assert_ea_fixture("weapon-damage-scalar-filters", 2);
    }

    #[test]
    fn dynamics_draw_matches_ea() {
        assert_ea_fixture("dynamics-draw", 3);
    }

    #[test]
    fn weapon_root_options_match_ea() {
        assert_ea_fixture("weapon-root-options", 4);
    }

    #[test]
    fn eva_events_and_settings_match_ea() {
        assert_ea_fixture("eva-events-and-settings", 4);
    }

    #[test]
    fn locomotor_templates_and_bounce_maps_match_ea() {
        assert_ea_fixture("locomotor-templates", 5);
    }

    #[test]
    fn transformed_game_object_labels_match_ea() {
        assert_ea_fixture("transformed-display", 2);
    }

    #[test]
    fn terrain_particles_and_dynamic_contacts_match_ea() {
        assert_ea_fixture("terrain-contact-mappings", 6);
    }

    #[test]
    fn animal_ai_matches_ea() {
        assert_ea_fixture("animal-ai", 2);
    }

    #[test]
    fn strafe_area_update_matches_ea() {
        assert_ea_fixture("strafe-area-update", 4);
    }

    #[test]
    fn destroy_objects_matches_ea() {
        assert_ea_fixture("destroy-objects-update", 2);
    }

    #[test]
    fn projectile_update_matches_ea() {
        assert_ea_fixture("projectile-update", 7);
    }

    #[test]
    fn invalid_targets_death_update_matches_ea() {
        assert_ea_fixture("invalid-targets-death-update", 2);
    }

    #[test]
    fn parabolic_curve_update_matches_ea() {
        assert_ea_fixture("parabolic-curve-update", 1);
    }

    #[test]
    fn selective_visibility_matches_ea() {
        assert_ea_fixture("selective-visibility", 2);
    }

    #[test]
    fn grant_upgrade_create_matches_ea() {
        assert_ea_fixture("grant-upgrade-create", 3);
    }

    #[test]
    fn bombard_area_update_matches_ea() {
        assert_ea_fixture("bombard-area-update", 3);
    }

    #[test]
    fn laser_scatter_matches_ea() {
        assert_ea_fixture("laser-scatter", 2);
    }

    #[test]
    fn weapon_scatter_effect_matches_ea() {
        assert_ea_fixture("weapon-scatter-effect-update", 3);
    }

    #[test]
    fn set_locomotor_speed_matches_ea() {
        assert_ea_fixture("set-locomotor-speed", 2);
    }

    #[test]
    fn invisibility_broadcast_matches_ea() {
        assert_ea_fixture("invisibility-broadcast", 3);
    }

    #[test]
    fn shield_sphere_update_matches_ea() {
        assert_ea_fixture("shield-sphere-update", 2);
    }

    #[test]
    fn height_die_update_matches_ea() {
        assert_ea_fixture("height-die-update", 4);
    }

    #[test]
    fn return_to_dock_special_power_matches_ea() {
        assert_ea_fixture("return-to-dock", 2);
    }

    #[test]
    fn crate_collide_matches_ea() {
        assert_ea_fixture("crate-collide", 5);
    }

    #[test]
    fn unit_vision_behavior_matches_ea() {
        assert_ea_fixture("unit-vision-behavior", 3);
    }

    #[test]
    fn energy_disabling_behavior_matches_ea() {
        assert_ea_fixture("energy-disabling-behavior", 1);
    }

    #[test]
    fn circular_damage_update_matches_ea() {
        assert_ea_fixture("circular-damage", 4);
    }

    #[test]
    fn linear_damage_updates_match_ea() {
        assert_ea_fixture("linear-damage", 4);
    }

    #[test]
    fn transform_upgrades_match_ea() {
        assert_ea_fixture("transform-upgrades", 3);
    }

    #[test]
    fn animation_sound_frame_and_reference_variants_match_ea() {
        assert_ea_fixture("animation-sound-edge", 1);
    }

    #[test]
    fn fire_weapon_collide_matches_ea() {
        assert_ea_fixture("collision-weapon", 3);
    }

    #[test]
    fn ore_collection_and_deposit_match_ea() {
        assert_ea_fixture("ore-behaviors", 9);
        assert_ea_fixture("monitor-condition", 3);
    }

    #[test]
    fn automatic_resource_deposits_match_ea() {
        assert_ea_fixture("auto-deposit", 2);
    }

    #[test]
    fn jet_ai_updates_match_ea() {
        assert_ea_fixture("jet-ai-update", 5);
    }

    #[test]
    fn cleared_eva_event_references_match_ea() {
        assert_ea_fixture("eva-entry-clear", 2);
    }

    #[test]
    fn enable_dynamics_collide_matches_ea() {
        assert_ea_fixture("enable-dynamics-collide", 2);
    }

    #[test]
    fn game_maps_match_ea() {
        assert_ea_fixture("game-map", 5);
    }

    #[test]
    fn map_override_types_match_ea() {
        assert_ea_fixture("map-encoder-types", 3);
    }

    #[test]
    fn roads_match_ea() {
        assert_ea_fixture("road", 2);
    }

    #[test]
    fn game_script_lists_match_ea() {
        assert_ea_fixture("game-script-list", 6);
    }

    #[test]
    fn stance_templates_match_ea() {
        assert_ea_fixture("stance-template", 5);
    }

    #[test]
    fn eject_passengers_matches_ea() {
        assert_ea_fixture("eject-passengers", 5);
    }

    #[test]
    fn assisted_placement_info_matches_ea() {
        assert_ea_fixture("assisted-placement-info", 2);
    }

    #[test]
    fn damage_arcs_match_ea() {
        assert_ea_fixture("damage-arc", 3);
    }

    #[test]
    fn joypad_buttons_match_ea() {
        assert_ea_fixture("joypad-buttons", 38);
        assert_ea_fixture("edge-joypad", 3);
    }

    #[test]
    fn player_power_buttons_match_ea() {
        assert_ea_fixture("player-power-buttons", 5);
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
