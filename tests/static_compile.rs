use binary_asset_builder::{
    compile::compile_static,
    manifest::{AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest},
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("bab-static-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    root
}

#[test]
fn compiles_video_with_the_ra3_sdk_hash_and_binary_layout() {
    let root = scratch_root();
    let xml = root.join("video.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
    <Video id="ui_archemaiden" Filename="ui_archemaiden" Volume="105" />
</AssetDeclaration>"#,
    )
    .unwrap();
    let result = compile_static(&xml, &root, &root);
    fs::remove_file(xml).unwrap();
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [asset] = manifest.assets.as_slice() else {
        panic!("expected one compiled Video asset");
    };
    // The EA SDK compiled this same declaration with these IDs and instance hash.
    assert_eq!(asset.id.type_id, 0x2302_36f6);
    assert_eq!(asset.id.instance_id, 0x7d92_3ff4);
    assert_eq!(asset.type_hash, 0x8433_efa6);
    assert_eq!(asset.instance_hash, 0x1ced_a7e2);
    assert_eq!(asset.chunk_sizes.instance, 56);
    assert_eq!(
        &stream.files.bin[4 + 28..],
        b"ui_archemaiden\0\0No Comment\0\0"
    );
    assert_eq!(
        &stream.files.relo[4..],
        &[8, 0, 0, 0, 16, 0, 0, 0, 255, 255, 255, 255]
    );
    assert_eq!(stream.files.imp.len(), 4);
}

#[test]
fn emits_the_sdk_external_audio_type_and_local_movie_import() {
    let root = scratch_root();
    let xml = root.join("sample.xml");
    let reference = root.join("baseline.xml");
    let manifest_path = root.join("baseline.manifest");
    fs::write(
        &reference,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset" />"#,
    )
    .unwrap();
    fs::write(&xml, r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <Includes><Include type="reference" source="DATA:baseline.xml" /></Includes>
        <VideoEventList id="ui_archemaiden"><EventList><Movie MovieAsset="ui_archemaiden" /></EventList></VideoEventList>
        <UnitIntro id="Japan_ArcherMaiden_UnitIntro" Video="ui_archemaiden" Audio="MIIn_ArcherMaiden" />
        <Video id="ui_archemaiden" Filename="ui_archemaiden" Volume="105" />
    </AssetDeclaration>"#).unwrap();

    let missing_reference = compile_static(&xml, &root, &root).err().unwrap();
    assert_eq!(missing_reference.kind(), std::io::ErrorKind::NotFound);

    let sizes = ChunkSizes {
        instance: 0,
        relocations: 0,
        imports: 0,
    };
    let base = Manifest {
        version: 6,
        byte_order: ByteOrder::Little,
        is_linked: true,
        stream_checksum: 0,
        all_types_hash: 0x54ee_e764,
        total_instance_data_size: 0,
        max_chunk_sizes: sizes,
        assets: vec![AssetEntry {
            id: AssetId {
                type_id: 0x844d_7b9f,
                instance_id: 0xface_2fcc,
            },
            type_hash: 0,
            instance_hash: 0,
            chunk_sizes: sizes,
            name: b"AudioEvent:MIIn_ArcherMaiden",
            source_file_name: b"DATA:baseline.xml",
            references: AssetReferences::from_ids(&[]),
            tokenized: Some(0),
        }],
        referenced_manifests: Vec::new(),
    };
    fs::write(&manifest_path, base.encode().unwrap()).unwrap();
    let result = compile_static(&xml, &root, &root);
    for path in [xml, reference, manifest_path] {
        fs::remove_file(path).unwrap();
    }
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.referenced_manifests[0].path, b"baseline.manifest");
    let [video, intro, events] = manifest.assets.as_slice() else {
        panic!("expected three compiled static assets");
    };
    assert_eq!(intro.instance_hash, 0x4be3_7748);
    assert_eq!(
        intro.references.iter().next(),
        Some(AssetId {
            type_id: 0x4053_c714,
            instance_id: 0xface_2fcc,
        })
    );
    assert_eq!(events.instance_hash, 0x279d_e4c1);
    assert_eq!(events.references.iter().next(), Some(video.id));
    assert_eq!(
        &stream.files.imp[4..],
        &[
            8, 0, 0, 0, 255, 255, 255, 255, 28, 0, 0, 0, 255, 255, 255, 255
        ]
    );
}
#[test]
fn compiles_ai_budget_with_the_sdk_relocation_order() {
    let root = scratch_root();
    let xml = root.join("budget.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <AIBudgetStateDefinition id="NewProductionDefenseBudget">
                <Heuristic><ConstantHeuristic/></Heuristic>
                <Budget>
                    <AccountShare Account="PRODUCTION" Percent="90%"/>
                    <AccountShare Account="DEFENSE" Percent="10%"/>
                </Budget>
            </AIBudgetStateDefinition>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let result = compile_static(&xml, &root, &root);
    fs::remove_file(xml).unwrap();
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let asset = &manifest.assets[0];
    assert_eq!(asset.type_hash, 0x99ff_1736);
    assert_eq!(asset.instance_hash, 0x2c94_a214);
    assert_eq!(asset.chunk_sizes.instance, 48);
    let expected: Vec<_> = [16, 8, 36, u32::MAX]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert_eq!(&stream.files.relo[4..], expected);
}

#[test]
fn compiles_w3d_collision_box_with_the_sdk_layout() {
    let root = scratch_root();
    let xml = root.join("box.w3x");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <W3DCollisionBox id="AUGUNSHIP_COL.OBBOX01">
                <Center X="7.289608" Y="-0.024450" Z="5.834013"/>
                <Extent X="22.339098" Y="6.380490" Z="5.834013"/>
            </W3DCollisionBox>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let result = compile_static(&xml, &root, &root);
    fs::remove_file(xml).unwrap();
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let asset = &manifest.assets[0];
    assert_eq!(asset.id.type_id, 0xe318_1c04);
    assert_eq!(asset.type_hash, 0x6e26_f699);
    assert_eq!(asset.chunk_sizes.instance, 32);
    let vectors = [
        7.289_608_f32,
        -0.024_450,
        5.834_013,
        22.339_098,
        6.380_49,
        5.834_013,
    ];
    let expected: Vec<_> = [0; 4]
        .into_iter()
        .chain(vectors.into_iter().flat_map(f32::to_le_bytes))
        .chain([0; 4])
        .collect();
    assert_eq!(&stream.files.bin[4..], expected);
    assert_eq!(stream.files.relo.len(), 4);
    assert_eq!(stream.files.imp.len(), 4);
}
