//! Linking and structural checks for the files accompanying a linked manifest.

use crate::manifest::{AssetEntry, AssetId, Manifest};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

const CHECKSUM_SIZE: u64 = 4;
const ASSET_HEADER_SIZE: usize = 32;

#[derive(Debug)]
pub struct LinkedFiles {
    pub bin: Vec<u8>,
    pub relo: Vec<u8>,
    pub imp: Vec<u8>,
}

/// Links precompiled assets in manifest order. The loader receives each .asset
/// file name and returns its header and payload. Patch entries with zero-sized
/// chunks reuse data from the base stream and do not call the loader.
pub fn link_assets(
    manifest: &Manifest<'_>,
    mut load_asset: impl FnMut(&str) -> io::Result<Vec<u8>>,
) -> io::Result<LinkedFiles> {
    ensure_linked(manifest)?;
    let byte_order = manifest.byte_order;
    let checksum = byte_order.u32_bytes(manifest.stream_checksum);
    let mut outputs = [checksum.to_vec(), checksum.to_vec(), checksum.to_vec()];
    let has_base = manifest
        .referenced_manifests
        .iter()
        .any(|entry| entry.is_patch);

    for asset in &manifest.assets {
        let [instance, relocations, imports] = asset.chunk_sizes.as_array();
        if has_base && [instance, relocations, imports] == [0; 3] {
            continue;
        }

        let name = format!(
            "{:08x}.{:08x}.{:08x}.{:08x}.asset",
            asset.id.type_id, asset.type_hash, asset.id.instance_id, asset.instance_hash
        );
        let bytes = load_asset(&name)
            .map_err(|error| io::Error::new(error.kind(), format!("{name} ({error})")))?;
        let invalid =
            |reason| io::Error::new(io::ErrorKind::InvalidData, format!("{name} ({reason})"));
        let (header, mut payload) = bytes
            .split_first_chunk::<ASSET_HEADER_SIZE>()
            .ok_or_else(|| invalid("truncated asset header"))?;
        let expected = [
            asset.id.type_id,
            asset.id.instance_id,
            asset.type_hash,
            asset.instance_hash,
            instance,
            relocations,
            imports,
        ];
        let (words, _) = header.as_chunks::<4>();
        // The SDK's version 6 loose assets use the final word for metadata
        // that is outside the linked payload. Version 5 reserves it as zero.
        if (manifest.version == 5 && words[7] != [0; 4])
            || !words
                .iter()
                .zip(expected)
                .all(|(&word, value)| byte_order.u32(word) == value)
        {
            return Err(invalid("asset header differs from the manifest"));
        }

        for (output, length) in outputs.iter_mut().zip([instance, relocations, imports]) {
            let (chunk, rest) = payload
                .split_at_checked(length as usize)
                .ok_or_else(|| invalid("truncated asset payload"))?;
            output.extend_from_slice(chunk);
            payload = rest;
        }
        if !payload.is_empty() {
            return Err(invalid("trailing bytes after asset payload"));
        }
    }

    let [bin, relo, imp] = outputs;
    Ok(LinkedFiles { bin, relo, imp })
}

/// Checks the lengths and stored checksums of the linked .bin, .relo, and .imp files.
pub fn check_linked_files(path: &Path, manifest: &Manifest<'_>) -> io::Result<()> {
    ensure_linked(manifest)?;

    for (extension, expected_length) in ["bin", "relo", "imp"]
        .into_iter()
        .zip(linked_lengths(manifest))
    {
        let path = path.with_extension(extension);
        check_file(&path, manifest, expected_length).map_err(|error| {
            io::Error::new(error.kind(), format!("{} ({error})", path.display()))
        })?;
    }
    Ok(())
}

fn linked_lengths(manifest: &Manifest<'_>) -> [u64; 3] {
    let mut lengths = [CHECKSUM_SIZE; 3];
    for asset in &manifest.assets {
        // A patch can omit the chunks of an unchanged asset by storing zero sizes.
        for (length, chunk_size) in lengths.iter_mut().zip(asset.chunk_sizes.as_array()) {
            *length += u64::from(chunk_size);
        }
    }

    lengths
}

