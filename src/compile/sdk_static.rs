use super::{compile_static, invalid};
use crate::compile_batch::{compile_streams, is_generated, publish, xml_sources};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Compiles a static XML stream from SDK sources using the native compiler.
pub fn compile_sdk_static(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    let (input, relative) = sdk_input(input, sdk_root)?;
    let directory = relative
        .parent()
        .ok_or_else(|| invalid(&input, "expected a static XML file"))?;
    let stem = input
        .file_stem()
        .ok_or_else(|| invalid(&input, "expected a static XML file"))?;
    let outputs = compile_streams(std::slice::from_ref(&input), b"\r\n", |source| {
        compile_static(source, sdk_root, output)
    })?;
    publish(&output.join(directory), outputs, |path| {
        is_generated(path)
            && path.file_stem().is_some_and(|name| {
                name.eq_ignore_ascii_case(stem)
                    || stem.eq_ignore_ascii_case("mod")
                        && (name.eq_ignore_ascii_case("mod_l")
                            || name.eq_ignore_ascii_case("mod_m"))
            })
    })
}

/// Compiles all map metadata streams in the SDK additionalmaps directory.
pub fn compile_sdk_global(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    let (input, relative) = sdk_input(input, sdk_root)?;
    let sources = xml_sources(&input, is_map_metadata)?;
    if sources.is_empty() {
        return Ok(());
    }
    let outputs = compile_streams(&sources, b" \r\n", |source| {
        super::compile(source, sdk_root, Some(output), true)
    })?;
    publish(&output.join(relative), outputs, |path| {
        is_generated(path) && is_map_metadata(path)
    })
}

fn sdk_input(input: &Path, sdk_root: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let root = fs::canonicalize(sdk_root)?;
    let input = fs::canonicalize(input)?;
    let relative = input
        .strip_prefix(root)
        .map_err(|_| invalid(&input, "input lies outside the SDK"))?;
    let mut parts = relative.iter();
    let (Some(mods), Some(_), Some(data), Some(_)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid(&input, "expected an SDK Mods/<mod>/Data input"));
    };
    if !mods.eq_ignore_ascii_case("mods") || !data.eq_ignore_ascii_case("data") {
        return Err(invalid(&input, "expected an SDK Mods/<mod>/Data input"));
    }
    let relative = PathBuf::from(relative.as_os_str().to_ascii_lowercase());
    Ok((input, relative))
}

fn is_map_metadata(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.to_ascii_lowercase().starts_with("mapmetadata_"))
}
