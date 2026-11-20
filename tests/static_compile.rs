use binary_asset_builder::{
    compile::{
        CustomData, compile_map, compile_sdk_global, compile_sdk_maps, compile_sdk_static,
        compile_static,
    },
    linked_stream::check_linked_files,
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
fn inherited_logic_command_sets_reach_reference_validation() {
    let root = scratch_root();
    fs::create_dir_all(root.join("Schemas/xsd")).unwrap();
    fs::write(
        root.join("Schemas/xsd/CnC3Types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
          <xs:complexType name="BaseInheritableAsset"/>
          <xs:complexType name="LogicCommandSet"><xs:complexContent><xs:extension base="BaseInheritableAsset">
            <xs:sequence><xs:element name="Cmd" type="xs:string" maxOccurs="unbounded"/></xs:sequence>
          </xs:extension></xs:complexContent></xs:complexType>
        </xs:schema>"#,
    )
    .unwrap();
    let xml = root.join("commands.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
      <LogicCommandSet id="Base"><Cmd>First</Cmd></LogicCommandSet>
      <LogicCommandSet id="Child" inheritFrom="Base"><Cmd>Second</Cmd></LogicCommandSet>
    </AssetDeclaration>"#,
    )
    .unwrap();
    let error = compile_static(&xml, &root, &root)
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("unresolved reference") && error.contains("LogicCommandSet"),
        "{error}"
    );
}

#[test]
fn ai_inheritance_applies_remove_and_replace_to_named_states() {
    let root = scratch_root();
    let input = root.join("ai.xml");
    fs::write(root.join("base.xml"), r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset"><AIPersonalityDefinition id="Base" PersonalityType="Base"><StrategicState id="RemoveMe" State="NoSuchState"/><StrategicState id="ReplaceMe" State="OtherMissingState" Difficulty="EASY"/><CountAsStructureForBuildPurposesFilter/></AIPersonalityDefinition></AssetDeclaration>"#).unwrap();
    fs::write(&input, r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset" xmlns:xai="uri:ea.com:eala:asset:instance"><Includes><Include type="instance" source="base.xml"/></Includes><AIPersonalityDefinition id="Derived" PersonalityType="Derived" inheritFrom="Base"><StrategicState id="RemoveMe" xai:joinAction="Remove"/><StrategicState id="ReplaceMe" xai:joinAction="Replace" State="Valid"/></AIPersonalityDefinition><AIStrategicStateDefinition id="Valid"><Heuristic><ConstantHeuristic Weight="1"/></Heuristic></AIStrategicStateDefinition></AssetDeclaration>"#).unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let asset = manifest
        .assets
        .iter()
        .find(|asset| asset.name == b"AIPersonalityDefinition:Derived")
        .unwrap();
    assert_eq!(asset.references.iter().len(), 1);
    let offset = 4 + manifest
        .assets
        .iter()
        .take_while(|other| other.id != asset.id)
        .map(|other| other.chunk_sizes.instance as usize)
        .sum::<usize>();
    let word = |i: usize| {
        u32::from_le_bytes(
            stream.files.bin[offset + i..offset + i + 4]
                .try_into()
                .unwrap(),
        )
    };
    assert_eq!(word(340), 1);
    assert_eq!(word(word(344) as usize + 8), 15);
    fs::remove_file(input).unwrap();
    fs::remove_file(root.join("base.xml")).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn embedded_dds_texture_matches_ea_payload_and_relocations() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/on-demand-texture");
    let stream = compile_static(&root.join("texture-probe.xml"), &root, &root).unwrap();
    let oracle = include_bytes!("fixtures/on-demand-texture/texture-probe.asset");
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].type_hash, 0x9f5f_f8da);
    let split = oracle.len() - 8;
    assert_eq!(&stream.files.bin[4..], &oracle[32..split]);
    assert_eq!(&stream.files.relo[4..], &oracle[split..]);
}

#[test]
fn on_demand_texture_sidecar_matches_ea_dds() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/on-demand-texture");
    let stream = compile_static(&root.join("on-demand-convert.xml"), &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    assert_eq!(&stream.files.bin[4..], &[0; 16]);
    for asset in &manifest.assets {
        let oracle: &[u8] = if asset.name == b"OnDemandTexture:ProbeToDxt" {
            include_bytes!("fixtures/on-demand-texture/dxt.cdata")
        } else {
            assert_eq!(asset.name, b"OnDemandTexture:ProbeToRgba");
            include_bytes!("fixtures/on-demand-texture/rgba.cdata")
        };
        let prefix = format!(
            "{:08x}.{:08x}.{:08x}.",
            asset.id.type_id, asset.type_hash, asset.id.instance_id
        );
        let (_, custom) = stream
            .custom_data
            .iter()
            .find(|(name, _)| name.starts_with(&prefix))
            .unwrap();
        let CustomData::Bytes(data) = custom else {
            panic!("expected DDS sidecar")
        };
        assert_eq!(&data[..12], &oracle[..12]);
        assert_eq!(&data[16..], &oracle[16..]);
        assert_eq!(&data[12..16], &asset.instance_hash.to_le_bytes());
    }
}

#[test]
fn audio_file_pcm_payload_and_cdata_match_ea() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio-pcm");
    let stream = compile_static(&root.join("audio-file-pcm.xml"), &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    let mut offset = 4;
    let mut relocation = 4;
    for entry in &manifest.assets {
        let (asset, cdata): (&[u8], &[u8]) = if entry.name == b"AudioFile:PcmRamProbe" {
            (
                include_bytes!("fixtures/audio-pcm/ram.asset"),
                include_bytes!("fixtures/audio-pcm/ram.cdata"),
            )
        } else {
            assert_eq!(entry.name, b"AudioFile:PcmStreamProbe");
            (
                include_bytes!("fixtures/audio-pcm/streamed.asset"),
                include_bytes!("fixtures/audio-pcm/streamed.cdata"),
            )
        };
        let payload_len = entry.chunk_sizes.instance as usize;
        let relo_len = entry.chunk_sizes.relocations as usize;
        assert_eq!(
            &stream.files.bin[offset..offset + payload_len],
            &asset[32..32 + payload_len]
        );
        assert_eq!(
            &stream.files.relo[relocation..relocation + relo_len],
            &asset[32 + payload_len..]
        );
        let prefix = format!(
            "{:08x}.{:08x}.{:08x}.",
            entry.id.type_id, entry.type_hash, entry.id.instance_id
        );
        let (_, custom) = stream
            .custom_data
            .iter()
            .find(|(name, _)| name.starts_with(&prefix))
            .unwrap();
        let CustomData::Bytes(bytes) = custom else {
            panic!("expected generated PCM")
        };
        assert_eq!(bytes, cdata);
        offset += payload_len;
        relocation += relo_len;
    }
}

#[test]
fn game_object_minimal_matches_ea_oracle() {
    for (name, xml, expected_manifest, bin, relo, imp) in [
        (
            "building.xml",
            include_bytes!("fixtures/game-object-building.xml").as_slice(),
            include_bytes!("fixtures/game-object-building.manifest").as_slice(),
            include_bytes!("fixtures/game-object-building.bin").as_slice(),
            include_bytes!("fixtures/game-object-building.relo").as_slice(),
            include_bytes!("fixtures/game-object-building.imp").as_slice(),
        ),
        (
            "unpack.xml",
            include_bytes!("fixtures/game-object-unpack.xml").as_slice(),
            include_bytes!("fixtures/game-object-unpack.manifest").as_slice(),
            include_bytes!("fixtures/game-object-unpack.bin").as_slice(),
            include_bytes!("fixtures/game-object-unpack.relo").as_slice(),
            include_bytes!("fixtures/game-object-unpack.imp").as_slice(),
        ),
        (
            "sell.xml",
            include_bytes!("fixtures/game-object-sell.xml").as_slice(),
            include_bytes!("fixtures/game-object-sell.manifest").as_slice(),
            include_bytes!("fixtures/game-object-sell.bin").as_slice(),
            include_bytes!("fixtures/game-object-sell.relo").as_slice(),
            include_bytes!("fixtures/game-object-sell.imp").as_slice(),
        ),
        (
            "physics.xml",
            include_bytes!("fixtures/game-object-physics.xml").as_slice(),
            include_bytes!("fixtures/game-object-physics.manifest").as_slice(),
            include_bytes!("fixtures/game-object-physics.bin").as_slice(),
            include_bytes!("fixtures/game-object-physics.relo").as_slice(),
            include_bytes!("fixtures/game-object-physics.imp").as_slice(),
        ),
        (
            "bare.xml",
            include_bytes!("fixtures/game-object-bare.xml").as_slice(),
            include_bytes!("fixtures/game-object-bare.manifest").as_slice(),
            include_bytes!("fixtures/game-object-bare.bin").as_slice(),
            include_bytes!("fixtures/game-object-bare.relo").as_slice(),
            include_bytes!("fixtures/game-object-bare.imp").as_slice(),
        ),
        (
            "probe.xml",
            include_bytes!("fixtures/game-object-minimal.xml").as_slice(),
            include_bytes!("fixtures/game-object-minimal.manifest").as_slice(),
            include_bytes!("fixtures/game-object-minimal.bin").as_slice(),
            include_bytes!("fixtures/game-object-minimal.relo").as_slice(),
            include_bytes!("fixtures/game-object-minimal.imp").as_slice(),
        ),
        (
            "variant.xml",
            include_bytes!("fixtures/game-object-variant.xml").as_slice(),
            include_bytes!("fixtures/game-object-variant.manifest").as_slice(),
            include_bytes!("fixtures/game-object-variant.bin").as_slice(),
            include_bytes!("fixtures/game-object-variant.relo").as_slice(),
            include_bytes!("fixtures/game-object-variant.imp").as_slice(),
        ),
        (
            "default.xml",
            include_bytes!("fixtures/game-object-default-health.xml").as_slice(),
            include_bytes!("fixtures/game-object-default-health.manifest").as_slice(),
            include_bytes!("fixtures/game-object-default-health.bin").as_slice(),
            include_bytes!("fixtures/game-object-default-health.relo").as_slice(),
            include_bytes!("fixtures/game-object-default-health.imp").as_slice(),
        ),
        (
            "active.xml",
            include_bytes!("fixtures/game-object-active-body.xml").as_slice(),
            include_bytes!("fixtures/game-object-active-body.manifest").as_slice(),
            include_bytes!("fixtures/game-object-active-body.bin").as_slice(),
            include_bytes!("fixtures/game-object-active-body.relo").as_slice(),
            include_bytes!("fixtures/game-object-active-body.imp").as_slice(),
        ),
        (
            "fields.xml",
            include_bytes!("fixtures/game-object-fields-all.xml").as_slice(),
            include_bytes!("fixtures/game-object-fields-all.manifest").as_slice(),
            include_bytes!("fixtures/game-object-fields-all.bin").as_slice(),
            include_bytes!("fixtures/game-object-fields-all.relo").as_slice(),
            include_bytes!("fixtures/game-object-fields-all.imp").as_slice(),
        ),
        (
            "scalar-fields.xml",
            include_bytes!("fixtures/game-object-scalar-fields.xml").as_slice(),
            include_bytes!("fixtures/game-object-scalar-fields.manifest").as_slice(),
            include_bytes!("fixtures/game-object-scalar-fields.bin").as_slice(),
            include_bytes!("fixtures/game-object-scalar-fields.relo").as_slice(),
            include_bytes!("fixtures/game-object-scalar-fields.imp").as_slice(),
        ),
        (
            "smc.xml",
            include_bytes!("fixtures/game-object-behavior-basic-smc.xml").as_slice(),
            include_bytes!("fixtures/game-object-behavior-basic-smc.manifest").as_slice(),
            include_bytes!("fixtures/game-object-behavior-basic-smc.bin").as_slice(),
            include_bytes!("fixtures/game-object-behavior-basic-smc.relo").as_slice(),
            include_bytes!("fixtures/game-object-behavior-basic-smc.imp").as_slice(),
        ),
    ] {
        let root = scratch_root();
        let input = root.join(name);
        fs::write(&input, xml).unwrap();
        let stream = compile_static(&input, &root, &root).unwrap();
        let manifest = Manifest::parse(&stream.manifest).unwrap();
        let oracle = Manifest::parse(expected_manifest).unwrap();
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(stream.files.bin.len(), bin.len(), "{name} binary length");
        assert_eq!(
            stream
                .files
                .bin
                .iter()
                .zip(bin)
                .position(|(actual, expected)| actual != expected),
            None,
            "{name} first binary mismatch"
        );
        assert_eq!(stream.files.relo, relo);
        assert_eq!(stream.files.imp, imp);
        assert_eq!(manifest.assets[0].type_hash, oracle.assets[0].type_hash);
        assert_eq!(
            manifest.assets[0].instance_hash,
            oracle.assets[0].instance_hash
        );
        assert_eq!(manifest.assets[0].tokenized, oracle.assets[0].tokenized);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn game_object_rejects_nonfinite_health() {
    let root = scratch_root();
    let input = root.join("bad-health.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset"><GameObject id="BadHealth"><Body><ImmortalBody id="ModuleTag_Body" MaxHealth="NaN" /></Body></GameObject></AssetDeclaration>"#,
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(error.to_string().contains("invalid MaxHealth"));
    fs::remove_dir_all(root).unwrap();
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
fn sdk_schema_reports_required_errors_and_keeps_building_after_optional_warnings() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data");
    let built = root.join("builtmods");
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&schemas).unwrap();
    fs::write(
        schemas.join("CnC3Types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:element name="AssetDeclaration"><xs:complexType><xs:choice>
                <xs:element name="GameObject" type="GameObject"/>
            </xs:choice></xs:complexType></xs:element>
            <xs:complexType name="GameObject"><xs:sequence>
                <xs:element name="Body" type="BodyModule" minOccurs="1"/>
                <xs:element name="Behaviors" type="BehaviorModules" minOccurs="0"/>
                <xs:element name="AI" type="BehaviorModules" minOccurs="0"/>
            </xs:sequence></xs:complexType>
            <xs:complexType name="BodyModule"><xs:choice>
                <xs:element name="ActiveBody" type="ActiveBodyModuleData"/>
            </xs:choice></xs:complexType>
            <xs:complexType name="BehaviorModules"><xs:choice minOccurs="0" maxOccurs="unbounded">
                <xs:element name="Simple" type="SimpleModuleData"/>
            </xs:choice></xs:complexType>
            <xs:complexType name="SimpleModuleData"/>
            <xs:simpleType name="Health">
                <xs:restriction base="SageReal"><xs:minInclusive value="0"/></xs:restriction>
            </xs:simpleType>
            <xs:complexType name="ActiveBodyModuleData">
                <xs:attribute name="MaxHealth" type="Health" use="required"/>
                <xs:attribute name="InitialHealth" type="SageReal" use="optional"/>
                <xs:attribute name="CheerRadius" type="SageReal" use="optional"/>
            </xs:complexType>
        </xs:schema>"#,
    )
    .unwrap();
    let input = data.join("Mod.xml");
    let compile = |body| {
        fs::write(
            &input,
            format!(
                r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
                    <GameObject id="Example">{body}</GameObject>
                </AssetDeclaration>"#
            ),
        )
        .unwrap();
        std::process::Command::new(env!("CARGO_BIN_EXE_binary-asset-builder"))
            .arg("compile-static-sdk")
            .arg(&input)
            .arg("--sdk-root")
            .arg(&root)
            .arg("--output")
            .arg(&built)
            .output()
            .unwrap()
    };

    let valid = compile(
        r#"<Body><ActiveBody id="Body" MaxHealth="100" InitialHealth="bad"/></Body>
           <Behaviors><Typo id="Unknown"/></Behaviors>"#,
    );
    let warnings = String::from_utf8(valid.stderr).unwrap();
    assert!(valid.status.success(), "{warnings}");
    for message in [
        "Invalid value 'bad' for attribute 'InitialHealth'",
        "ActiveBody.CheerRadius",
        "Unexpected module 'Typo' in 'Behaviors'",
        "AI module section",
    ] {
        assert!(
            warnings.lines().any(|line| {
                line.starts_with("Warning: ")
                    && line.contains(message)
                    && line.contains("GameObject:Example")
            }),
            "{warnings}"
        );
    }
    assert!(built.join("mods/example/data/mod.manifest").is_file());

    for (body, message) in [
        ("", "Missing required element 'Body'"),
        ("<Body><Typo/></Body>", "Unexpected module 'Typo' in 'Body'"),
        ("<Body/>", "Missing required entry in 'Body'"),
        (
            r#"<Body><ActiveBody id="Body"/></Body>"#,
            "Missing required attribute 'MaxHealth'",
        ),
        (
            r#"<Body><ActiveBody id="Body" MaxHealth="bad"/></Body>"#,
            "Invalid value 'bad' for attribute 'MaxHealth'",
        ),
        (
            r#"<Body><ActiveBody id="Body" MaxHealth="-1"/></Body>"#,
            "Invalid value '-1' for attribute 'MaxHealth'",
        ),
    ] {
        let failed = compile(body);
        let error = String::from_utf8(failed.stderr).unwrap();
        assert!(!failed.status.success());
        assert!(error.contains(message), "{error}");
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sdk_static_preserves_references_to_assets_in_commented_includes() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data");
    let built = root.join("builtmods");
    fs::create_dir_all(&data).unwrap();
    let input = data.join("Mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><!-- <Include type="all" source="Media.xml" /> --></Includes>
            <VideoEventList id="intro"><EventList><Movie MovieAsset="unavailable" /></EventList></VideoEventList>
        </AssetDeclaration>"#,
    )
    .unwrap();
    assert!(
        compile_static(&input, &root, &built)
            .err()
            .unwrap()
            .to_string()
            .contains("unresolved reference")
    );
    compile_sdk_static(&input, &root, &built).unwrap();
    let path = built.join("mods/example/data/mod.manifest");
    let bytes = fs::read(&path).unwrap();
    let manifest = Manifest::parse(&bytes).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(
        manifest.assets[0].references.iter().collect::<Vec<_>>(),
        [AssetId {
            type_id: binary_asset_builder::hash::fast_hash(b"Video"),
            instance_id: binary_asset_builder::hash::fast_hash(b"unavailable"),
        }]
    );
    check_linked_files(&path, &manifest).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sdk_static_builds_and_fixes_a_mod_with_only_sdk_references() {
    let root = scratch_root();
    let data = root.join("Mods/Test/Data");
    let built = root.join("builtmods");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(&built).unwrap();
    let sdk_xml = root.join("static.xml");
    fs::write(
        &sdk_xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="example" Filename="example" />
        </AssetDeclaration>"#,
    )
    .unwrap();
    let sdk_stream = compile_static(&sdk_xml, &root, &built).unwrap();
    fs::write(built.join("static.manifest"), sdk_stream.manifest).unwrap();
    let input = data.join("Mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Tags />
            <Includes><Include type="reference" source="DATA:static.xml" /></Includes>
        </AssetDeclaration>"#,
    )
    .unwrap();

    compile_sdk_static(&input, &root, &built).unwrap();
    let output = built.join("mods/test/data");
    let base = output.join("mod.manifest");
    let bytes = fs::read(&base).unwrap();
    let manifest = Manifest::parse(&bytes).unwrap();
    assert!(manifest.assets.is_empty());
    assert_eq!(manifest.stream_checksum, 0);
    assert_eq!(manifest.referenced_manifests.len(), 1);
    assert_eq!(manifest.referenced_manifests[0].path, b"static.manifest");
    check_linked_files(&base, &manifest).unwrap();

    let fixed = binary_asset_builder::fix_assets::fix_assets(&base, &built).unwrap();
    assert_eq!(fixed.assets, 0);
    assert_eq!(fixed.hashes_fixed, 0);
    assert_eq!(fs::read(base).unwrap(), bytes);
    for name in ["mod_l", "mod_m"] {
        let path = output.join(format!("{name}.manifest"));
        let bytes = fs::read(&path).unwrap();
        let patch = Manifest::parse(&bytes).unwrap();
        assert!(patch.assets.is_empty());
        assert_eq!(patch.referenced_manifests.len(), 1);
        assert!(patch.referenced_manifests[0].is_patch);
        assert_eq!(patch.referenced_manifests[0].path, b"mod.manifest");
        check_linked_files(&path, &patch).unwrap();
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_armor_with_sdk_token_tables_and_absolute_relocations() {
    let root = scratch_root();
    let input = root.join("armor.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <ArmorTemplate id="ProbeArmor" Default="50%"><Armor Damage="GUN" Percent="20%"/></ArmorTemplate>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let entry = &manifest.assets[0];
    assert_eq!(entry.tokenized, Some(1));
    assert_eq!(entry.type_hash, 0x641a_d32e);
    assert_eq!(entry.instance_hash, 0x1970_3ecc);
    let words: Vec<_> = stream.files.bin[4..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| u32::from_le_bytes(*bytes))
        .collect();
    assert_eq!(&words[..3], &[0xffe8_8740, 0x6458_64f3, 148]);
    assert_eq!(
        &words[37..],
        &[
            0x3f00_0000,
            0x3f80_0000,
            0x3f80_0000,
            0x3f80_0000,
            0x3f80_0000,
            1,
            176,
            21,
            0x3e4c_cccc
        ]
    );
    assert_eq!(&stream.files.relo[4..], &[172, 0, 0, 0, 255, 255, 255, 255]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_xinclude_expands_nested_fragments_before_defines_and_hashing() {
    let root = scratch_root();
    let input = root.join("probe.xml");
    let fragment = root.join("fragments/fragment.xml");
    let video = root.join("fragments/nested/video.xml");
    fs::create_dir_all(video.parent().unwrap()).unwrap();
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset" xmlns:xi="http://www.w3.org/2001/XInclude">
            <Defines><Define name="BASE_NAME" value="example"/></Defines>
            <xi:include href="fragments/fragment.xml"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        &fragment,
        r#"<xi:include xmlns:xi="http://www.w3.org/2001/XInclude" href="nested/video.xml" parse="xml"/>"#,
    )
    .unwrap();
    fs::write(
        &video,
        r#"<Video xmlns="uri:ea.com:eala:asset" id="included" Filename="=$BASE_NAME + '_included'"/>"#,
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets[0].name, b"Video:included");
    assert_eq!(manifest.assets[0].instance_hash, 0x2d59_0b55);
    assert!(
        stream
            .files
            .bin
            .windows(17)
            .any(|bytes| bytes == b"example_included\0")
    );

    fs::write(
        &video,
        r#"<xi:include xmlns:xi="http://www.w3.org/2001/XInclude" href="../fragment.xml"/>"#,
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(error.to_string().contains("XInclude cycle"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_xpointer_child_selection_matches_ea() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/xpointer-videos");
    let stream = compile_static(&root.join("probe.xml"), &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let oracle =
        Manifest::parse(include_bytes!("fixtures/xpointer-videos/probe.manifest")).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    let mut bin = &stream.files.bin[4..];
    let mut relo = &stream.files.relo[4..];
    for (actual, expected) in manifest.assets.iter().zip(oracle.assets) {
        assert_eq!(actual.name, expected.name);
        assert_eq!(actual.type_hash, expected.type_hash);
        assert_eq!(actual.instance_hash, expected.instance_hash);
        let asset: &[u8] = match actual.name {
            b"Video:One" => include_bytes!("fixtures/xpointer-videos/one.asset"),
            b"Video:Two" => include_bytes!("fixtures/xpointer-videos/two.asset"),
            name => panic!("unexpected selected asset {name:?}"),
        };
        let (payload, relocations) = asset[32..].split_at(expected.chunk_sizes.instance as usize);
        assert_eq!(&bin[..payload.len()], payload);
        assert_eq!(&relo[..relocations.len()], relocations);
        bin = &bin[payload.len()..];
        relo = &relo[relocations.len()..];
    }
    assert!(bin.is_empty() && relo.is_empty());
}

#[test]
fn static_expressions_use_transitive_defines_and_explicit_overrides() {
    let root = scratch_root();
    let defs = root.join("defs.xml");
    let scope = root.join("scope.xml");
    let input = root.join("Mod.xml");
    fs::write(
        &defs,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Defines>
                <Define name="DISTANCE" value="200"/>
                <Define name="MARGIN" value="30"/>
                <Define name="LABEL" value="repair drone"/>
            </Defines>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        &scope,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="instance" source="defs.xml"/></Includes>
            <Defines>
                <Define name="MARGIN" override="true" value="40"/>
                <Define name="SIZE" value="= ($DISTANCE * 2) + $MARGIN"/>
            </Defines>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let source = r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <Includes><Include type="all" source="scope.xml"/></Includes>
        <Video id="shadow" Filename="=$SIZE"/>
        <Video id="label" Filename='=$LABEL + &quot; &apos;preview&apos; &amp; details&quot;'/>
        <Video id="numeric" Filename="= round(3.5) + (5 % 2) + (1 &lt;= 2 and 1 != 0) + 1 / 3"/>
    </AssetDeclaration>"#;
    fs::write(&input, source).unwrap();
    let evaluated = compile_static(&input, &root, &root).unwrap();
    let literal = source
        .replace("=$SIZE", "440")
        .replace(
            "=$LABEL + &quot; &apos;preview&apos; &amp; details&quot;",
            "repair drone &apos;preview&apos; &amp; details",
        )
        .replace(
            "= round(3.5) + (5 % 2) + (1 &lt;= 2 and 1 != 0) + 1 / 3",
            "6.33333333333333",
        );
    fs::write(&input, literal).unwrap();
    let expected = compile_static(&input, &root, &root).unwrap();
    assert_eq!(evaluated.manifest, expected.manifest);
    assert_eq!(evaluated.files.bin, expected.files.bin);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_expressions_do_not_use_defines_from_sibling_documents() {
    let root = scratch_root();
    fs::write(
        root.join("left.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Defines><Define name="LEFT" value="30"/></Defines>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        root.join("right.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="isolated" Filename="=$LEFT"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let input = root.join("Mod.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes>
                <Include type="all" source="left.xml"/>
                <Include type="all" source="right.xml"/>
            </Includes>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let error = compile_static(&input, &root, &root)
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("right.xml"));
    assert!(error.contains("unknown define $LEFT"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn static_includes_accept_empty_legacy_w3x_placeholders() {
    let root = scratch_root();
    let input = root.join("Mod.xml");
    let placeholder = root.join("Placeholder.w3x");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="all" source="Placeholder.w3x"/></Includes>
            <Video id="example" Filename="example"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(&placeholder, "<AssetDeclaration></AssetDeclaration>").unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].name, b"Video:example");

    fs::write(
        &placeholder,
        "<AssetDeclaration><Unknown/></AssetDeclaration>",
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(
        error
            .to_string()
            .contains("expected a SAGE AssetDeclaration root")
    );
    fs::remove_file(placeholder).unwrap();
    fs::remove_file(input).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn static_art_includes_accept_only_empty_namespace_free_xml_placeholders() {
    let root = scratch_root();
    let input = root.join("Mod.xml");
    let art = root.join("Art");
    fs::create_dir(&art).unwrap();
    let placeholder = art.join("beam.xml");
    fs::write(
        &input,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="all" source="ART:./beam.xml"/></Includes>
            <Video id="example" Filename="example"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(&placeholder, r#"<?xml version="1.0"?><AssetDeclaration />"#).unwrap();

    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 1);
    assert_eq!(manifest.assets[0].name, b"Video:example");
    assert!(compile_static(&placeholder, &root, &root).is_err());

    fs::write(
        &placeholder,
        "<AssetDeclaration><Unknown/></AssetDeclaration>",
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(
        error
            .to_string()
            .contains("expected a SAGE AssetDeclaration root")
    );

    fs::remove_file(placeholder).unwrap();
    fs::remove_file(input).unwrap();
    fs::remove_dir(art).unwrap();
    fs::remove_dir(root).unwrap();
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
fn sdk_global_publishes_inherited_music_events_and_track_data() {
    let root = scratch_root();
    let data = root.join("Mods/Example/Data/AdditionalMaps");
    let built = root.join("builtmods");
    fs::create_dir_all(&data).unwrap();
    fs::write(
        data.join("events.h"),
        "#define PATH_EVENT_Intro 0x1234\n#define PATH_TRACK_Song 0x5678\n",
    )
    .unwrap();
    fs::write(data.join("song.mus"), b"test track bytes").unwrap();
    fs::write(
        data.join("base.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <PathMusicEvent id="Base" PathfinderEventHeader="events.h"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        data.join("mapmetadata_music.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="instance" source="base.xml"/></Includes>
            <PathMusicEvent id="Intro" inheritFrom="PathMusicEvent:Base"/>
            <PathMusicTrack id="Song" File="song.mus" Type="MAIN" PathfinderTrackHeader="events.h"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    compile_sdk_global(&data, &root, &built).unwrap();
    let output = built.join("mods/example/data/additionalmaps");
    let manifest = fs::read(output.join("mapmetadata_music.manifest")).unwrap();
    let manifest = Manifest::parse(&manifest).unwrap();
    let [event, track] = manifest.assets.as_slice() else {
        panic!("expected the inherited event and track");
    };
    assert_eq!(event.name, b"PathMusicEvent:Intro");
    assert_eq!(track.name, b"PathMusicTrack:Song");
    let binary = fs::read(output.join("mapmetadata_music.bin")).unwrap();
    assert_eq!(&binary[8..12], &0x1234_u32.to_le_bytes());
    let cdata = output.join("mapmetadata_music/cdata");
    let files: Vec<_> = fs::read_dir(&cdata)
        .unwrap()
        .map(|file| file.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1);
    assert_eq!(fs::read(&files[0]).unwrap(), b"test track bytes");

    fs::write(data.join("song.mus"), b"new track").unwrap();
    compile_sdk_global(&data, &root, &built).unwrap();
    assert!(!files[0].exists());
    assert_eq!(fs::read_dir(&cdata).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_campaign_movies_and_missions_with_unused_defines() {
    let root = scratch_root();
    let xml = root.join("campaign.xml");
    let document = r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <Defines><Define name="Unused" value="Example"/></Defines>
        <CampaignEnumerator id="campaigns"><Campaign>campaign</Campaign></CampaignEnumerator>
        <CampaignTemplate id="campaign">
            <Mission>mission</Mission>
            <FinalMovie FileName="Final"/>
            <AlternateFinalMovie FileName="Alternate"/>
        </CampaignTemplate>
        <MissionTemplate id="mission">
            <Prerequisite>0</Prerequisite>
            <AICommander Faction="Japan"/>
            <IntroMovie FileName="Intro"/>
        </MissionTemplate>
    </AssetDeclaration>"#;
    fs::write(&xml, document).unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [mission, campaign, enumerator] = manifest.assets.as_slice() else {
        panic!("expected the mission, campaign and campaign enumerator");
    };
    assert_eq!(mission.name, b"MissionTemplate:mission");
    assert_eq!(campaign.references.iter().next(), Some(mission.id));
    assert_eq!(enumerator.references.iter().next(), Some(campaign.id));
    assert_eq!(&stream.files.bin[4 + 176..4 + 184], b"Intro\0\0\0");

    fs::write(
        &xml,
        document.replace("FileName=\"Intro\"", "FileName=\"=Unused\""),
    )
    .unwrap();
    let error = compile_static(&xml, &root, &root).err().unwrap();
    assert!(error.to_string().contains("XML expression at"));
    assert!(error.to_string().contains("expected a 32-bit integer"));
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
fn compiles_a_video_with_an_explicitly_empty_filename() {
    let root = scratch_root();
    let xml = root.join("video.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Video id="EALogoMovie" Filename="" />
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let asset = &manifest.assets[0];
    assert_eq!(asset.instance_hash, 0x84d1_dd89);
    assert_eq!(asset.chunk_sizes.instance, 44);
    assert_eq!(&stream.files.bin[4 + 28..], b"\0\0\0\0No Comment\0\0");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_multiplayer_game_rules_with_the_sdk_layout() {
    let root = scratch_root();
    let xml = root.join("rules.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <MpGameRules id="TheMpGameRules">
                <SkirmishStartCash LoCash="10000" HiCash="1000000"
                    ChoiceStepAmount="10000" DefaultChoiceIndex="1"/>
            </MpGameRules>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [asset] = manifest.assets.as_slice() else {
        panic!("expected one MpGameRules asset");
    };
    assert_eq!(asset.type_hash, 0x34d9_1648);
    assert_eq!(asset.instance_hash, 0xd239_7a1c);
    assert_eq!(
        &stream.files.bin[4..],
        [0_u32, 5, 10000, 1000000, 10000, 1]
            .map(u32::to_le_bytes)
            .as_flattened()
    );
    assert_eq!(stream.files.imp.len(), 4);
    assert_eq!(stream.files.relo.len(), 4);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_standalone_ai_personality_and_hashes_its_default_portrait_reference() {
    let root = scratch_root();
    let xml = root.join("personality.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <AIPersonalityDefinition id="base" PersonalityType="Sample">
                <CountAsStructureForBuildPurposesFilter />
            </AIPersonalityDefinition>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let asset = &manifest.assets[0];
    assert_eq!(asset.type_hash, 0xcb3c_9d2b);
    assert_eq!(asset.instance_hash, 0x3eab_387e);
    assert_eq!(asset.chunk_sizes.instance, 520);
    assert_eq!(asset.references.iter().len(), 0);
    let payload = &stream.files.bin[4..];
    assert_eq!(&payload[224..228], &0x97a0_e672_u32.to_le_bytes());
    assert_eq!(&payload[512..], b"Sample\0\0");
    assert_eq!(&stream.files.relo[4..], &[8, 0, 0, 0, 255, 255, 255, 255]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_health_bar_imports_and_weak_object_filters() {
    let root = scratch_root();
    let xml = root.join("health.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <HealthBarSettings id="bars" DefaultHealthBar="small">
                <ObjectMapping HealthBar="small">
                    <ObjectFilter Rule="ANY" Include="INFANTRY">
                        <IncludeThing>SampleObject</IncludeThing>
                    </ObjectFilter>
                </ObjectMapping>
            </HealthBarSettings>
            <HealthBarTemplate id="small" Length="20"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [template, settings] = manifest.assets.as_slice() else {
        panic!("expected health bar template and settings");
    };
    assert_eq!(
        settings.references.iter().collect::<Vec<_>>(),
        [template.id, template.id]
    );
    let payload = &stream.files.bin[4 + 8..];
    assert_eq!(
        &payload[..20],
        [0_u32, 1, 1, 16, 2].map(u32::to_le_bytes).as_flattened()
    );
    assert_eq!(&payload[36..40], &(1_u32 << 10).to_le_bytes());
    assert_eq!(
        &payload[116..124],
        [1_u32, 132].map(u32::to_le_bytes).as_flattened()
    );
    assert_eq!(
        &stream.files.relo[4..],
        [12_u32, 120, u32::MAX].map(u32::to_le_bytes).as_flattened()
    );
    assert_eq!(
        &stream.files.imp[4..],
        [4_u32, 16, u32::MAX].map(u32::to_le_bytes).as_flattened()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compiles_credits_with_ansi_strings_and_empty_lines() {
    let root = scratch_root();
    let xml = root.join("credits.xml");
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Credits id="Example">
                <Scroll Rate="1" RateEveryFrame="2" ScrollDown="true"/>
                <Style Name="TitleFont">
                    <Color R="1" G="2" B="3"/>
                    <Font Name="É" Size="12"/>
                </Style>
                <CreditItem Style="TitleFont">
                    <Text>É—</Text>
                    <Text>        </Text>
                </CreditItem>
            </Credits>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stream = compile_static(&xml, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [asset] = manifest.assets.as_slice() else {
        panic!("expected one Credits asset");
    };
    assert_eq!(asset.type_hash, 0x10d6_57ee);
    assert_eq!(asset.chunk_sizes.instance, 96);
    let payload = &stream.files.bin[4..];
    assert_eq!(
        &payload[..32],
        [0_u32, 1, 2, 1, 1, 32, 1, 60]
            .map(u32::to_le_bytes)
            .as_flattened()
    );
    assert_eq!(&payload[56..60], b"\xc9\0\0\0");
    assert_eq!(
        &payload[72..88],
        [2_u32, 88, 0, 92].map(u32::to_le_bytes).as_flattened()
    );
    assert_eq!(&payload[88..], b"\xc9\x97\0\0\0\0\0\0");
    assert_eq!(
        &stream.files.relo[4..],
        [20_u32, 28, 44, 68, 76, 84, u32::MAX]
            .map(u32::to_le_bytes)
            .as_flattened()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn emits_the_concrete_external_audio_type_and_local_movie_import() {
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
            type_id: 0x844d_7b9f,
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
fn compiles_inherited_ai_personality_from_an_instance_include() {
    let root = scratch_root();
    let bases = root.join("bases.xml");
    let xml = root.join("ai.xml");
    fs::write(
        &bases,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <AIPersonalityDefinition id="Base" PersonalityType="Base" DesiredExcessPowerBuffer="100">
                <SpecificUnitCap Unit="ExampleUnit" Cap="3"/>
                <CountAsStructureForBuildPurposesFilter Rule="ANY" Include="STRUCTURE">
                    <ExcludeThing>ExampleUnit</ExcludeThing>
                </CountAsStructureForBuildPurposesFilter>
            </AIPersonalityDefinition>
            <AIPersonalityDefinition id="Intermediate" PersonalityType="Intermediate" inheritFrom="Base">
                <UnitCap Cap="20" Difficulty="MEDIUM"/>
            </AIPersonalityDefinition>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        &xml,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
            <Includes><Include type="instance" source="bases.xml"/></Includes>
            <AIPersonalityDefinition id="Child" PersonalityType="Child" inheritFrom="Intermediate"
                SkirmishPersonality="true" DesiredExcessPowerBuffer="50">
                <UnitCap Cap="40" Difficulty="HARD"/>
            </AIPersonalityDefinition>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let result = compile_static(&xml, &root, &root);
    fs::remove_file(xml).unwrap();
    fs::remove_file(bases).unwrap();
    fs::remove_dir(root).unwrap();

    let stream = result.unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let [asset] = manifest.assets.as_slice() else {
        panic!("expected only the child AI personality");
    };
    assert_eq!(asset.name, b"AIPersonalityDefinition:Child");
    assert_eq!(asset.references.iter().len(), 0);
    let payload = &stream.files.bin[4..];
    let word = |offset| u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
    assert_eq!(word(24), 50);
    assert_eq!(payload[508], 1);
    assert_eq!(word(252), 2);
    let caps = word(256) as usize;
    assert_eq!([word(caps), word(caps + 4)], [20, 2]);
    assert_eq!([word(caps + 8), word(caps + 12)], [40, 4]);
    assert_eq!(word(364), 1);
    let specific_cap = word(368) as usize;
    assert_eq!(word(specific_cap + 4), 3);
    assert_eq!(word(388), 2);
    assert_eq!(word(488), 1);
    assert_eq!(word(word(492) as usize), word(specific_cap));
    let expected: Vec<_> = [8, 256, 368, 492, u32::MAX]
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

#[test]
fn game_object_duplicate_precedence_matches_ea() {
    let root = scratch_root();
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&schemas).unwrap();
    fs::write(schemas.join("CnC3Types.xsd"), r#"
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="GameObject"><xs:sequence><xs:element name="Body" type="Body"/></xs:sequence></xs:complexType>
            <xs:complexType name="Body"><xs:sequence><xs:element name="ImmortalBody" type="ImmortalBody"/></xs:sequence></xs:complexType>
            <xs:complexType name="ImmortalBody"/>
        </xs:schema>
    "#).unwrap();
    let declaration = |body: &str| {
        format!("<AssetDeclaration xmlns=\"uri:ea.com:eala:asset\">{body}</AssetDeclaration>")
    };
    let game_object = |energy| {
        format!(
            "<GameObject id=\"SameObject\" EnergyProduction=\"{energy}\"><Body><ImmortalBody id=\"Body\" MaxHealth=\"1\"/></Body></GameObject>"
        )
    };
    fs::write(root.join("base.xml"), declaration(&game_object(11))).unwrap();
    fs::write(
        root.join("overlay.xml"),
        declaration(
            r#"
        <Includes><Include type="instance" source="base.xml"/></Includes>
        <GameObject id="SameObject" inheritFrom="SameObject" EnergyProduction="22"/>
    "#,
        ),
    )
    .unwrap();
    fs::write(
        root.join("wrapper.xml"),
        declaration(r#"<Includes><Include type="all" source="base.xml"/></Includes>"#),
    )
    .unwrap();
    let input = root.join("root.xml");
    for (first, second, source, energy, local) in [
        ("base.xml", "overlay.xml", "DATA:base.xml", 11, false),
        ("wrapper.xml", "overlay.xml", "DATA:base.xml", 11, false),
        ("overlay.xml", "base.xml", "DATA:overlay.xml", 22, false),
        ("base.xml", "overlay.xml", "DATA:root.xml", 44, true),
    ] {
        let local = if local {
            game_object(44)
        } else {
            String::new()
        };
        fs::write(&input, declaration(&format!(
            r#"<Includes><Include type="all" source="{first}"/><Include type="all" source="{second}"/></Includes>{local}"#,
        ))).unwrap();
        let stream = compile_static(&input, &root, &root).unwrap();
        let manifest = Manifest::parse(&stream.manifest).unwrap();
        assert_eq!(manifest.assets.len(), 1);
        assert_eq!(manifest.assets[0].source_file_name, source.as_bytes());
        let bytes = &stream.files.bin[4..];
        let object = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        assert_eq!(
            i32::from_le_bytes(bytes[object + 0x88..object + 0x8c].try_into().unwrap()),
            energy
        );
    }
    fs::write(
        &input,
        declaration(&format!("{}{}", game_object(11), game_object(66))),
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(
        error
            .to_string()
            .contains("duplicate asset ID GameObject:SameObject")
    );
}

#[test]
fn static_asset_duplicate_precedence_matches_ea() {
    let root = scratch_root();
    let declaration = |body: &str| {
        format!(r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">{body}</AssetDeclaration>"#)
    };
    fs::write(
        root.join("first.xml"),
        declaration(
            r#"
        <PlayerTech id="SiblingPriority" TechPointsRequired="11"/>
        <PlayerTech id="ParentPriority" TechPointsRequired="12"/>
    "#,
        ),
    )
    .unwrap();
    fs::write(
        root.join("second.xml"),
        declaration(
            r#"
        <PlayerTech id="SiblingPriority" TechPointsRequired="21"/>
        <PlayerTech id="ParentPriority" TechPointsRequired="22"/>
    "#,
        ),
    )
    .unwrap();
    let input = root.join("precedence.xml");
    let body = r#"
        <Includes><Include type="all" source="first.xml"/><Include type="all" source="second.xml"/></Includes>
        <PlayerTech id="ParentPriority" TechPointsRequired="30"/>
    "#;
    fs::write(&input, declaration(body)).unwrap();

    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    let mut offset = 4;
    for asset in &manifest.assets {
        let (tech_points, source) = match asset.name {
            b"PlayerTech:SiblingPriority" => (11, "DATA:first.xml"),
            b"PlayerTech:ParentPriority" => (30, "DATA:precedence.xml"),
            name => panic!("unexpected asset {}", String::from_utf8_lossy(name)),
        };
        assert_eq!(asset.source_file_name, source.as_bytes());
        assert_eq!(
            stream.files.bin[offset + 4..offset + 8],
            [tech_points, 0, 0, 0]
        );
        offset += asset.chunk_sizes.instance as usize;
    }

    fs::write(
        &input,
        declaration(&format!(
            r#"{body}<PlayerTech id="ParentPriority" TechPointsRequired="40"/>"#
        )),
    )
    .unwrap();
    let error = compile_static(&input, &root, &root).err().unwrap();
    assert!(
        error
            .to_string()
            .contains("duplicate asset ID PlayerTech:ParentPriority")
    );
}

#[test]
fn inherited_upgrade_payload_matches_ea() {
    let root = scratch_root();
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&schemas).unwrap();
    fs::write(
        schemas.join("CnC3Types.xsd"),
        include_str!("../schemas/ra3/xsd/AssetTypeUpgradeTemplate.xsd").replace(
            "</xs:schema>",
            r#"<xs:complexType name="BaseInheritableAsset"/></xs:schema>"#,
        ),
    )
    .unwrap();
    let input = root.join("upgrade.xml");
    fs::write(
        &input,
        include_bytes!("fixtures/inherited-upgrade/upgrade.xml"),
    )
    .unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 2);
    let mut payload = &stream.files.bin[4..];
    let mut relocations = &stream.files.relo[4..];
    for asset in &manifest.assets {
        let ea: &[u8] = match asset.name {
            b"UpgradeTemplate:UpgradeProbeBase" => {
                include_bytes!("fixtures/inherited-upgrade/base.asset")
            }
            b"UpgradeTemplate:UpgradeProbeChild" => {
                include_bytes!("fixtures/inherited-upgrade/child.asset")
            }
            _ => panic!("unexpected upgrade {:?}", asset.name),
        };
        let (actual_payload, rest) = payload.split_at(asset.chunk_sizes.instance as usize);
        payload = rest;
        let (actual_relocations, rest) =
            relocations.split_at(asset.chunk_sizes.relocations as usize);
        relocations = rest;
        assert_eq!(actual_payload, &ea[32..32 + actual_payload.len()]);
        assert_eq!(actual_relocations, &ea[32 + actual_payload.len()..]);
        assert_eq!(asset.instance_hash.to_le_bytes(), ea[12..16]);
    }
}

#[test]
fn inherited_fx_lists_match_ea() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inherited-fx");
    let root = scratch_root();
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&schemas).unwrap();
    let schema = include_str!("../schemas/ra3/xsd/AssetTypeFXList.xsd").replace(
        "</xs:schema>",
        "<xs:complexType name=\"BaseAssetType\"/></xs:schema>",
    );
    fs::write(schemas.join("CnC3Types.xsd"), schema).unwrap();
    let input = root.join("inherited-fx.xml");
    fs::copy(fixtures.join("inherited-fx.xml"), &input).unwrap();
    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    assert_eq!(manifest.assets.len(), 4);
    let mut payload = &stream.files.bin[4..];
    let mut relocations = &stream.files.relo[4..];
    for entry in manifest.assets {
        let filename = format!(
            "{:08x}.{:08x}.{:08x}.{:08x}.asset",
            entry.id.type_id, entry.type_hash, entry.id.instance_id, entry.instance_hash
        );
        let ea = fs::read(fixtures.join("assets").join(&filename)).unwrap();
        let bytes = entry.chunk_sizes.instance as usize;
        let relo_bytes = entry.chunk_sizes.relocations as usize;
        let (current, remaining) = payload.split_at(bytes);
        let (relo, remaining_relo) = relocations.split_at(relo_bytes);
        assert_eq!(current, &ea[32..32 + bytes], "{filename}");
        assert_eq!(relo, &ea[32 + bytes..32 + bytes + relo_bytes], "{filename}");
        payload = remaining;
        relocations = remaining_relo;
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inherited_experience_level_payload_matches_ea() {
    let root = scratch_root();
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&schemas).unwrap();
    let schema = include_str!("../schemas/ra3/xsd/AssetTypeExperienceLevelTemplate.xsd")
        .lines()
        .filter(|line| !line.contains("schemaLocation="))
        .collect::<Vec<_>>()
        .join("\n")
        .replace(
            "</xs:schema>",
            r#"
              <xs:complexType name="BaseInheritableAsset"/>
              <xs:complexType name="Color4f"/>
              <xs:simpleType name="WeakReference"><xs:restriction base="xs:string"/></xs:simpleType>
              <xs:simpleType name="GameObjectWeakRef"><xs:restriction base="WeakReference"/></xs:simpleType>
              <xs:simpleType name="UpgradeTemplateWeakRef"><xs:restriction base="WeakReference"/></xs:simpleType>
            </xs:schema>"#,
        );
    fs::write(schemas.join("CnC3Types.xsd"), schema).unwrap();
    let input = root.join("rank.xml");
    fs::write(
        &input,
        include_bytes!("fixtures/inherited-experience/rank.xml"),
    )
    .unwrap();
    fs::create_dir(root.join("base")).unwrap();
    fs::write(
        root.join("base/template.xml"),
        include_bytes!("fixtures/inherited-experience/base/template.xml"),
    )
    .unwrap();

    let stream = compile_static(&input, &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let mut payload = &stream.files.bin[4..];
    let mut relocations = &stream.files.relo[4..];
    for asset in &manifest.assets {
        let (current, remaining) = payload.split_at(asset.chunk_sizes.instance as usize);
        payload = remaining;
        let (relo, remaining) = relocations.split_at(asset.chunk_sizes.relocations as usize);
        relocations = remaining;
        if asset.name == b"ExperienceLevelTemplate:RankDerived" {
            let ea = include_bytes!("fixtures/inherited-experience/derived.asset");
            assert_eq!(current, &ea[32..32 + current.len()]);
            assert_eq!(relo, &ea[32 + current.len()..]);
            return;
        }
    }
    panic!("missing derived experience level");
}

#[test]
fn links_polymorphic_render_models_like_ea() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/render-link");
    let stream = compile_static(&root.join("render-link.xml"), &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let ea = Manifest::parse(include_bytes!("fixtures/render-link/render-link.manifest")).unwrap();
    let index = manifest
        .assets
        .iter()
        .position(|asset| asset.name == b"GameObject:RenderLinkProbe")
        .unwrap();
    let actual = &manifest.assets[index];
    let expected = ea
        .assets
        .iter()
        .find(|asset| asset.id == actual.id)
        .unwrap();
    assert_eq!(
        actual.references.iter().collect::<Vec<_>>(),
        expected.references.iter().collect::<Vec<_>>()
    );
    assert_eq!(actual.type_hash, expected.type_hash);
    let payload_start = 4 + manifest.assets[..index]
        .iter()
        .map(|asset| asset.chunk_sizes.instance as usize)
        .sum::<usize>();
    let payload_len = actual.chunk_sizes.instance as usize;
    let oracle = include_bytes!("fixtures/render-link/render-link.asset");
    assert_eq!(
        &stream.files.bin[payload_start..payload_start + payload_len],
        &oracle[32..32 + payload_len]
    );
}

#[test]
fn maps_link_worldbuilder_models_and_keep_local_overrides() {
    let root = scratch_root();
    let map = root.join("map.xml");
    let assets = root.join("WorldBuilder.xml");
    fs::create_dir(root.join("Data")).unwrap();
    fs::create_dir_all(root.join("Schemas/xsd")).unwrap();
    fs::write(
        root.join("Schemas/xsd/CnC3Types.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:xas="uri:ea.com:eala:asset:schema">
        <xs:simpleType name="WeakReference"><xs:restriction base="xs:string"/></xs:simpleType>
        <xs:simpleType name="GameObjectWeakRef" xas:refType="GameObject"><xs:restriction base="WeakReference"/></xs:simpleType>
        <xs:complexType name="ToppleUpdateModuleData"><xs:attribute name="StumpId" type="GameObjectWeakRef"/></xs:complexType>
        <xs:complexType name="GameObject"><xs:sequence>
          <xs:element name="Behaviors"><xs:complexType><xs:choice>
            <xs:element name="ToppleUpdate" type="ToppleUpdateModuleData"/>
          </xs:choice></xs:complexType></xs:element>
        </xs:sequence></xs:complexType>
        </xs:schema>"#,
    )
    .unwrap();
    fs::write(
        &assets,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <W3DContainer id="StockModel"/>
        <GameObject id="Prop" EditorName="Stock"><Body><PropBody id="Body"/></Body></GameObject>
        </AssetDeclaration>"#,
    )
    .unwrap();
    let stock = compile_static(&assets, &root, &root).unwrap();
    fs::write(root.join("worldbuilder.manifest"), &stock.manifest).unwrap();
    fs::write(
        root.join("Data/props.xml"),
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <GameObject id="Prop" EditorName="Map">
            <Draws><PropDraw id="Draw" Model="StockModel"/></Draws>
            <Behaviors><ToppleUpdate id="Topple" StumpId="Stump"/></Behaviors>
            <Body><PropBody id="Body"/></Body>
        </GameObject>
        <GameObject id="Stump"><Body><PropBody id="Body"/></Body></GameObject>
        <MusicPalette id="Palette"/>
        </AssetDeclaration>"#,
    )
    .unwrap();
    fs::write(
        &map,
        r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset">
        <Includes><Include type="instance" source="DATA:props.xml"/></Includes>
        <GameMap id="Map">
            <Prop ThingTemplate="Prop"><Position x="0" y="0" z="0"/></Prop>
            <WorldDict><AssetIdProperty Key="musicZone" Value="Palette"/></WorldDict>
        </GameMap>
        </AssetDeclaration>"#,
    )
    .unwrap();

    let stream = compile_map(&map, &root, &root).unwrap();
    let actual = Manifest::parse(&stream.manifest).unwrap();
    let expected = Manifest::parse(&stock.manifest).unwrap();
    let stock_model = expected
        .assets
        .iter()
        .find(|a| a.name == b"W3DContainer:StockModel")
        .unwrap();
    let stock_prop = expected
        .assets
        .iter()
        .find(|a| a.name == b"GameObject:Prop")
        .unwrap();
    let map_prop = actual
        .assets
        .iter()
        .find(|a| a.id == stock_prop.id)
        .unwrap();
    assert_eq!(actual.assets.len(), 4);
    assert!(
        actual
            .assets
            .iter()
            .any(|asset| asset.name == b"GameObject:Stump")
    );
    assert!(
        actual
            .assets
            .iter()
            .any(|asset| asset.name == b"MusicPalette:Palette")
    );
    assert_eq!(map_prop.instance_hash, stock_prop.instance_hash);
    assert_eq!(
        map_prop.references.iter().collect::<Vec<_>>(),
        [stock_model.id]
    );
    assert_eq!(actual.referenced_manifests.len(), 1);
    assert!(actual.referenced_manifests[0].is_patch);
    assert_eq!(
        actual.referenced_manifests[0].path,
        b"worldbuilder.11.manifest"
    );
}

#[test]
fn sdk_map_publishing_preserves_sources_and_previous_stream_on_failure() {
    let root = scratch_root();
    let official = root.join("Mods/Test/Misc/Data/Maps/Official");
    let map = official.join("MapOne");
    let source = map.join("map.xml");
    let xml = r#"<AssetDeclaration xmlns="uri:ea.com:eala:asset"><GameMap id="MapOne"/></AssetDeclaration>"#;
    fs::create_dir_all(&map).unwrap();
    fs::write(&source, xml).unwrap();
    let stock = compile_static(&source, &root, &root).unwrap();
    fs::write(root.join("worldbuilder.manifest"), &stock.manifest).unwrap();
    fs::write(map.join("MapOne.map"), b"terrain").unwrap();
    fs::create_dir(official.join("PrecompiledMap")).unwrap();

    for _ in 0..2 {
        assert_eq!(
            compile_sdk_maps(&official, &root, &root, &official).unwrap(),
            1
        );
    }
    let path = map.join("map_mod.manifest");
    let prior = fs::read(&path).unwrap();
    check_linked_files(&path, &Manifest::parse(&prior).unwrap()).unwrap();
    assert_eq!(fs::read(map.join("map.version")).unwrap(), b"_mod\r\n");
    assert_eq!(fs::read(map.join("MapOne.map")).unwrap(), b"terrain");
    assert_eq!(fs::read_to_string(&source).unwrap(), xml);
    assert!(!official.join("PrecompiledMap/map.version").exists());

    fs::write(&source, "<invalid").unwrap();
    assert!(compile_sdk_maps(&official, &root, &root, &official).is_err());
    assert_eq!(fs::read(&path).unwrap(), prior);
    assert_eq!(fs::read(map.join("map.version")).unwrap(), b"_mod\r\n");
    check_linked_files(&path, &Manifest::parse(&prior).unwrap()).unwrap();
}

#[test]
fn inherited_stance_reference_omits_schema_ordinal() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inherited-stance");
    let root = scratch_root();
    let schemas = root.join("Schemas/xsd");
    fs::create_dir_all(&schemas).unwrap();
    fs::write(
        schemas.join("CnC3Types.xsd"),
        r#"
        <xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
            <xs:complexType name="GameObject">
                <xs:sequence>
                    <xs:element name="Behaviors" type="Behaviors"/>
                    <xs:element name="Body" type="Body"/>
                </xs:sequence>
            </xs:complexType>
            <xs:complexType name="Behaviors"><xs:choice maxOccurs="unbounded">
                <xs:element name="StancesBehavior" type="StancesBehavior"/>
            </xs:choice></xs:complexType>
            <xs:complexType name="StancesBehavior">
                <xs:attribute name="StanceTemplate" type="AssetReference"/>
            </xs:complexType>
            <xs:complexType name="Body"><xs:sequence>
                <xs:element name="ImmortalBody" type="ImmortalBody"/>
            </xs:sequence></xs:complexType>
            <xs:complexType name="ImmortalBody"/>
        </xs:schema>
    "#,
    )
    .unwrap();
    for name in [
        "stance-base.xml",
        "derived-stance.xml",
        "stance-library.xml",
        "stance-library.manifest",
    ] {
        fs::copy(fixtures.join(name), root.join(name)).unwrap();
    }
    let stream = compile_static(&root.join("derived-stance.xml"), &root, &root).unwrap();
    let manifest = Manifest::parse(&stream.manifest).unwrap();
    let index = manifest
        .assets
        .iter()
        .position(|asset| asset.name == b"GameObject:StanceDerived")
        .unwrap();
    let external = Manifest::parse(include_bytes!(
        "fixtures/inherited-stance/stance-library.manifest"
    ))
    .unwrap();
    let stance = &external.assets[0];
    let derived = &manifest.assets[index];
    assert!(derived.references.iter().eq([stance.id]));
    let start = 4 + manifest.assets[..index]
        .iter()
        .map(|asset| asset.chunk_sizes.instance as usize)
        .sum::<usize>();
    let size = derived.chunk_sizes.instance as usize;
    let ea = include_bytes!("fixtures/inherited-stance/942fff2d.cb8d0980.e02eaed7.fd668405.asset");
    assert_eq!(&stream.files.bin[start..start + size], &ea[32..32 + size]);
}

#[test]
fn polymorphic_audio_imports_match_ea_and_survive_fix_assets() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/audio-external");
    let input = fixtures.join("audio-resolved.xml");
    let stream = compile_static(&input, &fixtures, &fixtures).unwrap();
    let native = Manifest::parse(&stream.manifest).unwrap();
    let ea = Manifest::parse(include_bytes!(
        "fixtures/audio-external/audio-resolved.manifest"
    ))
    .unwrap();
    assert_eq!(native.assets.len(), ea.assets.len());
    for asset in &native.assets {
        let expected = ea
            .assets
            .iter()
            .find(|expected| expected.id == asset.id)
            .unwrap();
        assert!(
            asset.references.iter().eq(expected.references.iter()),
            "{}",
            String::from_utf8_lossy(asset.name)
        );
    }
    let root = scratch_root();
    fs::copy(
        fixtures.join("audio-library.manifest"),
        root.join("audio-library.manifest"),
    )
    .unwrap();
    fs::copy(
        fixtures.join("audio-library.manifest"),
        root.join("worldbuilder.manifest"),
    )
    .unwrap();
    let output = root.join("streams");
    binary_asset_builder::compile::write_compiled_stream(&input, &output, stream).unwrap();
    let fixed = binary_asset_builder::fix_assets::fix_assets(
        &output.join("audio-resolved.manifest"),
        &root,
    )
    .unwrap();
    assert_eq!(fixed.assets, ea.assets.len());
}
