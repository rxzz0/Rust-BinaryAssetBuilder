use binary_asset_builder::linked_stream::link_assets;
use binary_asset_builder::manifest::{ByteOrder, ChunkSizes, Manifest};
use std::io;

const FIXTURES: [&[u8]; 2] = [
    include_bytes!("fixtures/manifest-v5-le.manifest"),
    include_bytes!("fixtures/manifest-v5-be.manifest"),
];

const FIRST_NAME: &str = "942fff2d.132408db.509bd329.13579bdf.asset";
const SECOND_NAME: &str = "7d464170.97d0a46e.abcdef01.2468ace0.asset";
const FIRST_HEADER: [u32; 8] = [
    0x942f_ff2d,
    0x509b_d329,
    0x1324_08db,
    0x1357_9bdf,
    16,
    8,
    4,
    0,
];
const SECOND_HEADER: [u32; 8] = [
    0x7d46_4170,
    0xabcd_ef01,
    0x97d0_a46e,
    0x2468_ace0,
    8,
    4,
    0,
    0,
];
const FIRST_PAYLOAD: &[u8] = b"0123456789abcdefABCDEFGHqrst";
const SECOND_PAYLOAD: &[u8] = b"01234567XYZ!";

#[test]
fn links_chunks_in_manifest_order_with_the_stream_byte_order() {
    for bytes in FIXTURES {
        let mut manifest = Manifest::parse(bytes).unwrap();
        manifest.is_linked = true;

        let mut requested = Vec::new();
        let streams = link_assets(&manifest, |name| {
            requested.push(name.to_owned());
            let (header, payload) = match name {
                FIRST_NAME => (FIRST_HEADER, FIRST_PAYLOAD),
                SECOND_NAME => (SECOND_HEADER, SECOND_PAYLOAD),
                _ => panic!("unexpected asset file {name}"),
            };
            Ok(asset_file(manifest.byte_order, header, payload))
        })
        .unwrap();

        assert_eq!(requested, [FIRST_NAME, SECOND_NAME]);
        let prefix = &bytes[4..8];
        assert_eq!(streams.bin, [prefix, b"0123456789abcdef01234567"].concat());
        assert_eq!(streams.relo, [prefix, b"ABCDEFGHXYZ!"].concat());
        assert_eq!(streams.imp, [prefix, b"qrst"].concat());
    }
}

#[test]
fn skips_chunks_reused_from_a_patch_base() {
    let mut manifest = Manifest::parse(FIXTURES[0]).unwrap();
    manifest.assets[0].chunk_sizes = ChunkSizes {
        instance: 0,
        relocations: 0,
        imports: 0,
    };
    let streams = link_assets(&manifest, |name| {
        assert_eq!(name, SECOND_NAME);
        Ok(asset_file(
            manifest.byte_order,
            SECOND_HEADER,
            SECOND_PAYLOAD,
        ))
    })
    .unwrap();
    assert_eq!(streams.bin, [&FIXTURES[0][4..8], b"01234567"].concat());
}

#[test]
fn rejects_incomplete_and_mismatched_asset_files() {
    let manifest = Manifest::parse(FIXTURES[0]).unwrap();
    let asset = asset_file(manifest.byte_order, FIRST_HEADER, FIRST_PAYLOAD);
    for (bytes, reason) in [
        (&asset[..31], "truncated asset header"),
        (&asset[..asset.len() - 1], "truncated asset payload"),
    ] {
        let error = link_assets(&manifest, |_| Ok(bytes.to_vec())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(reason));
    }

    let mut wrong_hash = asset;
    wrong_hash[8] ^= 1;
    let error = link_assets(&manifest, |_| Ok(wrong_hash.clone())).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("asset header differs"));
}

fn asset_file(byte_order: ByteOrder, header: [u32; 8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for word in header {
        bytes.extend(match byte_order {
            ByteOrder::Little => word.to_le_bytes(),
            ByteOrder::Big => word.to_be_bytes(),
        });
    }
    bytes.extend(payload);
    bytes
}
