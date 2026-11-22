use super::{compile_map, invalid};
use crate::compile_batch::publish_map_stream;
use crate::sdk_references::prepare_references;
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
    compile_sdk_map_overrides(input, sdk_root, references, output, None)
}

/// Builds SDK maps, preparing reference catalogs when the first source is found.
pub fn compile_sdk_map_overrides(
    input: &Path,
    sdk_root: &Path,
    references: &Path,
    output: &Path,
    game_data: Option<&Path>,
) -> io::Result<usize> {
    if !input.try_exists()? {
        return Ok(0);
    }
    let sdk_root = fs::canonicalize(sdk_root)?;
    let input = fs::canonicalize(input)?;
    if !input.starts_with(&sdk_root) {
        return Err(invalid(&input, "map directory lies outside the SDK"));
    }
    let mut maps = fs::read_dir(&input)?.collect::<io::Result<Vec<_>>>()?;
    maps.sort_by_key(|entry| entry.file_name());
    let mut compiled = 0;
    for map in maps {
        if !map.file_type()?.is_dir() || map.file_name().eq_ignore_ascii_case("official") {
            continue;
        }
        let source = map.path().join("map.xml");
        if !source.try_exists()? {
            continue;
        }
        if compiled == 0
            && let Some(game_data) = game_data
        {
            prepare_references(references, game_data)?;
        }
        println!(
            "Compiling map overrides with Rust {}",
            map.file_name().to_string_lossy()
        );
        publish_map_stream(&output.join(map.file_name()), || {
            compile_map(&source, &sdk_root, references)
        })?;
        compiled += 1;
    }
    Ok(compiled)
}