/// Compares two linked builds, attributing binary differences to individual assets.
pub fn compare_linked_streams(ea: &Path, rust: &Path, mut out: impl Write) -> io::Result<()> {
    let ea_bytes = fs::read(ea)?;
    let rust_bytes = fs::read(rust)?;
    let parse = |error| io::Error::new(io::ErrorKind::InvalidData, error);
    let ea_manifest = Manifest::parse(&ea_bytes).map_err(parse)?;
    let rust_manifest = Manifest::parse(&rust_bytes).map_err(parse)?;
    let ea_files = read_linked_chunks(ea, &ea_manifest)?;
    let rust_files = read_linked_chunks(rust, &rust_manifest)?;
    let ea_chunks = index_chunks(&ea_manifest, &ea_files);
    let rust_chunks = index_chunks(&rust_manifest, &rust_files);
    if ea_chunks.len() != ea_manifest.assets.len()
        || rust_chunks.len() != rust_manifest.assets.len()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "duplicate asset IDs in linked manifest",
        ));
    }
    let mut differences = 0;
    let mut check = |matches: bool, label: &str| -> io::Result<()> {
        if !matches {
            writeln!(out, "DIFF {label}")?;
            differences += 1;
        }
        Ok(())
    };
    check(
        (
            ea_manifest.version,
            ea_manifest.byte_order,
            ea_manifest.all_types_hash,
        ) == (
            rust_manifest.version,
            rust_manifest.byte_order,
            rust_manifest.all_types_hash,
        ),
        "stream format or all-types hash",
    )?;
    check(
        ea_manifest.stream_checksum == rust_manifest.stream_checksum,
        "stream checksum",
    )?;
    check(
        (
            ea_manifest.total_instance_data_size,
            ea_manifest.max_chunk_sizes,
        ) == (
            rust_manifest.total_instance_data_size,
            rust_manifest.max_chunk_sizes,
        ),
        "manifest chunk totals or maxima",
    )?;
    check(
        ea_manifest
            .referenced_manifests
            .iter()
            .map(|entry| (entry.path, entry.is_patch))
            .eq(rust_manifest
                .referenced_manifests
                .iter()
                .map(|entry| (entry.path, entry.is_patch))),
        "referenced manifests (path, kind or order)",
    )?;
    check(
        ea_manifest
            .assets
            .iter()
            .map(|asset| asset.id)
            .eq(rust_manifest.assets.iter().map(|asset| asset.id)),
        "asset IDs or order",
    )?;
    for asset in &ea_manifest.assets {
        let name = String::from_utf8_lossy(asset.name);
        let Some((other, actual)) = rust_chunks.get(&asset.id) else {
            check(false, &format!("{name} missing in Rust"))?;
            continue;
        };
        check(
            (
                asset.type_hash,
                asset.instance_hash,
                asset.tokenized,
                asset.name,
            ) == (
                other.type_hash,
                other.instance_hash,
                other.tokenized,
                other.name,
            ),
            &format!("{name} asset metadata"),
        )?;
        check(
            asset.source_file_name == other.source_file_name,
            &format!("{name} source path"),
        )?;
        check(
            asset.references.iter().eq(other.references.iter()),
            &format!("{name} ordered references"),
        )?;
        let (_, expected) = ea_chunks[&asset.id];
        for (index, chunk) in ["payload", "relocations", "imports"]
            .into_iter()
            .enumerate()
        {
            if expected[index] != actual[index] {
                let offset = expected[index]
                    .iter()
                    .zip(actual[index])
                    .position(|(left, right)| left != right)
                    .unwrap_or(expected[index].len().min(actual[index].len()));
                check(false, &format!("{name} {chunk} at byte {offset:#x}"))?;
            }
        }
    }
    for asset in &rust_manifest.assets {
        if !ea_chunks.contains_key(&asset.id) {
            check(
                false,
                &format!("{} missing in EA", String::from_utf8_lossy(asset.name)),
            )?;
        }
    }
    if differences != 0 {
        return Err(io::Error::other(format!(
            "{differences} linked stream differences"
        )));
    }
    writeln!(out, "Matched {} linked assets", ea_manifest.assets.len())
}

type AssetChunks<'a> = HashMap<AssetId, (&'a AssetEntry<'a>, [&'a [u8]; 3])>;

fn read_linked_chunks(path: &Path, manifest: &Manifest<'_>) -> io::Result<LinkedFiles> {
    ensure_linked(manifest)?;
    let mut files = [Vec::new(), Vec::new(), Vec::new()];
    for ((extension, expected_length), bytes) in ["bin", "relo", "imp"]
        .into_iter()
        .zip(linked_lengths(manifest))
        .zip(&mut files)
    {
        let path = path.with_extension(extension);
        *bytes = fs::read(&path)?;
        if bytes.len() as u64 != expected_length
            || bytes.get(..4) != Some(&manifest.byte_order.u32_bytes(manifest.stream_checksum))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} (linked length or checksum differs from manifest)",
                    path.display()
                ),
            ));
        }
    }
    let [bin, relo, imp] = files;
    Ok(LinkedFiles { bin, relo, imp })
}

fn index_chunks<'a>(manifest: &'a Manifest<'_>, files: &'a LinkedFiles) -> AssetChunks<'a> {
    let mut remaining = [&files.bin[4..], &files.relo[4..], &files.imp[4..]];
    manifest
        .assets
        .iter()
        .map(|asset| {
            let mut chunks = [&[][..]; 3];
            for ((chunk, rest), length) in chunks
                .iter_mut()
                .zip(&mut remaining)
                .zip(asset.chunk_sizes.as_array())
            {
                (*chunk, *rest) = rest.split_at(length as usize);
            }
            (asset.id, (asset, chunks))
        })
        .collect()
}

fn ensure_linked(manifest: &Manifest<'_>) -> io::Result<()> {
    if manifest.is_linked {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest describes an unlinked stream",
        ))
    }
}

fn check_file(path: &Path, manifest: &Manifest<'_>, expected_length: u64) -> io::Result<()> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length != expected_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected_length} bytes, found {length}"),
        ));
    }
    let mut checksum = [0; 4];
    file.read_exact(&mut checksum)?;
    let checksum = manifest.byte_order.u32(checksum);
    if checksum != manifest.stream_checksum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "expected checksum {:08x}, found {checksum:08x}",
                manifest.stream_checksum
            ),
        ));
    }
    Ok(())
}
