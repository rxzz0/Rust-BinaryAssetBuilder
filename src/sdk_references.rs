//! Seeds a fresh SDK with the stock reference manifests from a local RA3 installation.

use crate::manifest::{ByteOrder, Manifest};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const AUDIO_BASE: &str = "audio.manifest";
pub(crate) const AUDIO_PATCH: &str = "audio.5.manifest";

pub(crate) fn is_stock_audio_catalog(path: &Path, manifest: &Manifest<'_>) -> bool {
    manifest.is_linked
        && path.file_name().is_some_and(|name| {
            (name.eq_ignore_ascii_case(AUDIO_BASE) && manifest.all_types_hash == 0x5e3b_5247)
                || (name.eq_ignore_ascii_case(AUDIO_PATCH)
                    && manifest.all_types_hash == 0xbde2_5aad)
        })
}

pub fn prepare_references(built_mods: &Path, game_data: &Path) -> io::Result<usize> {
    let sources = [
        ("global.manifest", "WBGlobal.big"),
        ("static.manifest", "WBStatic.big"),
        ("worldbuilder.manifest", "WBData.big"),
        (AUDIO_BASE, "EnglishAudio.big"),
    ];
    let mut missing = sources
        .into_iter()
        .filter(|(name, _)| {
            !built_mods.join(name).is_file() && !built_mods.join("sagexml").join(name).is_file()
        })
        .peekable();
    if missing.peek().is_none() {
        return Ok(0);
    }

    let mut manifests = Vec::new();
    for (name, archive) in missing {
        let bytes = (|| {
            let archive = if name == AUDIO_BASE {
                audio_archive(game_data)?
            } else {
                game_data.join(archive)
            };
            read_member(&archive, name)
                .and_then(|bytes| unpack_manifest(bytes, name))
                .map_err(|error| {
                    io::Error::new(error.kind(), format!("{} ({error})", archive.display()))
                })
        })()
        .map_err(|error: io::Error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Cannot prepare stock {name} from the Red Alert 3 Data folder {} ({error}). \
                 Check that Red Alert 3 and its stock archives are installed.",
                    game_data.display()
                ),
            )
        })?;
        if name == AUDIO_BASE {
            let patch = game_data.join("EnglishAudio5.big");
            if patch.is_file() && game_data.join("EnglishAudio.big").is_file() {
                let bytes = read_member(&patch, AUDIO_PATCH)
                    .and_then(|bytes| unpack_manifest(bytes, AUDIO_PATCH))
                    .map_err(|error| {
                        io::Error::new(error.kind(), format!("{} ({error})", patch.display()))
                    })?;
                manifests.push((AUDIO_PATCH, bytes));
            }
        }
        manifests.push((name, bytes));
    }
    fs::create_dir_all(built_mods)?;
    for (name, bytes) in &manifests {
        publish(built_mods, name, bytes)?;
    }
    Ok(manifests.len())
}

fn audio_archive(directory: &Path) -> io::Result<PathBuf> {
    let english = directory.join("EnglishAudio.big");
    if english.is_file() {
        return Ok(english);
    }
    let mut candidates = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("audio.big")
        })
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    candidates.sort();
    for candidate in candidates {
        if read_member(&candidate, AUDIO_BASE).is_ok() {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "no language audio BIG contains audio.manifest",
    ))
}

fn publish(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let staged = directory.join(format!(".{name}.{}.{nonce}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staged)?;
    let written = file.write_all(bytes);
    drop(file);
    let target = directory.join(name);
    let result = written.and_then(|()| {
        if target.is_file() {
            Ok(())
        } else {
            fs::rename(&staged, target)
        }
    });
    let _ = fs::remove_file(staged);
    result
}

fn read_member(path: &Path, name: &str) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let file_size = file.metadata()?.len();
    let mut file = BufReader::new(file);
    let mut header = [0; 16];
    file.read_exact(&mut header)?;
    if !matches!(&header[..4], b"BIG4" | b"BIGF") {
        return Err(invalid("invalid BIG archive header"));
    }
    let count = u32::from_be_bytes(header[8..12].try_into().unwrap());
    let index_end = u64::from(u32::from_be_bytes(header[12..16].try_into().unwrap()));
    if !(16..=file_size).contains(&index_end) || u64::from(count) > (index_end - 16) / 9 {
        return Err(invalid("invalid BIG archive index"));
    }

    let mut index = file.by_ref().take(index_end - 16);
    let mut entry_name = Vec::new();
    for _ in 0..count {
        let mut entry = [0; 8];
        index.read_exact(&mut entry)?;
        entry_name.clear();
        index.read_until(0, &mut entry_name)?;
        if entry_name.pop() != Some(0) {
            return Err(invalid("unterminated BIG member name"));
        }
        let entry_name = entry_name
            .rsplit(|&byte| byte == b'\\' || byte == b'/')
            .next()
            .unwrap();
        if !entry_name.eq_ignore_ascii_case(name.as_bytes()) {
            continue;
        }
        let offset = u64::from(u32::from_be_bytes(entry[..4].try_into().unwrap()));
        let size = u64::from(u32::from_be_bytes(entry[4..].try_into().unwrap()));
        if offset < index_end || offset + size > file_size || size > 0xff_ffff {
            return Err(invalid("invalid BIG manifest member size or offset"));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; size as usize];
        file.read_exact(&mut bytes)?;
        return Ok(bytes);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("BIG member {name} is missing"),
    ))
}

