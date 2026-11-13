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
    ("Draws", &["LaserDraw", "ScriptedModelDraw"]),
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
