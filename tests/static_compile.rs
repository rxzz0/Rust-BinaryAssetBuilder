use binary_asset_builder::{
    compile::{compile_sdk_global, compile_sdk_static, compile_static},
    manifest::{AssetEntry, AssetId, AssetReferences, ByteOrder, ChunkSizes, Manifest},
};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("bab-static-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn sdk_static_builds_without_an_ea_compiler_and_preserves_outputs_on_failure() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data");
    let built = root.join("builtmods");
    fs::create_dir_all(&data).unwrap();
    let input = data.join("Mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="example" Filename="example" />
        </AssetDeclaration>"#,
    )
    .unwrap();

    compile_sdk_static(&input, &root, &built).unwrap();
    let output = built.join("mods/example/data");
    let files = [
        "mod.manifest",
        "mod.bin",
        "mod.relo",
        "mod.imp",
        "mod.version",
    ];
    let expected = files.map(|name| fs::read(output.join(name)).unwrap());
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Unsupported id="example" />
        </AssetDeclaration>"#,
    )
    .unwrap();
    let error = compile_sdk_static(&input, &root, &built).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported static asset type Unsupported")
    );
    assert_eq!(
        files.map(|name| fs::read(output.join(name)).unwrap()),
        expected
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sdk_global_compiles_multiplayer_colors_with_the_sdk_layout() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data/AdditionalMaps");
    let built = root.join("builtmods");
    fs::create_dir_all(&data).unwrap();
    fs::write(
        data.join("MapMetaData_Colors.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <MultiplayerColor id="ColorPurple">
                <RGBColor R="125" G="25" B="200" A="255"/>
                <RGBNightColor R="125" G="25" B="200" A="255"/>
                <TooltipName>Color:Purple</TooltipName>
                <AvailableInMetaGame>true</AvailableInMetaGame>
            </MultiplayerColor>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(data.join("Unrelated.xml"), "not an asset stream").unwrap();
    compile_sdk_global(&data, &root, &built).unwrap();
    let output = built.join("mods/example/data/additionalmaps");
    let manifest = fs::read(output.join("mapmetadata_colors.manifest")).unwrap();
    let manifest = Manifest::parse(&manifest).unwrap();
    assert_eq!(manifest.assets[0].instance_hash, 0xa678_aae6);
    let binary = fs::read(output.join("mapmetadata_colors.bin")).unwrap();
    assert_eq!(
        &binary[4..],
        b"\0\0\0\0\xc8\x19\x7d\xff\0\0\0\0\xc8\x19\x7d\xff\x0c\0\0\0\x1c\0\0\0\x01\0\0\0Color:Purple\0\0\0\0"
    );
    let relocations = fs::read(output.join("mapmetadata_colors.relo")).unwrap();
    assert_eq!(&relocations[4..], &[20, 0, 0, 0, 255, 255, 255, 255]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolves_sdk_art_buckets_and_mod_audio_paths() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data");
    let art = root.join("Mods/Example/Art/bo");
    let audio = root.join("Mods/Example/Audio/vo");
    for directory in [&data, &art, &audio] {
        fs::create_dir_all(directory).unwrap();
    }
    let input = data.join("Mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes>
                <Include type="all" source="ART:box.w3x"/>
                <Include type="all" source="Audio:vo/dialog.xml"/>
            </Includes>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        art.join("box.w3x"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <W3DCollisionBox id="box">
                <Center X="0" Y="0" Z="0"/>
                <Extent X="1" Y="1" Z="1"/>
            </W3DCollisionBox>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        audio.join("dialog.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="dialog" Filename="dialog" />
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    assert_eq!(
        manifest.assets[0].source_file_name,
        b"DATA:mods/example/audio/vo/dialog.xml"
    );
    assert_eq!(
        manifest.assets[1].source_file_name,
        b"DATA:mods/example/art/bo/box.w3x"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn emits_only_referenced_assets_from_instance_includes() {
    let root = scratch_root();
    let input = root.join("mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="instance" source="library.xml"/></Includes>
            <VideoEventList id="intro">
                <EventList><Movie MovieAsset="selected"/></EventList>
            </VideoEventList>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        root.join("library.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="all" source="videos.xml"/></Includes>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        root.join("videos.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="selected" Filename="test"/>
            <Video id="unused" Filename="test"/>
            <Unsupported id="unused"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [video, events] = manifest.assets.as_slice() else {
        panic!("expected the video event and its referenced video");
    };
    assert_eq!(video.name, b"Video:selected");
    assert_eq!(video.source_file_name, b"DATA:videos.xml");
    assert_eq!(events.references.iter().next(), Some(video.id));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_hotkeys_with_sdk_slot_order_and_import_indices() {
    let root = scratch_root();
    let input = root.join("hotkeys.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <HotKeySlot id="Slot10" Name="Slot10"/>
            <HotKeySlot id="Slot1_Mod" Name="Slot1"/>
            <MappableKey id="KEY_F1" KeyDef="KEY_F1"/>
            <DefaultHotKeys id="Default">
                <Map><HotKey Slot="Slot1_Mod" Key="KEY_F1" Modifiers="ALT"/></Map>
            </DefaultHotKeys>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [first, second, key, defaults] = manifest.assets.as_slice() else {
        panic!("expected two slots, a key and a default hotkey map");
    };
    assert_eq!(first.name, b"HotKeySlot:Slot1_Mod");
    assert_eq!(second.name, b"HotKeySlot:Slot10");
    assert_eq!(
        defaults.references.iter().collect::<Vec<_>>(),
        [first.id, key.id]
    );
    let start = stream.files.bin.len() - defaults.chunk_sizes.instance as usize;
    let words = stream.files.bin[start..].as_chunks::<4>().0;
    assert_eq!(words, [0_u32, 1, 12, 1, 2, 2].map(u32::to_le_bytes));
    assert_eq!(
        &stream.files.imp[4..],
        [12_u32, 16, u32::MAX].map(u32::to_le_bytes).as_flattened()
    );
    fs::remove_dir_all(root).unwrap();
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

#[test]
fn compiles_w3d_hierarchy_with_hashed_pivot_names() {
    let root = scratch_root();
    let xml = root.join("hierarchy.w3x");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <W3DHierarchy id="Example">
                <Pivot Name="ROOTTRANSFORM" Parent="-1">
                    <Translation X="2" Y="3" Z="4"/>
                    <Rotation X="0" Y="0" Z="0" W="1"/>
                    <FixupMatrix M00="1" M11="1" M22="1" M30="5"/>
                </Pivot>
            </W3DHierarchy>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let result = compile_static(&xml, &root, &root);
    fs::remove_file(xml).unwrap();
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets[0].type_hash, 0xed94_de61);
    assert_eq!(manifest.assets[0].chunk_sizes.instance, 112);
    let words: Vec<_> = stream.files.bin[4..]
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_le_bytes)
        .collect();
    assert_eq!(&words[..5], &[0, 1, 12, 0xcef3_087b, u32::MAX]);
    assert_eq!(
        &words[5..8],
        &[2_f32.to_bits(), 3_f32.to_bits(), 4_f32.to_bits()]
    );
    assert_eq!(words[11], 1_f32.to_bits());
    assert_eq!(words[15], 5_f32.to_bits());
    assert_eq!(&stream.files.relo[4..], &[8, 0, 0, 0, 255, 255, 255, 255]);
    assert_eq!(stream.files.imp.len(), 4);
}