fn unpack_manifest(bytes: Vec<u8>, name: &str) -> io::Result<Vec<u8>> {
    let bytes = if bytes.starts_with(&[0x10, 0xfb]) {
        unpack_refpack(&bytes)?
    } else {
        bytes
    };
    let manifest = Manifest::parse(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if manifest.version != 6
        || manifest.byte_order != ByteOrder::Little
        || (manifest.all_types_hash != crate::compile::RA3_ALL_TYPES_HASH
            && !is_stock_audio_catalog(Path::new(name), &manifest))
    {
        return Err(invalid("expected a stock RA3 PC manifest"));
    }
    Ok(bytes)
}

// RA3 stock manifests use the five-byte RefPack header with a 24-bit decoded size.
fn unpack_refpack(mut bytes: &[u8]) -> io::Result<Vec<u8>> {
    fn take_byte(bytes: &mut &[u8]) -> io::Result<usize> {
        let (&byte, rest) = bytes
            .split_first()
            .ok_or_else(|| invalid("truncated RefPack stream"))?;
        *bytes = rest;
        Ok(byte.into())
    }

    let header = bytes
        .get(2..5)
        .ok_or_else(|| invalid("truncated RefPack header"))?;
    let size = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
    bytes = &bytes[5..];
    let mut out = Vec::with_capacity(size);
    loop {
        let control = take_byte(&mut bytes)?;
        let (literals, length, distance) = match control {
            0x00..=0x7f => {
                let next = take_byte(&mut bytes)?;
                (
                    control & 3,
                    ((control >> 2) & 7) + 3,
                    ((control & 0x60) << 3) + next + 1,
                )
            }
            0x80..=0xbf => {
                let next = take_byte(&mut bytes)?;
                let low = take_byte(&mut bytes)?;
                (
                    next >> 6,
                    (control & 0x3f) + 4,
                    ((next & 0x3f) << 8) + low + 1,
                )
            }
            0xc0..=0xdf => {
                let high = take_byte(&mut bytes)?;
                let low = take_byte(&mut bytes)?;
                let length = take_byte(&mut bytes)?;
                (
                    control & 3,
                    ((control & 0x0c) << 6) + length + 5,
                    ((control & 0x10) << 12) + (high << 8) + low + 1,
                )
            }
            0xe0..=0xfb => (((control & 0x1f) + 1) * 4, 0, 0),
            _ => (control & 3, 0, 0),
        };
        if out.len() + literals + length > size || distance > out.len() + literals {
            return Err(invalid("RefPack command exceeds its output bounds"));
        }
        let (literal_bytes, rest) = bytes
            .split_at_checked(literals)
            .ok_or_else(|| invalid("truncated RefPack literals"))?;
        out.extend_from_slice(literal_bytes);
        bytes = rest;
        for _ in 0..length {
            out.push(out[out.len() - distance]);
        }
        if control >= 0xfc {
            return if out.len() == size {
                Ok(out)
            } else {
                Err(invalid("RefPack size mismatch"))
            };
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refpack_decodes_overlapping_matches_and_rejects_truncation() {
        let bytes = [0x10, 0xfb, 0, 0, 5, 0x01, 0, b'A', 0xfd, b'B'];
        assert_eq!(unpack_refpack(&bytes).unwrap(), b"AAAAB");
        assert!(unpack_refpack(&bytes[..bytes.len() - 1]).is_err());
        let mut bad_distance = bytes;
        bad_distance[6] = 1;
        assert!(unpack_refpack(&bad_distance).is_err());
    }
}
