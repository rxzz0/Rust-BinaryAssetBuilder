use super::{compile_map, invalid};
use crate::compile_batch::publish_map_stream;
use std::fs;
use std::io;
use std::path::Path;

/// Compiles map.xml sources in the SDK's official map directory.
pub fn compile_sdk_maps(
    input: &Path,
    sdk_root: &Path,
    references: &Path,
    output: &Path,
) -> io::Result<usize> {
    if !input.try_exists()? {
        return Ok(0);
    }
    let sdk_root = fs::canonicalize(sdk_root)?;
    let input = fs::canonicalize(input)?;
    if !input.starts_with(&sdk_root) {
        return Err(invalid(
            &input,
            "official map directory lies outside the SDK",
        ));
    }
    let mut maps = fs::read_dir(&input)?.collect::<io::Result<Vec<_>>>()?;
    maps.sort_by_key(|entry| entry.file_name());
    let mut compiled = 0;
    for map in maps {
        if !map.file_type()?.is_dir() {
            continue;
        }
        let source = map.path().join("map.xml");
        if !source.try_exists()? {
            continue;
        }
        println!(
            "Compiling official map with Rust {}",
            map.file_name().to_string_lossy()
        );
        publish_map_stream(&output.join(map.file_name()), || {
            compile_map(&source, &sdk_root, references)
        })?;
        compiled += 1;
    }
    Ok(compiled)
}
