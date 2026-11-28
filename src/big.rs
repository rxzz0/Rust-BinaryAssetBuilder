//! Uncompressed BIG4 archives produced by the Red Alert 3 SDK.

use crate::compile::{configured_map_roots, has_packable_map, map_folders};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf, absolute};
use std::time::{SystemTime, UNIX_EPOCH};

const BUFFER_SIZE: usize = 1024 * 1024;
const INDEX_TRAILER: &[u8] = b"L219\0\0\0\0";
const OFFICIAL_MAPS: &str = "data/maps/official";
const OFFICIAL_MAP_PREFIX: &str = "data\\maps\\official\\";

struct Entry {
    path: PathBuf,
    name: String,
    size: u32,
    modified: SystemTime,
}

pub struct PackResult {
    pub entries: usize,
    pub reused: bool,
}

/// Packages a directory in SDK order, excluding loose .asset intermediates.
/// Publishes the archive only after every input has been copied successfully.
pub fn pack_big(input: &Path, output: &Path) -> io::Result<PackResult> {
    stage_big(input, output, output)
}

/// Writes a replacement only when the previously published BIG differs from the input.
pub fn stage_big(input: &Path, output: &Path, previous: &Path) -> io::Result<PackResult> {
    let input = fs::canonicalize(input)?;
    let output = archive_output(output, std::iter::once(input.as_path()))?;
    let mut entries = Vec::new();
    collect(
        &input,
        &input,
        Path::new(""),
        &MapInputs::default(),
        &mut entries,
    )?;
    stage_entries(&entries, &output, previous)
}

