//! Byte-oriented hashes used by the original BinaryAssetBuilder and launcher.

const LAUNCHER_CHUNK_SIZE: usize = 0x4000;

/// Hashes bytes using the length as the initial seed.
///
/// The upstream length-seeded entry point returns `0x1337c0de` for empty input.
pub fn fast_hash(bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0x1337_c0de;
    }
    fast_hash_with_seed(bytes.len() as u32, bytes)
}

/// Hashes bytes with an explicit seed. Empty input returns zero.
pub fn fast_hash_with_seed(seed: u32, bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }
    let mut hash = mix_bytes(seed, bytes);
    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 2;
    hash = hash.wrapping_add(hash >> 15);
    hash ^ (hash << 10)
}

/// Hashes bytes in the launcher's 16 KiB blocks, starting with the input length.
pub fn launcher_hash(bytes: &[u8]) -> u32 {
    launcher_hash_with_seed(bytes.len() as u32, bytes)
}

/// Hashes bytes in the launcher's 16 KiB blocks with an explicit seed.
/// Empty input returns zero.
pub fn launcher_hash_with_seed(seed: u32, bytes: &[u8]) -> u32 {
    if bytes.is_empty() {
        return 0;
    }
    bytes.chunks(LAUNCHER_CHUNK_SIZE).fold(seed, |seed, chunk| {
        let mut hash = mix_bytes(seed, chunk);
        hash ^= hash << 3;
        hash = hash.wrapping_add(hash >> 5);
        hash ^= hash << 4;
        hash = hash.wrapping_add(hash >> 17);
        hash ^= hash << 25;
        hash.wrapping_add(hash >> 6)
    })
}

fn mix_bytes(mut hash: u32, bytes: &[u8]) -> u32 {
    let (blocks, tail) = bytes.as_chunks::<4>();
    for block in blocks {
        let low = u32::from(u16::from_le_bytes([block[0], block[1]]));
        let high = u32::from(u16::from_le_bytes([block[2], block[3]]));
        hash = hash.wrapping_add(low);
        hash ^= (high ^ (hash << 5)) << 11;
        hash = hash.wrapping_add(hash >> 11);
    }

    match tail {
        [first] => {
            hash = hash.wrapping_add(u32::from(*first));
            hash ^= hash << 10;
            hash = hash.wrapping_add(hash >> 1);
        }
        [first, second] => {
            hash = hash.wrapping_add(u32::from(u16::from_le_bytes([*first, *second])));
            hash ^= hash << 11;
            hash = hash.wrapping_add(hash >> 17);
        }
        [first, second, third] => {
            hash = hash.wrapping_add(u32::from(u16::from_le_bytes([*first, *second])));
            hash ^= hash << 16;
            hash ^= u32::from(*third) << 18;
            hash = hash.wrapping_add(hash >> 11);
        }
        _ => {}
    }

    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values come from the upstream C# FastHash implementation.
    #[test]
    fn matches_upstream_for_each_tail_length() {
        let vectors: &[(&[u8], u32, u32, u32)] = &[
            (b"", 0x1337_c0de, 0, 0),
            (b"A", 0xdca1_559c, 0xed07_df74, 0xc6e1_6250),
            (b"AB", 0x4206_e86e, 0xa67d_aa03, 0xdcd6_91cf),
            (b"ABC", 0xf7cd_4acd, 0xc55e_99d3, 0xaed9_2e12),
            (b"ABCD", 0x3730_2663, 0x1b72_08ac, 0x79af_7472),
            (b"ABCDE", 0x27dd_b10e, 0x0602_48f4, 0x636a_c6d7),
            (
                b"\xff\x80\x01\x00\xf3\x9a\xd1",
                0x45fa_07db,
                0xd8ce_5eda,
                0x8655_d5a6,
            ),
        ];
        for &(bytes, unseeded, seeded, launcher) in vectors {
            assert_eq!(fast_hash(bytes), unseeded, "{bytes:?}");
            assert_eq!(fast_hash_with_seed(0x89ab_cdef, bytes), seeded, "{bytes:?}");
            assert_eq!(
                launcher_hash_with_seed(0x89ab_cdef, bytes),
                launcher,
                "{bytes:?}"
            );
        }
    }

    #[test]
    fn matches_upstream_at_launcher_chunk_boundaries() {
        let vectors = [
            (0x3fff, 0xaaea_8249, 0xda26_137f),
            (0x4000, 0xe287_3175, 0x0f29_3826),
            (0x4001, 0x5c22_bca0, 0x4ca8_190a),
            (0x8000, 0x2d28_82fd, 0x43e5_2d68),
            (0x8001, 0x5321_e974, 0xa00e_c3e3),
        ];
        for (length, unseeded, seeded) in vectors {
            let bytes: Vec<u8> = (0..length).map(|i| (31 * i + 7) as u8).collect();
            assert_eq!(launcher_hash(&bytes), unseeded, "{length}");
            assert_eq!(
                launcher_hash_with_seed(0x89ab_cdef, &bytes),
                seeded,
                "{length}"
            );
        }
    }
}
