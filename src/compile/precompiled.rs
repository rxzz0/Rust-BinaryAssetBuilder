//! AssetMerger interchange files used by SDK mods for precompiled overrides.

use super::{CompiledAsset, fast_hash, invalid, read_with_context};
use crate::manifest::AssetId;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) struct Archive {
    pub asset: CompiledAsset,
    pub custom_data: Option<PathBuf>,
}

pub(super) fn read_assets(directory: &Path) -> io::Result<Vec<Archive>> {
    let (paths, mut sidecars) = asset_paths(directory)?;
    let mut assets = Vec::with_capacity(paths.len());
    let mut seen = HashSet::with_capacity(paths.len());
    for path in paths {
        let bytes = read_with_context(&path, fs::read)?;
        let asset = parse_asset(&path, &bytes)?;
        if !seen.insert(asset.id) {
            return Err(invalid(
                &path,
                format!("duplicate precompiled asset {}", asset.name),
            ));
        }
        let sidecar = path.with_file_name(custom_data_name(&asset));
        let custom_data = sidecars.remove(&sidecar_key(&sidecar));
        if let Some(source) = &custom_data
            && !fs::symlink_metadata(source)?.is_file()
        {
            return Err(invalid(source, "expected a regular precompiled sidecar"));
        }
        assets.push(Archive { asset, custom_data });
    }
    Ok(assets)
}

type Sidecars = HashMap<PathBuf, PathBuf>;

fn asset_paths(directory: &Path) -> io::Result<(Vec<PathBuf>, Sidecars)> {
    let root = fs::canonicalize(directory)?;
    let mut pending = vec![root.clone()];
    let mut visited = HashSet::new();
    let mut paths = Vec::new();
    let mut sidecars = HashMap::new();
    while let Some(directory) = pending.pop() {
        if !visited.insert(directory.clone()) {
            continue;
        }
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            let extension = path.extension().unwrap_or_default();
            if !kind.is_dir() && extension.eq_ignore_ascii_case("cdata") {
                let key = sidecar_key(&path);
                if sidecars.insert(key, path.clone()).is_some() {
                    return Err(invalid(&path, "duplicate precompiled sidecar name"));
                }
                continue;
            }
            let is_archive = extension.eq_ignore_ascii_case("asset");
            if !kind.is_dir() && !is_archive {
                continue;
            }
            if kind.is_symlink() || !kind.is_dir() && !kind.is_file() {
                return Err(invalid(&path, "expected a regular precompiled asset"));
            }
            let resolved = fs::canonicalize(&path)?;
            if !resolved.starts_with(&root) {
                return Err(invalid(
                    &path,
                    "precompiled asset lies outside the asset directory",
                ));
            }
            if kind.is_dir() {
                pending.push(resolved);
            } else {
                paths.push(resolved);
            }
        }
    }
    paths.sort_unstable();
    Ok((paths, sidecars))
}

fn sidecar_key(path: &Path) -> PathBuf {
    path.with_file_name(path.file_name().unwrap().to_ascii_lowercase())
}

pub(super) fn custom_data_prefix(asset: &CompiledAsset) -> String {
    format!(
        "{:08X}.{:08X}.{:08X}",
        asset.id.type_id, asset.type_hash, asset.id.instance_id
    )
}

pub(super) fn custom_data_name(asset: &CompiledAsset) -> OsString {
    format!(
        "{}.{:08X}.cdata",
        custom_data_prefix(asset),
        asset.instance_hash
    )
    .into()
}

