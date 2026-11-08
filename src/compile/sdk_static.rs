use super::{CompiledAsset, RA3_ALL_TYPES_HASH, SAGE_NS, invalid, static_assets};
use crate::linked_stream::check_linked_files;
use crate::manifest::{AssetEntry, AssetId, ByteOrder, Manifest};
use roxmltree::Document;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf, absolute};
use std::process::Command;
use std::time::Instant;

/// Builds the complete SDK static stream through EA's compatibility backend,
/// then recompiles and checks the static assets supported by the Rust port.
pub fn compile_sdk_static(input: &Path, sdk_root: &Path, output: &Path) -> io::Result<()> {
    let root = fs::canonicalize(sdk_root)?;
    let canonical_input = fs::canonicalize(input)?;
    let relative = canonical_input
        .strip_prefix(&root)
        .map_err(|_| invalid(input, "static XML lies outside the SDK"))?;
    let mut parts = relative.iter();
    let (Some(mods), Some(mod_name), Some(data)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(invalid(input, "expected an SDK Mods/<mod>/Data XML path"));
    };
    if !mods.eq_ignore_ascii_case("mods") || !data.eq_ignore_ascii_case("data") {
        return Err(invalid(input, "expected an SDK Mods/<mod>/Data XML path"));
    }
    let mod_name = mod_name
        .to_str()
        .filter(|name| !name.contains(';'))
        .ok_or_else(|| invalid(input, "mod name is invalid for the SDK search path"))?;
    let sdk_root = absolute(sdk_root)?;
    let output = absolute(output)?;
    let manifest_path = output.join(relative).with_extension("manifest");
    let backend = sdk_root.join("tools").join("BinaryAssetBuilder.exe");

    println!("Building the complete static stream through the EA SDK compatibility backend");
    let start = Instant::now();
    let status = Command::new(&backend)
        .current_dir(&sdk_root)
        .arg(sdk_root.join(relative))
        .arg(path_option("/od:", &output))
        .arg(path_option("/iod:", &output))
        .args([
            "/csc:false",
            "/ls:true",
            "/osh:false",
            "/pc:true",
            "/res:false",
            "/slowclean:true",
            "/ss:true",
        ])
        .arg(format!(r"/art:.;.\Mods\{mod_name}\Art;.\Mods;.\Art"))
        .arg(format!(r"/audio:.;.\Mods\{mod_name}\Audio;.\Mods;.\Audio"))
        .arg(format!(r"/data:.;.\Mods\{mod_name}\Data;.\Mods;.\SageXml"))
        .status()
        .map_err(|error| invalid(&backend, error))?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "EA static compiler exited with {status}"
        )));
    }
    println!(
        "EA compatibility build took {:.2}s",
        start.elapsed().as_secs_f64()
    );

    let start = Instant::now();
    let bytes = fs::read(&manifest_path)?;
    let manifest = Manifest::parse(&bytes).map_err(|error| invalid(&manifest_path, error))?;
    if manifest.version != 6
        || manifest.byte_order != ByteOrder::Little
        || manifest.all_types_hash != RA3_ALL_TYPES_HASH
    {
        return Err(invalid(&manifest_path, "expected a linked RA3 PC stream"));
    }
    check_linked_files(&manifest_path, &manifest)?;
    let native = check_native_assets(&root, &manifest_path, &manifest)?;
    println!(
        "Rust recompiled and matched {native} assets. EA supplied the other {} assets.",
        manifest.assets.len() - native
    );
    println!(
        "Static stream verified in {:.2}s",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn path_option(prefix: &str, path: &Path) -> OsString {
    let mut arg = OsString::from(prefix);
    arg.push(path);
    arg
}

struct Candidate<'a> {
    asset: &'a AssetEntry<'a>,
    offsets: [u64; 3],
    type_name: &'a str,
    id: &'a str,
}

fn check_native_assets(
    root: &Path,
    manifest_path: &Path,
    manifest: &Manifest<'_>,
) -> io::Result<usize> {
    let mut by_source: HashMap<&[u8], Vec<Candidate<'_>>> = HashMap::new();
    let mut offsets = [4u64; 3];
    for asset in &manifest.assets {
        if asset.tokenized == Some(0)
            && let Ok(name) = std::str::from_utf8(asset.name)
            && let Some((type_name, id)) = name.split_once(':')
            && static_assets::supports(type_name)
        {
            by_source
                .entry(asset.source_file_name)
                .or_default()
                .push(Candidate {
                    asset,
                    offsets,
                    type_name,
                    id,
                });
        }
        for (offset, size) in offsets.iter_mut().zip(asset.chunk_sizes.as_array()) {
            *offset += u64::from(size);
        }
    }

    let mut streams = [
        File::open(manifest_path.with_extension("bin"))?,
        File::open(manifest_path.with_extension("relo"))?,
        File::open(manifest_path.with_extension("imp"))?,
    ];
    let local_ids: HashSet<_> = manifest.assets.iter().map(|asset| asset.id).collect();
    let mut native = 0;
    let mut buffer = Vec::new();
    for (source, candidates) in by_source {
        let Some(path) = source_path(root, source) else {
            continue;
        };
        let Ok(xml) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = Document::parse(&xml) else {
            continue;
        };
        let root_node = document.root_element();
        if !root_node.has_tag_name((SAGE_NS, "AssetDeclaration")) {
            continue;
        }
        for candidate in candidates {
            let mut nodes = root_node.children().filter(|node| {
                node.has_tag_name((SAGE_NS, candidate.type_name))
                    && node
                        .attribute("id")
                        .is_some_and(|id| id.eq_ignore_ascii_case(candidate.id))
            });
            let Some(node) = nodes.next() else { continue };
            if nodes.next().is_some() {
                continue;
            }
            let Ok(compiled) = static_assets::compile_asset(node, &path, "") else {
                continue;
            };
            if matches_metadata(&compiled, candidate.asset, &local_ids)
                && matches_chunks(&compiled, candidate.offsets, &mut streams, &mut buffer)?
            {
                native += 1;
            }
        }
    }
    Ok(native)
}

fn source_path(root: &Path, source: &[u8]) -> Option<PathBuf> {
    let relative = std::str::from_utf8(source).ok()?.strip_prefix("DATA:")?;
    let path = fs::canonicalize(root.join(relative.replace('\\', "/"))).ok()?;
    path.starts_with(root).then_some(path)
}

fn matches_metadata(
    compiled: &CompiledAsset,
    expected: &AssetEntry<'_>,
    local_ids: &HashSet<AssetId>,
) -> bool {
    compiled.id == expected.id
        && compiled.type_hash == expected.type_hash
        && compiled.instance_hash == expected.instance_hash
        && [
            compiled.payload.len(),
            compiled.relocations.len(),
            compiled.imports.len(),
        ] == expected.chunk_sizes.as_array().map(|size| size as usize)
        && compiled
            .references
            .iter()
            .map(|reference| reference.output_id(local_ids))
            .eq(expected.references.iter())
}

fn matches_chunks(
    compiled: &CompiledAsset,
    offsets: [u64; 3],
    streams: &mut [File; 3],
    buffer: &mut Vec<u8>,
) -> io::Result<bool> {
    let chunks = [&compiled.payload, &compiled.relocations, &compiled.imports];
    for ((file, offset), chunk) in streams.iter_mut().zip(offsets).zip(chunks) {
        if chunk.is_empty() {
            continue;
        }
        buffer.resize(chunk.len(), 0);
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buffer)?;
        if buffer != chunk {
            return Ok(false);
        }
    }
    Ok(true)
}
