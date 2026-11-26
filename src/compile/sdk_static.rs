use super::{
    CompiledStream, ReferenceMode, compile_with_source_references, invalid,
    redirected_sdk_directory, sdk_relative_path, string_hash_stream, string_hashes::StringHashes,
};
use crate::compile_batch::{
    Contents, append_versioned_stream, compile_streams, is_generated, publish, publish_prepared,
    stream_outputs, xml_sources,
};
use crate::manifest::AssetId;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Compiles a static XML stream from SDK sources using the native compiler.
pub fn compile_sdk_static(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    compile_sdk_static_impl(input, sdk_root, output, false)
}

/// Compiles a static stream and EA-compatible linked string-hash tables.
pub fn compile_sdk_static_with_string_hashes(
    input: &Path,
    sdk_root: &Path,
    output: &Path,
) -> io::Result<()> {
    compile_sdk_static_impl(input, sdk_root, output, true)
}

fn compile_sdk_static_impl(
    input: &Path,
    sdk_root: &Path,
    output: &Path,
    string_hashes: bool,
) -> io::Result<()> {
    let (input, relative) = sdk_input(input, sdk_root, None)?;
    let relative_directory = relative.parent().unwrap();
    let directory = output.join(relative_directory);
    let stem = stream_stem(&input)?;
    if string_hashes && !sdk_root.join("Schemas/xsd/CnC3Types.xsd").try_exists()? {
        return Err(invalid(sdk_root, "string-hash output requires SDK schemas"));
    }
    if string_hashes && stem == "stringhashes" {
        return Err(invalid(
            &input,
            "stringhashes is reserved for string-hash output",
        ));
    }
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
                string_hashes: string_hashes.then(StringHashes::default),
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
                stream_outputs(name.as_os_str(), name.as_os_str(), stream, &mut outputs);
            }
            // ModStudio's Step 6 writes the blank version marker for the main stream.
            append_versioned_stream(OsStr::new(&stem), main, b"\r\n", &mut outputs);
            if let Some(hashes) = references.string_hashes {
                let xml = hashes.xml();
                let path = fs::canonicalize(&directory)?.join("StringHashes.xml");
                let sdk = fs::canonicalize(sdk_root)?;
                let source = if let Ok(relative) = path.strip_prefix(&sdk) {
                    format!(
                        "DATA:{}",
                        relative.to_string_lossy().replace('\\', "/").to_lowercase()
                    )
                } else {
                    path.display().to_string()
                };
                let stream = string_hash_stream::encode(&xml, &source, &path)?;
                let name = OsStr::new("stringhashes");
                stream_outputs(name, name, stream, &mut outputs);
                outputs.push(("StringHashes.xml".into(), Contents::Bytes(xml.into_bytes())));
            }
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
    pub(super) string_hashes: Option<StringHashes>,
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
        if self.string_hashes.is_some() && stem == Path::new("stringhashes") {
            return Err(invalid(
                source,
                "stringhashes is reserved for string-hash output",
            ));
        }
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
    let (input, relative) = sdk_input(input, sdk_root, Some("AdditionalMaps"))?;
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

fn sdk_input(
    input: &Path,
    sdk_root: &Path,
    redirected_directory: Option<&str>,
) -> io::Result<(PathBuf, PathBuf)> {
    let root = fs::canonicalize(sdk_root)?;
    let resolved = fs::canonicalize(input)?;
    let redirect = redirected_directory
        .filter(|_| !resolved.starts_with(&root))
        .and_then(|directory| redirected_sdk_directory(input, sdk_root, &resolved, directory));
    let relative = sdk_relative_path(&resolved, &root, redirect.as_ref())
        .ok_or_else(|| invalid(&resolved, "input lies outside the SDK"))?;
    let mut parts = relative.iter();
    let (Some(mods), Some(_), Some(data), Some(_)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid(input, "expected an SDK Mods/<mod>/Data input"));
    };
    if !mods.eq_ignore_ascii_case("mods") || !data.eq_ignore_ascii_case("data") {
        return Err(invalid(input, "expected an SDK Mods/<mod>/Data input"));
    }
    let relative = PathBuf::from(relative.as_os_str().to_ascii_lowercase());
    // Keep the SDK spelling for batch entries so the compiler can recover their
    // logical names after canonicalizing each file.
    let source = if redirect.is_some() {
        input.to_path_buf()
    } else {
        resolved
    };
    Ok((source, relative))
}

fn is_map_metadata(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.to_ascii_lowercase().starts_with("mapmetadata_"))
}
