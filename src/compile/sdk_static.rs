use super::{CompiledStream, ReferenceMode, compile_with_source_references, invalid};
use crate::compile_batch::{
    append_versioned_stream, compile_streams, is_generated, publish, publish_prepared, xml_sources,
};
use crate::manifest::AssetId;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Compiles a static XML stream from SDK sources using the native compiler.
pub fn compile_sdk_static(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    let (input, relative) = sdk_input(input, sdk_root)?;
    let relative_directory = relative.parent().unwrap();
    let directory = output.join(relative_directory);
    let stem = stream_stem(&input)?;
    publish_prepared(
        &directory,
        || {
            let mut references = SourceReferences {
                output: directory.clone(),
                relative_directory: relative_directory.to_path_buf(),
                source_directory: input.parent().unwrap().to_path_buf(),
                sources: HashMap::from([(PathBuf::from(&stem), input.clone())]),
                active: HashSet::from([input.clone()]),
                exports: HashMap::new(),
                streams: Vec::new(),
            };
            println!("Compiling XML with Rust {}", input.display());
            let (main, _) = compile_with_source_references(
                &input,
                sdk_root,
                Some(output),
                ReferenceMode::SdkStatic,
                Some(&mut references),
            )?;
            let mut outputs = Vec::new();
            for (name, stream) in references.streams {
                append_versioned_stream(name.as_os_str(), stream, b"\r\n", &mut outputs);
            }
            append_versioned_stream(OsStr::new(&stem), main, b"\r\n", &mut outputs);
            Ok(outputs)
        },
        |path| {
            is_generated(path)
                && path.file_stem().is_some_and(|name| {
                    name.eq_ignore_ascii_case(&stem)
                        || stem == "mod"
                            && (name.eq_ignore_ascii_case("mod_l")
                                || name.eq_ignore_ascii_case("mod_m"))
                })
        },
    )
}

pub(super) struct SourceReferences {
    pub(super) output: PathBuf,
    pub(super) relative_directory: PathBuf,
    source_directory: PathBuf,
    sources: HashMap<PathBuf, PathBuf>,
    active: HashSet<PathBuf>,
    exports: HashMap<PathBuf, HashSet<AssetId>>,
    streams: Vec<(PathBuf, CompiledStream)>,
}

impl SourceReferences {
    pub(super) fn is_root(&self) -> bool {
        self.active.len() == 1
    }

    pub(super) fn is_local(&self, source: &Path) -> bool {
        source.starts_with(&self.source_directory)
    }

    pub(super) fn claim(&mut self, source: &Path) -> io::Result<PathBuf> {
        if self.active.contains(source) {
            return Err(invalid(source, "source reference cycle"));
        }
        let directory = source
            .parent()
            .unwrap()
            .strip_prefix(&self.source_directory)
            .unwrap_or(Path::new(""));
        let stem = directory.join(stream_stem(source)?);
        let stem = PathBuf::from(stem.as_os_str().to_ascii_lowercase());
        match self.sources.entry(stem.clone()) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(source.to_path_buf());
            }
            std::collections::hash_map::Entry::Occupied(slot) if slot.get() != source => {
                return Err(invalid(
                    source,
                    format!(
                        "reference stream {}.manifest also names {}",
                        stem.display(),
                        slot.get().display()
                    ),
                ));
            }
            _ => {}
        }
        Ok(stem)
    }

    pub(super) fn build(
        &mut self,
        source: &Path,
        stem: PathBuf,
        sdk_root: &Path,
        built_mods: &Path,
    ) -> io::Result<&HashSet<AssetId>> {
        if !self.exports.contains_key(source) {
            if self.active.len() >= 64 {
                return Err(invalid(
                    source,
                    "source references are nested more than 64 levels",
                ));
            }
            self.active.insert(source.to_path_buf());
            println!("Compiling referenced XML with Rust {}", source.display());
            let result = compile_with_source_references(
                source,
                sdk_root,
                Some(built_mods),
                ReferenceMode::SdkStatic,
                Some(self),
            );
            self.active.remove(source);
            let (stream, ids) = result?;
            self.streams.push((stem, stream));
            self.exports.insert(source.to_path_buf(), ids);
        }
        Ok(&self.exports[source])
    }
}

fn stream_stem(source: &Path) -> io::Result<String> {
    source
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|stem| stem.is_ascii())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| invalid(source, "expected an ASCII stream name"))
}

/// Compiles all map metadata streams in the SDK additionalmaps directory.
pub fn compile_sdk_global(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    let (input, relative) = sdk_input(input, sdk_root)?;
    let sources = xml_sources(&input, is_map_metadata)?;
    if sources.is_empty() {
        return Ok(());
    }
    let outputs = compile_streams(&sources, b" \r\n", |source| {
        super::compile(
            source,
            sdk_root,
            Some(output),
            super::ReferenceMode::Implicit,
        )
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
