use binary_asset_builder::manifest::{
    AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest, ManifestError, ReferencedManifest,
};

// Generated with the upstream C# v5 ManifestHeader, AssetEntry and buffer writers.
const LITTLE_ENDIAN: &[u8] = include_bytes!("fixtures/manifest-v5-le.manifest");
const BIG_ENDIAN: &[u8] = include_bytes!("fixtures/manifest-v5-be.manifest");

#[test]
fn reads_upstream_manifests_in_both_byte_orders() {
    for (bytes, byte_order, is_linked) in [
        (LITTLE_ENDIAN, ByteOrder::Little, true),
        (BIG_ENDIAN, ByteOrder::Big, false),
    ] {
        let manifest = Manifest::parse(bytes).unwrap();
        assert_eq!(manifest.byte_order, byte_order);
        assert_eq!(manifest.is_linked, is_linked);
        assert_eq!(manifest.stream_checksum, 0x1234_5678);
        assert_eq!(manifest.all_types_hash, 0x90ab_cdef);
        assert_eq!(manifest.total_instance_data_size, 24);
        assert_eq!(
            manifest.max_chunk_sizes,
            ChunkSizes {
                instance: 16,
                relocations: 8,
                imports: 4,
            }
        );
        assert_eq!(
            manifest.referenced_manifests,
            [
                ReferencedManifest {
                    path: b"base.manifest",
                    is_patch: true,
                },
                ReferencedManifest {
                    path: b"common\\ui.manifest",
                    is_patch: false,
                }
            ]
        );

        let [first, second] = manifest.assets.as_slice() else {
            panic!("expected two asset entries");
        };
        assert_eq!(
            first.id,
            AssetId {
                type_id: 0x942f_ff2d,
                instance_id: 0x509b_d329,
            }
        );
        assert_eq!(first.type_hash, 0x1324_08db);
        assert_eq!(first.instance_hash, 0x1357_9bdf);
        assert_eq!(first.chunk_sizes, manifest.max_chunk_sizes);
        assert_eq!(first.name, b"GameObject:AlliedBarracks");
        assert_eq!(first.source_file_name, b"data\\caf\xe9.xml");
        assert_eq!(
            first.references.iter().collect::<Vec<_>>(),
            [
                AssetId {
                    type_id: 0x1122_3344,
                    instance_id: 0x5566_7788,
                },
                AssetId {
                    type_id: 0x0102_0304,
                    instance_id: 0x0506_0708,
                },
            ]
        );
        assert_eq!(second.name, b"LogicCommand:Build");
        assert_eq!(second.source_file_name, first.source_file_name);
        assert_eq!(second.references.iter().len(), 0);
    }
}

#[test]
fn rejects_truncated_sections_and_unterminated_names() {
    for len in 0..LITTLE_ENDIAN.len() {
        assert!(Manifest::parse(&LITTLE_ENDIAN[..len]).is_err(), "{len}");
    }

    let mut bytes = LITTLE_ENDIAN.to_vec();
    *bytes.last_mut().unwrap() = b'x';
    assert_eq!(
        Manifest::parse(&bytes).unwrap_err(),
        ManifestError::Invalid("source file name"),
    );
}

#[test]
fn rejects_unsupported_versions_and_invalid_reference_ranges() {
    let mut bytes = BIG_ENDIAN.to_vec();
    bytes[3] = 6;
    assert_eq!(
        Manifest::parse(&bytes).unwrap_err(),
        ManifestError::UnsupportedVersion(6),
    );

    let mut bytes = LITTLE_ENDIAN.to_vec();
    let first_reference_count = 48 + 5 * 4;
    bytes[first_reference_count..first_reference_count + 4].copy_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        Manifest::parse(&bytes).unwrap_err(),
        ManifestError::Invalid("asset reference range"),
    );
}

#[test]
fn writes_byte_identical_upstream_manifests() {
    for bytes in [LITTLE_ENDIAN, BIG_ENDIAN] {
        let mut manifest = Manifest::parse(bytes).unwrap();
        assert_eq!(manifest.encode().unwrap(), bytes);

        let decoded_references = manifest.assets[0].references.iter().collect::<Vec<_>>();
        manifest.assets[0].references = AssetReferences::from_ids(&decoded_references);
        assert_eq!(manifest.encode().unwrap(), bytes);
    }
}

#[test]
fn rejects_names_and_sizes_that_cannot_be_encoded() {
    let mut manifest = Manifest::parse(LITTLE_ENDIAN).unwrap();
    manifest.assets[0].name = b"Asset\0Other";
    assert_eq!(
        manifest.encode().unwrap_err(),
        ManifestError::Invalid("asset name"),
    );

    manifest.assets[0].name = b"Asset";
    manifest.assets[0].chunk_sizes.instance = u32::MAX;
    assert_eq!(
        manifest.encode().unwrap_err(),
        ManifestError::Invalid("instance data size"),
    );
}
