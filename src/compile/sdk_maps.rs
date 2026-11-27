use super::{compile_map, invalid, redirected_sdk_map_directory};
use crate::compile_batch::publish_map_stream;
use crate::manifest::{AssetEntry, AssetId};
use crate::sdk_references::prepare_references;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

/// Compiles direct child map.xml sources in a selected SDK map directory.
pub fn compile_sdk_maps(
    input: &Path,
    sdk_root: &Path,
    references: &Path,
    output: &Path,
) -> io::Result<usize> {
    compile_sdk_map_overrides(input, sdk_root, references, output, None, false)
}

/// Builds SDK maps, preparing reference catalogs when the first source is found.
/// In unbuilt-only mode, reports failures per map and continues with the rest.
pub fn compile_sdk_map_overrides(
    input: &Path,
    sdk_root: &Path,
    references: &Path,
    output: &Path,
    game_data: Option<&Path>,
    skip_precompiled: bool,
) -> io::Result<usize> {
    if !input.try_exists()? {
        return Ok(0);
    }
    let canonical_sdk = fs::canonicalize(sdk_root)?;
    let resolved = fs::canonicalize(input)?;
    let input = if resolved.starts_with(&canonical_sdk) {
        &resolved
    } else if redirected_sdk_map_directory(input, sdk_root, &canonical_sdk, &resolved).is_some() {
        // Keep the logical SDK path when handing each map to compile_map.
        input
    } else {
        return Err(invalid(&resolved, "map directory lies outside the SDK"));
    };
    let mut maps = fs::read_dir(input)?.collect::<io::Result<Vec<_>>>()?;
    maps.sort_by_key(|entry| entry.file_name());
    let mut compiled = 0;
    let mut catalogs_ready = game_data.is_none();
    for map in maps {
        if !map.file_type()?.is_dir() || map.file_name().eq_ignore_ascii_case("official") {
            continue;
        }
        let name = map.file_name();
        let path = map.path();
        let result = (|| {
            if skip_precompiled && has_precompiled_map(&path)? {
                println!("Skipping precompiled map {}", name.to_string_lossy());
                return Ok(false);
            }
            let source = match map_source(&path)? {
                Some(source) => source,
                None => return Ok(false),
            };
            if !catalogs_ready && let Some(game_data) = game_data {
                prepare_references(references, game_data)?;
                catalogs_ready = true;
            }
            println!(
                "Compiling map overrides with Rust {}",
                name.to_string_lossy()
            );
            publish_map_stream(&output.join(&name), || {
                compile_map(&source, sdk_root, references)
            })?;
            Ok(true)
        })();
        match result {
            Ok(built) => compiled += usize::from(built),
            Err(error) if skip_precompiled => {
                eprintln!(
                    "Error: Map '{}' failed to compile. {error}",
                    name.to_string_lossy()
                );
            }
            Err(error) => return Err(error),
        }
    }
    Ok(compiled)
}

pub(super) fn worldbuilder_dependencies(
    stock: &HashMap<AssetId, &AssetEntry<'_>>,
    local: &HashSet<AssetId>,
    external: &HashSet<AssetId>,
    references: &[Vec<AssetId>],
) -> HashSet<AssetId> {
    let mut needed = HashSet::new();
    let mut pending: Vec<_> = references.iter().flatten().copied().collect();
    while let Some(id) = pending.pop() {
        if !local.contains(&id)
            && !external.contains(&id)
            && let Some(asset) = stock.get(&id)
            && needed.insert(id)
        {
            pending.extend(asset.references.iter());
        }
    }
    needed
}

fn map_source(directory: &Path) -> io::Result<Option<std::path::PathBuf>> {
    let mut terrain = false;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if entry.file_name().eq_ignore_ascii_case("map.xml") {
            return Ok(Some(entry.path()));
        }
        terrain |= entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("map"));
    }
    if terrain {
        return Err(invalid(directory, "unbuilt map is missing map.xml"));
    }
    Ok(None)
}

fn has_precompiled_map(directory: &Path) -> io::Result<bool> {
    let mut streams = std::collections::HashMap::<String, u8>::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let stem = stem.to_ascii_lowercase();
        if !matches!(stem.as_str(), "map" | "map_mod") && !stem.starts_with("map.") {
            continue;
        }
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        let part = match extension.to_ascii_lowercase().as_str() {
            "bin" => 1,
            "imp" => 2,
            "manifest" => 4,
            "relo" => 8,
            _ => continue,
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        let parts = streams.entry(stem).or_default();
        *parts |= part;
        if *parts == 15 {
            return Ok(true);
        }
    }
    Ok(false)
}