fn parse_asset(path: &Path, mut bytes: &[u8]) -> io::Result<CompiledAsset> {
    let [
        type_id,
        instance_id,
        type_hash,
        instance_hash,
        reference_count,
        payload,
        relocations,
        imports,
    ] = words::<8>(&mut bytes)
        .ok_or_else(|| invalid(path, "truncated precompiled asset header"))?;
    let reference_size = (reference_count as usize)
        .checked_mul(8)
        .filter(|&size| size <= bytes.len())
        .ok_or_else(|| invalid(path, "invalid precompiled asset reference size"))?;
    let (reference_bytes, remainder) = bytes.split_at(reference_size);
    bytes = remainder;
    let (references, _) = reference_bytes.as_chunks::<8>();
    let references = references
        .iter()
        .map(|bytes| {
            let ([type_id, instance_id], []) = bytes.as_chunks::<4>() else {
                unreachable!("an asset reference contains exactly two words")
            };
            AssetId {
                type_id: u32::from_le_bytes(*type_id),
                instance_id: u32::from_le_bytes(*instance_id),
            }
            .into()
        })
        .collect();
    let name =
        read_name(&mut bytes).ok_or_else(|| invalid(path, "invalid precompiled asset name"))?;
    let source =
        read_name(&mut bytes).ok_or_else(|| invalid(path, "invalid precompiled asset source"))?;
    let (kind, instance) = name
        .split_once(':')
        .filter(|(kind, instance)| {
            !kind.is_empty() && !instance.is_empty() && !instance.contains(':')
        })
        .ok_or_else(|| invalid(path, "expected a Type:Name precompiled asset name"))?;
    if type_id != fast_hash(kind.as_bytes())
        || instance_id != fast_hash(instance.to_ascii_lowercase().as_bytes())
    {
        return Err(invalid(
            path,
            "precompiled name does not match its asset ID",
        ));
    }
    let file_stem = path.file_stem().and_then(|stem| stem.to_str());
    if !file_stem.is_some_and(|stem| stem.eq_ignore_ascii_case(&format!("{kind}.{instance}"))) {
        return Err(invalid(path, "expected a Type.Name.asset file name"));
    }
    // The interchange format stores sidecars separately and has no tokenization flag.
    let sort_order = match (kind, type_hash) {
        ("W3DAnimation", 0x5545_5fe8) => 66,
        ("AudioFile", 0x53c8_1e47) => 39,
        _ => {
            return Err(invalid(
                path,
                format!("unsupported precompiled asset type {kind}"),
            ));
        }
    };
    let [Some(payload), Some(relocations), Some(imports)] =
        [payload, relocations, imports].map(|size| take(&mut bytes, size as usize))
    else {
        return Err(invalid(path, "truncated precompiled asset payload"));
    };
    if !bytes.is_empty() {
        return Err(invalid(
            path,
            "trailing bytes after precompiled asset payload",
        ));
    }
    Ok(CompiledAsset {
        id: AssetId {
            type_id,
            instance_id,
        },
        type_hash,
        instance_hash,
        name: name.to_owned(),
        source: source.to_owned(),
        payload: payload.to_vec(),
        relocations: relocations.to_vec(),
        imports: imports.to_vec(),
        references,
        weak_references: Vec::new(),
        custom_data: None,
        tokenized: false,
        sort_order,
    })
}

fn read_name<'a>(bytes: &mut &'a [u8]) -> Option<&'a str> {
    let [length] = words::<1>(bytes)?;
    let name = take(bytes, length as usize)?;
    if name.is_empty()
        || !name
            .iter()
            .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        return None;
    }
    std::str::from_utf8(name).ok()
}

fn words<const N: usize>(bytes: &mut &[u8]) -> Option<[u32; N]> {
    let array: &[[u8; 4]; N] = take(bytes, N * 4)?.as_chunks::<4>().0.try_into().ok()?;
    Some(array.map(u32::from_le_bytes))
}

fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
    let (chunk, remainder) = bytes.split_at_checked(length)?;
    *bytes = remainder;
    Some(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_exact_ids_names_and_linked_chunks() {
        let path = Path::new("w3danimation.test.asset");
        let name = b"W3DAnimation:TEST";
        let source = b"art:test.w3x";
        let words = [
            fast_hash(b"W3DAnimation"),
            fast_hash(b"test"),
            0x5545_5fe8,
            17,
            1,
            4,
            4,
            8,
            fast_hash(b"W3DHierarchy"),
            fast_hash(b"test_hierarchy"),
            name.len() as u32,
        ];
        let mut bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&(source.len() as u32).to_le_bytes());
        bytes.extend_from_slice(source);
        bytes.extend([0; 4]);
        bytes.extend(u32::MAX.to_le_bytes());
        bytes.extend(4u32.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes());

        let asset = parse_asset(path, &bytes).unwrap();
        assert_eq!(asset.id.instance_id, fast_hash(b"test"));
        assert_eq!(asset.instance_hash, 17);
        assert_eq!(asset.name.as_bytes(), name);
        assert_eq!(asset.source.as_bytes(), source);
        assert_eq!(asset.payload, [0; 4]);
        assert_eq!(asset.relocations, u32::MAX.to_le_bytes());
        assert_eq!(asset.imports, [4, 0, 0, 0, 255, 255, 255, 255]);
        assert_eq!(asset.references[0].id.type_id, fast_hash(b"W3DHierarchy"));
        assert_eq!(
            asset.references[0].id.instance_id,
            fast_hash(b"test_hierarchy")
        );
        assert!(parse_asset(Path::new("W3DAnimation.other.asset"), &bytes).is_err());
        assert!(parse_asset(path, &bytes[..bytes.len() - 1]).is_err());
        bytes.push(0);
        assert!(parse_asset(path, &bytes).is_err());
    }
}
