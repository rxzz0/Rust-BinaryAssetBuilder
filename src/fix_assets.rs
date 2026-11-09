//! Native post-processing for a linked RA3 mod stream.

use crate::compile_batch::{Contents, publish_prepared};
use crate::linked_stream::check_linked_files;
use crate::manifest::{AssetId, ByteOrder, Manifest};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path};

type Output = (OsString, Vec<u8>);

#[derive(Default)]
pub struct FixResult {
    pub assets: usize,
    pub hashes_fixed: usize,
}

/// Reconciles WorldBuilder overrides, orders local dependencies, and writes LoD patches.
pub fn fix_assets(input: &Path, references: &Path) -> io::Result<FixResult> {
    let name = input
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| name.is_ascii())
        .ok_or_else(|| invalid("expected an ASCII manifest file name"))?;
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| invalid("expected a mod stream name"))?;
    if !input
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("manifest"))
    {
        return Err(invalid("expected a mod .manifest file"));
    }
    if !input.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "static stream is missing. Build Static Data must succeed before Fix Assets",
        ));
    }
    let directory = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut stats = FixResult::default();
    publish_prepared(
        directory,
        || {
            let bytes = fs::read(input)?;
            let mut manifest = Manifest::parse(&bytes).map_err(invalid)?;
            if manifest.version != 6
                || manifest.byte_order != ByteOrder::Little
                || manifest.assets.is_empty()
                || manifest
                    .referenced_manifests
                    .iter()
                    .any(|reference| reference.is_patch)
            {
                return Err(invalid("expected a nonempty base RA3 PC stream"));
            }
            check_linked_files(input, &manifest)?;
            let external = external_ids(&manifest, directory, references)?;
            let order = dependency_order(&manifest, &external)?;
            let hashes = worldbuilder_hashes(references)?;
            for asset in &mut manifest.assets {
                if let Some(&hash) = hashes.get(&asset.id)
                    && asset.instance_hash != hash
                {
                    asset.instance_hash = hash;
                    stats.hashes_fixed += 1;
                }
            }
            stats.assets = manifest.assets.len();
            let reordered = order.iter().copied().ne(0..order.len());
            let mut outputs = Vec::new();
            if reordered {
                reorder_chunks(input, stem, &manifest, &order, &mut outputs)?;
                manifest.assets = order.iter().map(|&i| manifest.assets[i].clone()).collect();
            }
            let changed = (reordered || stats.hashes_fixed != 0)
                .then(|| manifest.encode())
                .transpose()
                .map_err(invalid)?;
            let base = changed.as_deref().unwrap_or(&bytes);
            outputs.extend(lod_outputs(base, name, stem, manifest.stream_checksum)?);
            if let Some(base) = changed {
                outputs.push((name.into(), base));
            }
            Ok(outputs
                .into_iter()
                .map(|(name, bytes)| (name, Contents::Bytes(bytes)))
                .collect())
        },
        |_| false,
    )?;
    Ok(stats)
}

fn external_ids(
    manifest: &Manifest<'_>,
    directory: &Path,
    references: &Path,
) -> io::Result<HashSet<AssetId>> {
    let mut ids = HashSet::new();
    let mut seen = HashSet::new();
    let mut pending: Vec<_> = manifest
        .referenced_manifests
        .iter()
        .map(|entry| (directory.to_path_buf(), entry.path.to_vec()))
        .collect();
    while let Some((directory, name)) = pending.pop() {
        let name = std::str::from_utf8(&name)
            .map_err(invalid)?
            .replace('\\', "/");
        let relative = Path::new(&name);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || name.contains(':')
        {
            return Err(invalid(format!(
                "invalid referenced manifest path {name:?}"
            )));
        }
        let path = [
            directory.join(relative),
            references.join(relative),
            references.join("sagexml").join(relative),
        ]
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("referenced manifest {name} is missing"),
            )
        })?;
        let path = fs::canonicalize(path)?;
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = fs::read(&path)?;
        let manifest = Manifest::parse(&bytes)
            .map_err(|error| invalid(format!("{} ({error})", path.display())))?;
        ids.extend(manifest.assets.into_iter().map(|asset| asset.id));
        let parent = path.parent().unwrap_or(references);
        pending.extend(
            manifest
                .referenced_manifests
                .iter()
                .map(|entry| (parent.to_path_buf(), entry.path.to_vec())),
        );
    }
    Ok(ids)
}

