//! Merges SDK precompiled assets into a native linked stream.

use super::{RA3_ALL_TYPES_HASH, invalid, precompiled, read_with_context};
use crate::compile_batch::{Contents, Output, publish_prepared};
use crate::linked_stream::check_linked_files;
use crate::manifest::{AssetEntry, AssetReferences, ByteOrder, ChunkSizes, Manifest};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

const CHUNK_EXTENSIONS: [&str; 3] = ["bin", "relo", "imp"];

/// Publishes precompiled overrides and preserves the relative order of untouched assets.
pub fn merge_assets(input: &Path, assets_directory: &Path) -> io::Result<usize> {
    if !input
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("manifest"))
    {
        return Err(invalid(input, "expected a .manifest file"));
    }
    let directory = input
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut count = 0;
    publish_prepared(
        directory,
        || {
            let bytes = read_with_context(input, fs::read)?;
            let mut manifest = Manifest::parse(&bytes).map_err(|error| invalid(input, error))?;
            if manifest.version != 6
                || manifest.byte_order != ByteOrder::Little
                || manifest.all_types_hash != RA3_ALL_TYPES_HASH
                || manifest
                    .referenced_manifests
                    .iter()
                    .any(|entry| entry.is_patch)
            {
                return Err(invalid(input, "expected a base RA3 PC stream"));
            }
            check_linked_files(input, &manifest)?;
            let incoming = precompiled::read_assets(assets_directory)?;
            count = incoming.len();
            if incoming.is_empty() {
                return Ok(Vec::new());
            }

            let [bin, relo, imp] = CHUNK_EXTENSIONS
                .map(|extension| read_with_context(&input.with_extension(extension), fs::read));
            let sources = [bin?, relo?, imp?];
            let checksum = manifest.stream_checksum.to_le_bytes();
            let [Some(bin), Some(relo), Some(imp)] = sources
                .each_ref()
                .map(|source| source.strip_prefix(checksum.as_slice()))
            else {
                return Err(invalid(input, "linked stream changed during merge"));
            };
            let mut remaining = [bin, relo, imp];
            let mut outputs = sources.each_ref().map(|source| {
                let mut output = Vec::with_capacity(source.len());
                output.extend_from_slice(&checksum);
                output
            });
            let references: Vec<Vec<_>> = incoming
                .iter()
                .map(|archive| {
                    archive
                        .asset
                        .references
                        .iter()
                        .map(|reference| reference.id)
                        .collect()
                })
                .collect();
            let mut entries = Vec::with_capacity(manifest.assets.len() + incoming.len());
            let mut incoming_ids = HashMap::with_capacity(incoming.len());
            for (archive, references) in incoming.iter().zip(&references) {
                let asset = &archive.asset;
                incoming_ids.insert(asset.id, asset);
                let mut sizes = [0; 3];
                for ((output, size), chunk) in outputs.iter_mut().zip(&mut sizes).zip([
                    &asset.payload,
                    &asset.relocations,
                    &asset.imports,
                ]) {
                    *size = u32::try_from(chunk.len())
                        .map_err(|_| invalid(input, "precompiled asset chunk is too large"))?;
                    output.extend_from_slice(chunk);
                }
                let [instance, relocations, imports] = sizes;
                entries.push(AssetEntry {
                    id: asset.id,
                    type_hash: asset.type_hash,
                    instance_hash: asset.instance_hash,
                    chunk_sizes: ChunkSizes {
                        instance,
                        relocations,
                        imports,
                    },
                    name: asset.name.as_bytes(),
                    source_file_name: asset.source.as_bytes(),
                    references: AssetReferences::from_ids(references),
                    tokenized: Some(u32::from(asset.tokenized)),
                });
            }

            let mut seen = HashSet::with_capacity(manifest.assets.len());
            for entry in manifest.assets {
                if !seen.insert(entry.id) {
                    return Err(invalid(
                        input,
                        format!(
                            "duplicate stream asset {}",
                            String::from_utf8_lossy(entry.name)
                        ),
                    ));
                }
                let replacement = incoming_ids.get(&entry.id);
                if let Some(asset) = replacement
                    && (!entry.name.eq_ignore_ascii_case(asset.name.as_bytes())
                        || entry.type_hash != asset.type_hash)
                {
                    return Err(invalid(
                        input,
                        format!(
                            "precompiled asset conflicts with {}",
                            String::from_utf8_lossy(entry.name)
                        ),
                    ));
                }
                for ((output, source), size) in outputs
                    .iter_mut()
                    .zip(&mut remaining)
                    .zip(entry.chunk_sizes.as_array())
                {
                    let (chunk, rest) = source
                        .split_at_checked(size as usize)
                        .ok_or_else(|| invalid(input, "linked stream changed during merge"))?;
                    *source = rest;
                    if replacement.is_none() {
                        output.extend_from_slice(chunk);
                    }
                }
                if replacement.is_none() {
                    entries.push(entry);
                }
            }
            if remaining.iter().any(|source| !source.is_empty()) {
                return Err(invalid(input, "linked stream changed during merge"));
            }
            manifest.assets = entries;
            manifest.total_instance_data_size = u32::try_from(outputs[0].len() - 4)
                .map_err(|_| invalid(input, "merged stream is too large"))?;
            manifest.max_chunk_sizes = ChunkSizes {
                instance: 0,
                relocations: 0,
                imports: 0,
            };
            for asset in &manifest.assets {
                let sizes = asset.chunk_sizes;
                let maxima = &mut manifest.max_chunk_sizes;
                maxima.instance = maxima.instance.max(sizes.instance);
                maxima.relocations = maxima.relocations.max(sizes.relocations);
                maxima.imports = maxima.imports.max(sizes.imports);
            }
            let mut published: Vec<_> = CHUNK_EXTENSIONS
                .into_iter()
                .zip(outputs)
                .map(|(extension, bytes)| {
                    let name = input.with_extension(extension);
                    (name.file_name().unwrap().to_owned(), Contents::Bytes(bytes))
                })
                .collect();
            let manifest = manifest.encode().map_err(|error| invalid(input, error))?;
            published.push((
                input.file_name().unwrap().to_owned(),
                Contents::Bytes(manifest),
            ));
            published.extend(custom_data_outputs(directory, input, incoming)?);
            Ok(published)
        },
        |_| false,
    )?;
    Ok(count)
}

