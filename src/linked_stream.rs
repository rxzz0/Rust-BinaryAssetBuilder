//! Linking and structural checks for the files accompanying a linked manifest.

use crate::manifest::Manifest;
use std::fs::File;
use std::io::{self, Read};
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

    let mut lengths = [CHECKSUM_SIZE; 3];
    for asset in &manifest.assets {
        // A patch can omit the chunks of an unchanged asset by storing zero sizes.
        for (length, chunk_size) in lengths.iter_mut().zip(asset.chunk_sizes.as_array()) {
            *length += u64::from(chunk_size);
        }
    }

    for (extension, expected_length) in ["bin", "relo", "imp"].into_iter().zip(lengths) {
        let path = path.with_extension(extension);
        check_file(&path, manifest, expected_length).map_err(|error| {
            io::Error::new(error.kind(), format!("{} ({error})", path.display()))
        })?;
    }
    Ok(())
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
