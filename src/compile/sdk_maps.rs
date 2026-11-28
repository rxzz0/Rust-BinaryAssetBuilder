use super::{
    RedirectedSdkDirectory, ReferenceMode, compile_map, compile_with_source_references, invalid,
    redirected_map_tree, redirected_sdk_map_directory,
};
use crate::compile_batch::publish_map_stream;
use crate::manifest::{AssetEntry, AssetId};
use crate::sdk_references::prepare_references;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

const DEFAULT_MAP_DIRECTORY: &str = "Misc/Data/Maps/Official";

pub(super) struct ConfiguredMapDirectory {
    input: PathBuf,
    pub(super) mod_relative: PathBuf,
    pub(super) redirects: Vec<RedirectedSdkDirectory>,
    external: bool,
}

impl ConfiguredMapDirectory {
    pub(super) fn external_root(&self) -> Option<&Path> {
        self.external.then_some(self.input.as_path())
    }
}

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
    mut game_data: Option<&Path>,
    skip_precompiled: bool,
) -> io::Result<usize> {
    if let Some(mod_relative) = default_map_mod(input, sdk_root, output)? {
        let directories = configured_map_directories(sdk_root, &mod_relative)?;
        if !directories.is_empty() {
            return directories.iter().try_fold(0, |total, directory| {
                build_maps(
                    &directory.input,
                    sdk_root,
                    references,
                    &directory.input,
                    &mut game_data,
                    skip_precompiled,
                    Some(directory),
                )
                .map(|count| total + count)
            });
        }
    }
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
    build_maps(
        input,
        sdk_root,
        references,
        output,
        &mut game_data,
        skip_precompiled,
        None,
    )
}

pub(super) fn relative_sdk_path(path: &Path, sdk: &Path) -> Option<PathBuf> {
    let mut parts = path.components();
    for parent in sdk.components() {
        let part = parts.next()?;
        if part != parent
            && !(cfg!(windows) && part.as_os_str().eq_ignore_ascii_case(parent.as_os_str()))
        {
            return None;
        }
    }
    Some(parts.collect())
}

fn default_map_mod(input: &Path, sdk: &Path, output: &Path) -> io::Result<Option<PathBuf>> {
    let input = std::path::absolute(input)?;
    let output = std::path::absolute(output)?;
    let sdk = std::path::absolute(sdk)?;
    if !relative_sdk_path(&output, &input).is_some_and(|relative| relative.as_os_str().is_empty()) {
        return Ok(None);
    }
    let Some(path) = relative_sdk_path(&input, &sdk) else {
        return Ok(None);
    };
    let parts: Vec<_> = path.iter().collect();
    let [mods, name, rest @ ..] = parts.as_slice() else {
        return Ok(None);
    };
    Ok((mods.eq_ignore_ascii_case("Mods")
        && matches!(path.components().nth(1), Some(Component::Normal(_)))
        && rest.len() == 4
        && rest
            .iter()
            .zip(DEFAULT_MAP_DIRECTORY.split('/'))
            .all(|(part, expected)| part.eq_ignore_ascii_case(expected)))
    .then(|| Path::new(mods).join(name)))
}

fn configured_map_directories(
    sdk: &Path,
    mod_relative: &Path,
) -> io::Result<Vec<ConfiguredMapDirectory>> {
    let mod_directory = std::path::absolute(sdk.join(mod_relative))?;
    let config = mod_directory.join("mapdir.toml");
    let text = match fs::read_to_string(&config) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(invalid(&config, error)),
    };
    let canonical_sdk = fs::canonicalize(sdk)?;
    let physical_mod = fs::canonicalize(&mod_directory)?;
    let mut directories = Vec::new();
    let mut seen = HashSet::new();
    for (index, (line, entry)) in text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .enumerate()
        .filter(|(_, entry)| !entry.is_empty())
        .enumerate()
    {
        let config_error = |reason| invalid(&config, format!("line {}: {reason}", line + 1));
        if index == 3 {
            return Err(config_error(
                "expected at most three map directories".to_owned(),
            ));
        }
        let path = Path::new(entry);
        let relative = !path.is_absolute();
        if relative
            && (!path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
                || entry.contains(':'))
        {
            return Err(config_error(format!(
                "expected a mod-relative or full path: {entry}"
            )));
        }
        let logical = mod_directory.join(path);
        let input = fs::canonicalize(&logical)
            .map_err(|error| config_error(format!("cannot open map directory {entry}: {error}")))?;
        if !input.is_dir() {
            return Err(config_error(format!("not a map directory: {entry}")));
        }
        let mut redirects = Vec::new();
        let mut external = false;
        if !input.starts_with(&canonical_sdk) {
            let mod_relative_entry = relative_sdk_path(&logical, &mod_directory);
            let redirected = mod_relative_entry.and_then(|entry| {
                redirected_map_tree(&mod_relative.join(entry), sdk, &canonical_sdk, &input)
            });
            if let Some(redirected) = redirected {
                redirects.push(redirected);
            } else if relative {
                return Err(config_error(format!(
                    "map directory escapes its linked tree: {entry}"
                )));
            } else if !input.starts_with(&physical_mod) {
                external = true;
            }
        }
        if !physical_mod.starts_with(&canonical_sdk)
            && !redirects
                .iter()
                .any(|directory| directory.physical == physical_mod)
        {
            redirects.push(RedirectedSdkDirectory {
                sdk_relative: mod_relative.to_path_buf(),
                physical: physical_mod.clone(),
            });
        }
        if seen.insert(input.clone()) {
            directories.push(ConfiguredMapDirectory {
                input,
                mod_relative: mod_relative.to_path_buf(),
                redirects,
                external,
            });
        }
    }
    Ok(directories)
}

fn build_maps(
    input: &Path,
    sdk_root: &Path,
    references: &Path,
    output: &Path,
    game_data: &mut Option<&Path>,
    skip_precompiled: bool,
    configured: Option<&ConfiguredMapDirectory>,
) -> io::Result<usize> {
    let mut maps = fs::read_dir(input)?.collect::<io::Result<Vec<_>>>()?;
    maps.sort_by_key(|entry| entry.file_name());
    let mut compiled = 0;
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
            if let Some(data) = *game_data {
                prepare_references(references, data)?;
                *game_data = None;
            }
            println!(
                "Compiling map overrides with Rust {}",
                name.to_string_lossy()
            );
            publish_map_stream(&output.join(&name), || match configured {
                Some(directory) => compile_with_source_references(
                    &source,
                    sdk_root,
                    Some(references),
                    ReferenceMode::Map,
                    None,
                    Some(directory),
                )
                .map(|(stream, _)| stream),
                None => compile_map(&source, sdk_root, references),
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

fn map_source(directory: &Path) -> io::Result<Option<PathBuf>> {
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