/// Packages the SDK Misc directory and any configured map folders.
pub fn stage_misc_big(
    mod_directory: &Path,
    output: &Path,
    previous: &Path,
) -> io::Result<Option<PackResult>> {
    let misc = mod_directory.join("Misc");
    let misc = match fs::canonicalize(&misc) {
        Ok(path) => Some(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let roots = configured_map_roots(mod_directory)?;
    let mut maps = Vec::new();
    let mut by_name = HashMap::new();
    let mut inputs = MapInputs::default();
    for root in &roots {
        for map in map_folders(root)? {
            let source = map.path();
            if !has_packable_map(&source)? {
                continue;
            }
            let name = archive_name(Path::new(&map.file_name()))?;
            if let Some(other) = by_name.insert(name.clone(), source.clone()) {
                if source != other {
                    return Err(invalid(format!(
                        "map folder {name:?} is present in {} and {}",
                        other.display(),
                        source.display()
                    )));
                }
                continue;
            }
            inputs.mapped.insert(source.clone());
            maps.push((source, Path::new(OFFICIAL_MAPS).join(name)));
        }
    }
    if misc.is_none() && maps.is_empty() {
        return Ok(None);
    }
    inputs
        .configured_roots
        .extend(roots.iter().map(PathBuf::as_path));
    let output = archive_output(
        output,
        misc.as_deref()
            .into_iter()
            .chain(roots.iter().map(PathBuf::as_path)),
    )?;
    let mut entries = Vec::new();
    if let Some(misc) = &misc {
        collect(misc, misc, Path::new(""), &inputs, &mut entries)?;
    }
    for entry in &entries {
        if !by_name.is_empty()
            && matches!(
                entry.name.as_str(),
                "data" | "data\\maps" | "data\\maps\\official"
            )
        {
            return Err(invalid(format!(
                "Misc file {:?} blocks the official map archive directory",
                entry.name
            )));
        }
        if let Some(name) = entry
            .name
            .strip_prefix(OFFICIAL_MAP_PREFIX)
            .and_then(|name| name.split('\\').next())
            && let Some(other) = by_name.get(name)
        {
            return Err(invalid(format!(
                "map folder {name:?} is present in Misc and {}",
                other.display()
            )));
        }
    }
    for (source, name) in maps {
        collect(&source, &source, &name, &MapInputs::default(), &mut entries)?;
    }
    if !by_name.is_empty() {
        // MakeBig visits child directories in name order before the files in each directory.
        entries.sort_by_cached_key(|entry| {
            let mut components = entry.name.split('\\').peekable();
            let mut key = Vec::new();
            while let Some(component) = components.next() {
                key.push((components.peek().is_none(), component.to_ascii_uppercase()));
            }
            key
        });
    }
    stage_entries(&entries, &output, previous).map(Some)
}

fn archive_output<'a>(
    output: &Path,
    inputs: impl IntoIterator<Item = &'a Path>,
) -> io::Result<PathBuf> {
    let output = absolute(output)?;
    output
        .file_name()
        .ok_or_else(|| invalid("BIG output needs a file name"))?;
    let parent = output.parent().unwrap();
    fs::create_dir_all(parent)?;
    let parent = fs::canonicalize(parent)?;
    if inputs.into_iter().any(|input| parent.starts_with(input)) {
        return Err(invalid("BIG output must be outside the input directory"));
    }
    Ok(output)
}

fn stage_entries(entries: &[Entry], output: &Path, previous: &Path) -> io::Result<PackResult> {
    let index_end = entries
        .iter()
        .try_fold(16 + INDEX_TRAILER.len() as u32, |offset, entry| {
            offset.checked_add(9 + entry.name.len() as u32)
        })
        .ok_or_else(|| invalid("BIG index exceeds 4 GiB"))?;
    let size = entries
        .iter()
        .try_fold(index_end, |offset, entry| offset.checked_add(entry.size))
        .ok_or_else(|| invalid("BIG archive exceeds 4 GiB"))?;
    let count = u32::try_from(entries.len()).map_err(|_| invalid("too many BIG entries"))?;
    let index = encode_index(entries, size, count, index_end);
    if existing_matches(previous, entries, &index, size)? {
        return Ok(PackResult {
            entries: entries.len(),
            reused: true,
        });
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mut temporary = output.file_name().unwrap().to_os_string();
    temporary.push(format!(".{}.{nonce}.tmp", std::process::id()));
    let temporary = output.with_file_name(temporary);
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let result = write_big(file, entries, &index).and_then(|()| fs::rename(&temporary, output));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map(|()| PackResult {
        entries: entries.len(),
        reused: false,
    })
}

#[derive(Default)]
struct MapInputs<'a> {
    mapped: HashSet<PathBuf>,
    configured_roots: HashSet<&'a Path>,
}

fn collect(
    root: &Path,
    directory: &Path,
    prefix: &Path,
    maps: &MapInputs<'_>,
    entries: &mut Vec<Entry>,
) -> io::Result<()> {
    let mut children = fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?;
    children.sort_by_cached_key(|entry| entry.file_name().to_ascii_uppercase());
    let mut files = Vec::new();
    let mut previous = None;
    for child in children {
        let key = child.file_name().to_ascii_uppercase();
        if previous.as_ref() == Some(&key) {
            return Err(invalid(format!(
                "case-insensitive BIG name collision in {}",
                directory.display()
            )));
        }
        previous = Some(key);
        let kind = child.file_type()?;
        let path = child.path();
        if kind.is_dir() {
            if !maps.mapped.contains(&path) {
                collect(root, &path, prefix, maps, entries)?;
            }
        } else if kind.is_file() {
            files.push(child);
        } else if maps.configured_roots.is_empty()
            || !fs::canonicalize(&path)
                .is_ok_and(|physical| maps.configured_roots.contains(physical.as_path()))
        {
            return Err(invalid(format!(
                "unsupported BIG input {}",
                child.path().display()
            )));
        }
    }
    for file in files {
        let path = file.path();
        let name = archive_name(&prefix.join(path.strip_prefix(root).unwrap()))?;
        if name.ends_with(".asset") {
            continue;
        }
        let metadata = file.metadata()?;
        let size = u32::try_from(metadata.len())
            .map_err(|_| invalid(format!("BIG input exceeds 4 GiB {}", path.display())))?;
        entries.push(Entry {
            path,
            name,
            size,
            modified: metadata.modified()?,
        });
    }
    Ok(())
}

fn archive_name(path: &Path) -> io::Result<String> {
    let name = path
        .to_str()
        .filter(|name| name.is_ascii() && name.len() < 260)
        .ok_or_else(|| invalid(format!("invalid SDK archive name {}", path.display())))?;
    Ok(name.replace('/', "\\").to_ascii_lowercase())
}

fn encode_index(entries: &[Entry], size: u32, count: u32, index_end: u32) -> Vec<u8> {
    let mut index = Vec::with_capacity(index_end as usize);
    index.extend_from_slice(b"BIG4");
    index.extend(size.to_le_bytes());
    index.extend(count.to_be_bytes());
    index.extend(index_end.to_be_bytes());
    let mut offset = index_end;
    for entry in entries {
        index.extend(offset.to_be_bytes());
        index.extend(entry.size.to_be_bytes());
        index.extend_from_slice(entry.name.as_bytes());
        index.push(0);
        offset += entry.size;
    }
    index.extend_from_slice(INDEX_TRAILER);
    index
}

fn existing_matches(path: &Path, entries: &[Entry], index: &[u8], size: u32) -> io::Result<bool> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if file.metadata()?.len() != u64::from(size) {
        return Ok(false);
    }
    let mut archive = BufReader::with_capacity(BUFFER_SIZE, file);
    let mut header = vec![0; index.len()];
    archive.read_exact(&mut header)?;
    if header != index {
        return Ok(false);
    }
    let mut existing = vec![0; BUFFER_SIZE];
    stream_entries(entries, |source| {
        let chunk = &mut existing[..source.len()];
        archive.read_exact(chunk)?;
        Ok(source == chunk)
    })
}

fn write_big(file: File, entries: &[Entry], index: &[u8]) -> io::Result<()> {
    let mut output = BufWriter::with_capacity(BUFFER_SIZE, file);
    output.write_all(index)?;
    stream_entries(entries, |chunk| {
        output.write_all(chunk)?;
        Ok(true)
    })?;
    output.flush()
}

fn stream_entries(
    entries: &[Entry],
    mut consume: impl FnMut(&[u8]) -> io::Result<bool>,
) -> io::Result<bool> {
    let mut buffer = vec![0; BUFFER_SIZE];
    for entry in entries {
        let mut input = File::open(&entry.path)?;
        let mut remaining = entry.size as usize;
        while remaining > 0 {
            let chunk = remaining.min(buffer.len());
            input.read_exact(&mut buffer[..chunk])?;
            if !consume(&buffer[..chunk])? {
                return Ok(false);
            }
            remaining -= chunk;
        }
        let metadata = input.metadata()?;
        if metadata.len() != u64::from(entry.size) || metadata.modified()? != entry.modified {
            return Err(invalid(format!(
                "BIG input changed while packing {}",
                entry.path.display()
            )));
        }
    }
    Ok(true)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