fn custom_data_outputs(
    directory: &Path,
    input: &Path,
    incoming: Vec<precompiled::Archive>,
) -> io::Result<Vec<Output>> {
    let folder = Path::new(input.file_stem().unwrap()).join("cdata");
    let target = directory.join(&folder);
    let mut prefixes = HashSet::with_capacity(incoming.len());
    let mut copies = Vec::new();
    for archive in incoming {
        prefixes.insert(precompiled::custom_data_prefix(&archive.asset));
        if let Some(source) = archive.custom_data {
            let name = folder.join(precompiled::custom_data_name(&archive.asset));
            copies.push((name.into_os_string(), Contents::Copy(source)));
        }
    }

    // AssetMerger clears any older sidecar for the incoming asset's type and instance.
    let mut outputs = Vec::new();
    match fs::read_dir(&target) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                let path = Path::new(&name);
                if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("cdata"))
                    && name
                        .to_str()
                        .and_then(|name| name.rsplit_once('.'))
                        .and_then(|(stem, _)| stem.rsplit_once('.'))
                        .is_some_and(|(prefix, _)| prefixes.contains(&prefix.to_ascii_uppercase()))
                    && entry.file_type()?.is_file()
                {
                    outputs.push((folder.join(name).into_os_string(), Contents::Remove));
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    outputs.extend(copies);
    Ok(outputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::fast_hash;
    use crate::manifest::{AssetId, ReferencedManifest};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CHECKSUM: u32 = 0x1234_abcd;
    const ANIMATION_TYPE_HASH: u32 = 0x5545_5fe8;
    const STREAM_FILES: [&str; 4] = ["mod.manifest", "mod.bin", "mod.relo", "mod.imp"];

    fn animation_id(name: &str) -> AssetId {
        AssetId {
            type_id: fast_hash(b"W3DAnimation"),
            instance_id: fast_hash(name.as_bytes()),
        }
    }

    struct TestStream(PathBuf);

    impl TestStream {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root =
                std::env::temp_dir().join(format!("bab-merge-{}-{nonce}", std::process::id()));
            fs::create_dir_all(root.join("archives/folder")).unwrap();
            fs::create_dir(root.join("cdata")).unwrap();
            fs::write(root.join("cdata/keep.cdata"), b"custom data").unwrap();
            let chunk_sizes = ChunkSizes {
                instance: 4,
                relocations: 0,
                imports: 0,
            };
            let asset = |name: &'static str, instance: &str| AssetEntry {
                id: animation_id(instance),
                type_hash: ANIMATION_TYPE_HASH,
                instance_hash: 100,
                chunk_sizes,
                name: name.as_bytes(),
                source_file_name: b"DATA:old.xml",
                references: AssetReferences::from_ids(&[]),
                tokenized: Some(0),
            };
            let manifest = Manifest {
                version: 6,
                byte_order: ByteOrder::Little,
                is_linked: true,
                stream_checksum: CHECKSUM,
                all_types_hash: RA3_ALL_TYPES_HASH,
                total_instance_data_size: 8,
                max_chunk_sizes: chunk_sizes,
                assets: vec![
                    asset("W3DAnimation:Keep", "keep"),
                    asset("W3DAnimation:Replace", "replace"),
                ],
                referenced_manifests: vec![ReferencedManifest {
                    path: b"static.manifest",
                    is_patch: false,
                }],
            };
            fs::write(root.join("mod.manifest"), manifest.encode().unwrap()).unwrap();
            fs::write(
                root.join("mod.bin"),
                [CHECKSUM.to_le_bytes(), *b"keep", *b"base"].concat(),
            )
            .unwrap();
            for ext in ["relo", "imp"] {
                fs::write(root.join(format!("mod.{ext}")), CHECKSUM.to_le_bytes()).unwrap();
            }
            Self(root)
        }

        fn archive(&self, name: &str, bytes: &[u8]) {
            let id = animation_id(&name.to_ascii_lowercase());
            let reference = animation_id("keep");
            let name = format!("W3DAnimation:{name}");
            let mut data: Vec<_> = [
                id.type_id,
                id.instance_id,
                ANIMATION_TYPE_HASH,
                231,
                1,
                bytes.len() as u32,
                0,
                0,
                reference.type_id,
                reference.instance_id,
            ]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
            for value in [name.as_bytes(), b"art:incoming.w3x".as_slice()] {
                data.extend((value.len() as u32).to_le_bytes());
                data.extend(value);
            }
            data.extend(bytes);
            fs::write(
                self.0
                    .join("archives/folder")
                    .join(format!("{}.asset", name.replace(':', "."))),
                data,
            )
            .unwrap();
        }
    }

    impl Drop for TestStream {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn merges_overrides_before_untouched_assets() {
        let stream = TestStream::new();
        stream.archive("Replace", b"replaced");
        stream.archive("Add", b"new!");
        let root = &stream.0;
        let input = root.join("mod.manifest");
        let directory = root.join("archives");
        assert_eq!(merge_assets(&input, &directory).unwrap(), 2);
        let raw = fs::read(&input).unwrap();
        let merged = Manifest::parse(&raw).unwrap();
        assert_eq!(
            merged
                .assets
                .iter()
                .map(|asset| asset.id)
                .collect::<Vec<_>>(),
            [
                animation_id("add"),
                animation_id("replace"),
                animation_id("keep"),
            ]
        );
        assert_eq!(
            merged.assets[1].references.iter().collect::<Vec<_>>(),
            [animation_id("keep")]
        );
        assert_eq!(merged.assets[1].instance_hash, 231);
        assert_eq!(merged.assets[1].source_file_name, b"art:incoming.w3x");
        assert_eq!(merged.assets[2].source_file_name, b"DATA:old.xml");
        assert_eq!(merged.referenced_manifests[0].path, b"static.manifest");
        assert_eq!(merged.stream_checksum, CHECKSUM);
        assert_eq!(merged.total_instance_data_size, 16);
        assert_eq!(merged.max_chunk_sizes.instance, 8);
        assert_eq!(
            fs::read(input.with_extension("bin")).unwrap(),
            [&CHECKSUM.to_le_bytes()[..], b"new!replacedkeep"].concat()
        );
        assert_eq!(
            fs::read(root.join("cdata/keep.cdata")).unwrap(),
            b"custom data"
        );
        check_linked_files(&input, &merged).unwrap();
        assert_eq!(merge_assets(&input, &directory).unwrap(), 2);
        assert_eq!(fs::read(&input).unwrap(), raw);
    }

    #[test]
    fn failed_merge_preserves_every_linked_file() {
        let stream = TestStream::new();
        let root = &stream.0;
        let originals = STREAM_FILES.map(|file| fs::read(root.join(file)).unwrap());
        stream.archive("Add", b"new!");
        fs::write(
            root.join("archives/folder/W3DAnimation.Broken.asset"),
            b"bad",
        )
        .unwrap();
        let error = merge_assets(&root.join("mod.manifest"), &root.join("archives")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            STREAM_FILES.map(|file| fs::read(root.join(file)).unwrap()),
            originals
        );
        assert!(!root.join(".rust-streams").exists());
    }

    #[test]
    fn merges_audio_and_replaces_only_its_sidecars() {
        let stream = TestStream::new();
        let root = &stream.0;
        let input = root.join("mod.manifest");
        let directory = root.join("archives");
        let archive = directory.join("folder/AudioFile.Test.asset");
        let name = b"AudioFile:Test";
        let type_id = fast_hash(b"AudioFile");
        let instance_id = fast_hash(b"test");
        let type_hash = 0x53c8_1e47;
        let prefix = format!("{type_id:08X}.{type_hash:08X}.{instance_id:08X}");
        let mut bytes: Vec<_> = [type_id, instance_id, type_hash, 7, 0, 4, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        for value in [name.as_slice(), b"AUDIO:test.xml"] {
            bytes.extend((value.len() as u32).to_le_bytes());
            bytes.extend(value);
        }
        bytes.extend(b"test");
        fs::write(archive, bytes).unwrap();

        let filename = format!("{prefix}.00000007.cdata");
        let source = directory.join("folder").join(filename.to_ascii_lowercase());
        let output = root.join("mod/cdata");
        fs::create_dir_all(&output).unwrap();
        fs::write(&source, b"encoded audio").unwrap();
        fs::write(directory.join(&filename), b"outside sibling directory").unwrap();
        fs::write(output.join(&filename), b"previous copy").unwrap();
        let stale = output.join(format!("{prefix}.00000006.cdata"));
        fs::write(&stale, b"older version").unwrap();
        fs::write(output.join("keep.cdata"), b"unrelated").unwrap();

        assert_eq!(merge_assets(&input, &directory).unwrap(), 1);
        let raw = fs::read(&input).unwrap();
        let manifest = Manifest::parse(&raw).unwrap();
        assert_eq!(manifest.assets[0].name, name);
        assert_eq!(manifest.assets[0].instance_hash, 7);
        assert_eq!(manifest.assets[0].tokenized, Some(0));
        check_linked_files(&input, &manifest).unwrap();
        assert_eq!(fs::read(output.join(&filename)).unwrap(), b"encoded audio");
        assert_eq!(fs::read(output.join("keep.cdata")).unwrap(), b"unrelated");
        assert!(!stale.exists());

        fs::remove_file(source).unwrap();
        assert_eq!(merge_assets(&input, &directory).unwrap(), 1);
        assert_eq!(fs::read(&input).unwrap(), raw);
        assert!(!output.join(&filename).exists());
        assert_eq!(fs::read(output.join("keep.cdata")).unwrap(), b"unrelated");
    }
}
