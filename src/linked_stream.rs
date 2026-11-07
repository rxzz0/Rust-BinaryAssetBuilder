//! Structural checks for the files accompanying a linked manifest.

use crate::manifest::Manifest;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

const CHECKSUM_SIZE: u64 = 4;

/// Checks the lengths and stored checksums of the linked .bin, .relo, and .imp files.
pub fn check_linked_files(path: &Path, manifest: &Manifest<'_>) -> io::Result<()> {
    if !manifest.is_linked {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest describes an unlinked stream",
        ));
    }

    let mut lengths = [CHECKSUM_SIZE; 3];
    for asset in &manifest.assets {
        // A patch can omit the chunks of an unchanged asset by storing zero sizes.
        lengths[0] += u64::from(asset.chunk_sizes.instance);
        lengths[1] += u64::from(asset.chunk_sizes.relocations);
        lengths[2] += u64::from(asset.chunk_sizes.imports);
    }

    for (extension, expected_length) in ["bin", "relo", "imp"].into_iter().zip(lengths) {
        let path = path.with_extension(extension);
        check_file(&path, manifest, expected_length).map_err(|error| {
            io::Error::new(error.kind(), format!("{} ({error})", path.display()))
        })?;
    }
    Ok(())
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