fn worldbuilder_hashes(references: &Path) -> io::Result<HashMap<AssetId, u32>> {
    let schema_copy = references.join("sagexml").join("worldbuilder.manifest");
    let path = if schema_copy.is_file() {
        schema_copy
    } else {
        references.join("worldbuilder.manifest")
    };
    let bytes = fs::read(&path)
        .map_err(|error| io::Error::new(error.kind(), format!("{} ({error})", path.display())))?;
    let manifest = Manifest::parse(&bytes).map_err(invalid)?;
    Ok(manifest
        .assets
        .into_iter()
        .map(|asset| (asset.id, asset.instance_hash))
        .collect())
}

fn dependency_order(
    manifest: &Manifest<'_>,
    external: &HashSet<AssetId>,
) -> io::Result<Vec<usize>> {
    let mut positions = HashMap::with_capacity(manifest.assets.len());
    for (index, asset) in manifest.assets.iter().enumerate() {
        if positions.insert(asset.id, index).is_some() {
            return Err(invalid(format!(
                "duplicate asset {}",
                String::from_utf8_lossy(asset.name)
            )));
        }
    }
    let mut dependents = vec![Vec::new(); manifest.assets.len()];
    let mut remaining = vec![0; manifest.assets.len()];
    for (index, asset) in manifest.assets.iter().enumerate() {
        for reference in asset.references.iter() {
            if let Some(&dependency) = positions.get(&reference) {
                dependents[dependency].push(index);
                remaining[index] += 1;
            } else if !external.contains(&reference) {
                return Err(invalid(format!(
                    "{} references unresolved asset {:08x}:{:08x}",
                    String::from_utf8_lossy(asset.name),
                    reference.type_id,
                    reference.instance_id
                )));
            }
        }
    }
    let mut ready: BinaryHeap<_> = (0..remaining.len())
        .filter(|&index| remaining[index] == 0)
        .map(Reverse)
        .collect();
    let mut order = Vec::with_capacity(remaining.len());
    while let Some(Reverse(index)) = ready.pop() {
        order.push(index);
        for &dependent in &dependents[index] {
            remaining[dependent] -= 1;
            if remaining[dependent] == 0 {
                ready.push(Reverse(dependent));
            }
        }
    }
    if order.len() != remaining.len() {
        return Err(invalid("cycle in local asset dependencies"));
    }
    Ok(order)
}

fn reorder_chunks(
    input: &Path,
    stem: &str,
    manifest: &Manifest<'_>,
    order: &[usize],
    outputs: &mut Vec<Output>,
) -> io::Result<()> {
    for (column, extension) in ["bin", "relo", "imp"].into_iter().enumerate() {
        let source = fs::read(input.with_extension(extension))?;
        let mut start = 4;
        let chunks: Vec<_> = manifest
            .assets
            .iter()
            .map(|asset| {
                let end = start + asset.chunk_sizes.as_array()[column] as usize;
                let chunk = source
                    .get(start..end)
                    .ok_or_else(|| invalid("linked stream changed during sorting"));
                start = end;
                chunk
            })
            .collect::<io::Result<_>>()?;
        let mut output = Vec::with_capacity(source.len());
        output.extend_from_slice(
            source
                .get(..4)
                .ok_or_else(|| invalid("linked stream checksum is missing"))?,
        );
        for &index in order {
            output.extend_from_slice(chunks[index]);
        }
        outputs.push((format!("{stem}.{extension}").into(), output));
    }
    Ok(())
}

fn lod_outputs(base: &[u8], name: &str, stem: &str, checksum: u32) -> io::Result<Vec<Output>> {
    let bytes = Manifest::inherited_patch(base, name.as_bytes()).map_err(invalid)?;
    let mut outputs = Vec::with_capacity(10);
    for lod in ["l", "m"] {
        for extension in ["bin", "relo", "imp"] {
            outputs.push((
                format!("{stem}_{lod}.{extension}").into(),
                checksum.to_le_bytes().to_vec(),
            ));
        }
        outputs.push((format!("{stem}_{lod}.manifest").into(), bytes.clone()));
        outputs.push((format!("{stem}_{lod}.version").into(), b"\r\n".to_vec()));
    }
    Ok(outputs)
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
