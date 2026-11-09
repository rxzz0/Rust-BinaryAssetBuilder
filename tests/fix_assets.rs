use binary_asset_builder::fix_assets::fix_assets;
use binary_asset_builder::linked_stream::check_linked_files;
use binary_asset_builder::manifest::{
    AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest,
};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn fixes_override_hashes_and_reorders_chunks_before_publishing_lods() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("bab-fix-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let input = root.join("mod.manifest");
    let error = fix_assets(&input, &root).err().unwrap();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error.to_string().contains("Build Static Data"));

    let dependency = AssetId {
        type_id: 1,
        instance_id: 1,
    };
    let consumer = AssetId {
        type_id: 2,
        instance_id: 2,
    };
    let chunks = ChunkSizes {
        instance: 4,
        relocations: 0,
        imports: 0,
    };
    let dependency_entry = AssetEntry {
        id: dependency,
        type_hash: 100,
        instance_hash: 101,
        chunk_sizes: chunks,
        name: b"Type1:Dependency",
        source_file_name: b"DATA:mod.xml",
        references: AssetReferences::from_ids(&[]),
        tokenized: Some(0),
    };
    let worldbuilder = Manifest {
        version: 6,
        byte_order: ByteOrder::Little,
        is_linked: true,
        stream_checksum: 0x1234_abcd,
        all_types_hash: 0x54ee_e764,
        total_instance_data_size: 4,
        max_chunk_sizes: chunks,
        assets: vec![AssetEntry {
            instance_hash: 102,
            ..dependency_entry.clone()
        }],
        referenced_manifests: Vec::new(),
    };
    fs::write(
        root.join("worldbuilder.manifest"),
        worldbuilder.encode().unwrap(),
    )
    .unwrap();
    let references = [dependency];
    let mut mod_stream = Manifest {
        total_instance_data_size: 8,
        assets: vec![
            AssetEntry {
                id: consumer,
                name: b"Type2:Consumer",
                references: AssetReferences::from_ids(&references),
                ..dependency_entry.clone()
            },
            dependency_entry,
        ],
        ..worldbuilder
    };
    let checksum = mod_stream.stream_checksum.to_le_bytes();
    let raw = mod_stream.encode().unwrap();
    fs::write(&input, &raw).unwrap();
    fs::write(
        input.with_extension("bin"),
        [checksum, *b"used", *b"base"].concat(),
    )
    .unwrap();
    for ext in ["imp", "relo"] {
        fs::write(input.with_extension(ext), checksum).unwrap();
    }

    let result = fix_assets(&input, &root).unwrap();
    assert_eq!(result.assets, 2);
    assert_eq!(result.hashes_fixed, 1);
    let base = fs::read(&input).unwrap();
    let manifest = Manifest::parse(&base).unwrap();
    assert_eq!(manifest.assets[0].id, dependency);
    assert_eq!(manifest.assets[0].instance_hash, 102);
    assert_eq!(manifest.assets[1].id, consumer);
    assert_eq!(
        fs::read(input.with_extension("bin")).unwrap(),
        [checksum, *b"base", *b"used"].concat()
    );
    for stem in ["mod_l", "mod_m"] {
        let patch_path = root.join(format!("{stem}.manifest"));
        let patch = fs::read(&patch_path).unwrap();
        let patch = Manifest::parse(&patch).unwrap();
        assert!(patch.referenced_manifests[0].is_patch);
        assert_eq!(patch.referenced_manifests[0].path, b"mod.manifest");
        assert_eq!(patch.assets[0].instance_hash, 102);
        assert_eq!(patch.assets[0].chunk_sizes.instance, 0);
        check_linked_files(&patch_path, &patch).unwrap();
    }
    assert_eq!(fix_assets(&input, &root).unwrap().hashes_fixed, 0);

    let missing = [AssetId {
        type_id: 77,
        instance_id: 78,
    }];
    mod_stream.assets[0].references = AssetReferences::from_ids(&missing);
    let invalid = mod_stream.encode().unwrap();
    fs::write(&input, &invalid).unwrap();
    let patch_path = root.join("mod_l.manifest");
    let patch_before = fs::read(&patch_path).unwrap();
    assert!(
        fix_assets(&input, &root)
            .err()
            .unwrap()
            .to_string()
            .contains("unresolved asset")
    );
    assert_eq!(fs::read(&input).unwrap(), invalid);
    assert_eq!(fs::read(patch_path).unwrap(), patch_before);
    fs::remove_dir_all(root).unwrap();
}
