use binary_asset_builder::{compile::compile_static, manifest::Manifest};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn compiles_video_with_the_ra3_sdk_hash_and_binary_layout() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("bab-static-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
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
