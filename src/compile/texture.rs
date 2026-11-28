use super::static_assets::{declaration_id, validate_attributes};
use super::{
    CompiledAsset, Compiler, CustomData, has_content, hash_source_file_with, hash_xml, invalid,
    memory_stream_hash, required,
};
use crate::hash::fast_hash_with_signed_tail;
use roxmltree::Node;
use std::{io, path::Path};

mod bc1;
mod resize;

pub(super) const TEXTURE_COMPILER_VERSION: u32 = 0x106a;

impl Compiler<'_> {
    pub(super) fn compile_texture(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let on_demand = node.tag_name().name() == "OnDemandTexture";
        let type_hash = if on_demand { 0xf848_8ede } else { 0x9f5f_f8da };
        let (name, id) = declaration_id(node, path)?;
        validate_attributes(
            node,
            &[
                "id",
                "File",
                "Type",
                "GenerateMipMaps",
                "OutputFormat",
                "AllowAutomaticResize",
            ],
            path,
        )?;
        if has_content(node) {
            return Err(invalid(path, "expected an empty texture declaration"));
        }
        let kind = node.attribute("Type").unwrap_or("StandardTexture");
        let texture_type = match kind {
            "StandardTexture" => 0u32,
            "VolumeTexture" => 1,
            "CubeTexture" => 2,
            _ => return Err(invalid(path, format!("invalid texture Type {kind:?}"))),
        };
        let bool_attr = |name| match node.attribute(name).unwrap_or("true").trim() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(invalid(path, format!("invalid {name}"))),
        };
        let mipmaps = bool_attr("GenerateMipMaps")?;
        bool_attr("AllowAutomaticResize")?;
        let input = self.resolve(path, required(node, "File", path)?)?;
        let mut bytes = Vec::new();
        let file_hash = hash_source_file_with(&input, &mut bytes, fast_hash_with_signed_tail)?;
        let format = node.attribute("OutputFormat").unwrap_or("DXT1");
        if !matches!(
            format,
            "DXT1" | "DXT1A" | "DXT3" | "DXT5" | "A8R8G8B8" | "X8R8G8B8" | "R5G6B5"
        ) {
            return Err(invalid(path, "invalid texture OutputFormat"));
        }
        if input
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("tga"))
            && !bytes.starts_with(b"DDS ")
        {
            if texture_type != 0 {
                return Err(invalid(
                    &input,
                    format!("TGA conversion for {kind} is unsupported"),
                ));
            }
            bytes = compile_tga(&bytes, mipmaps, format).map_err(|error| invalid(&input, error))?;
        }
        // EA applies conversion settings to source images and passes through prebuilt DDS data.
        validate_dds(&bytes).map_err(|error| invalid(&input, error))?;
        let instance_hash = hash_xml(node, type_hash ^ TEXTURE_COMPILER_VERSION, path)?
            ^ memory_stream_hash(file_hash.to_le_bytes().to_vec());
        let size =
            u32::try_from(bytes.len()).map_err(|_| invalid(&input, "DDS sidecar is too large"))?;
        if !on_demand {
            let mut payload: Vec<_> = [0u32, 12, size]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            payload.extend(bytes);
            payload.resize(payload.len().next_multiple_of(4), 0);
            return Ok(CompiledAsset {
                id,
                type_hash,
                instance_hash,
                name: format!("Texture:{name}"),
                source: source.to_owned(),
                payload,
                relocations: [4u32, u32::MAX]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect(),
                imports: Vec::new(),
                references: Vec::new(),
                weak_references: Vec::new(),
                custom_data: None,
                tokenized: false,
                sort_order: 47,
            });
        }
        let words = [
            id.type_id,
            id.instance_id,
            type_hash,
            instance_hash,
            size.checked_add(8)
                .ok_or_else(|| invalid(&input, "DDS sidecar is too large"))?,
            8,
            0,
            0,
            8,
            size,
        ];
        let mut cdata: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        cdata.extend(bytes);
        cdata.extend(0u32.to_le_bytes());
        cdata.extend(u32::MAX.to_le_bytes());
        Ok(CompiledAsset {
            id,
            type_hash,
            instance_hash,
            name: format!("OnDemandTexture:{name}"),
            source: source.to_owned(),
            payload: [0, texture_type]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect(),
            relocations: Vec::new(),
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            custom_data: Some(CustomData::Bytes(cdata)),
            tokenized: false,
            sort_order: 40,
        })
    }
}

pub(super) fn validate_dds(bytes: &[u8]) -> Result<(), &'static str> {
    const CUBEMAP: u32 = 0x0200;
    const CUBEMAP_FACES: u32 = 0xfc00;
    const VOLUME: u32 = 0x0020_0000;

    let (header, pixels) = bytes
        .split_first_chunk::<128>()
        .ok_or("truncated DDS header")?;
    let word = |index| u32::from_le_bytes(header.as_chunks::<4>().0[index]);
    if word(0) != u32::from_le_bytes(*b"DDS ") || word(1) != 124 || word(19) != 32 {
        return Err("invalid DDS header");
    }
    let caps = word(28);
    let (faces, depth) = match caps {
        0 => (1, 1),
        VOLUME => (1, word(6)),
        caps if caps & !CUBEMAP_FACES == CUBEMAP && caps & CUBEMAP_FACES != 0 => {
            ((caps & CUBEMAP_FACES).count_ones(), 1)
        }
        _ => return Err("unsupported DDS surface layout"),
    };
    let (width, height) = (word(4), word(3));
    if [width, height, depth]
        .into_iter()
        .any(|value| !(1..=16384).contains(&value))
    {
        return Err("invalid DDS dimensions");
    }
    let levels = word(7).max(1);
    let full_mips = u32::BITS - width.max(height).max(depth).leading_zeros();
    if levels > full_mips {
        return Err("unsupported DDS mipmap layout");
    }
    let level_size = |w: u32, h: u32| -> Result<u64, &'static str> {
        if word(20) & 4 != 0 {
            let block_size = match &word(21).to_le_bytes() {
                b"DXT1" => 8,
                b"DXT3" | b"DXT5" => 16,
                _ => return Err("unsupported DDS compression"),
            };
            Ok(u64::from(w.div_ceil(4)) * u64::from(h.div_ceil(4)) * block_size)
        } else {
            let pixel_bytes = match word(22) {
                16 => 2,
                24 => 3,
                32 => 4,
                _ => return Err("unsupported DDS pixel size"),
            };
            Ok(u64::from(w) * u64::from(h) * pixel_bytes)
        }
    };
    let mut size = 0;
    for i in 0..levels {
        size +=
            level_size((width >> i).max(1), (height >> i).max(1))? * u64::from((depth >> i).max(1));
    }
    if (pixels.len() as u64) < size * u64::from(faces) {
        return Err("truncated DDS pixel data");
    }
    Ok(())
}

/// Converts 24-bit and 32-bit true-color TGAs to single-level A8R8G8B8 DDS data.
pub(super) fn tga_to_dds(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let (header, rest) = bytes
        .split_first_chunk::<18>()
        .ok_or("truncated TGA header")?;
    if header[1] != 0 || !matches!(header[2], 2 | 10) || header[17] & 0xd0 != 0 {
        return Err("expected a left-origin true-color TGA");
    }
    let bytes_per_pixel = match header[16] {
        24 => 3,
        32 => 4,
        _ => return Err("expected 24-bit or 32-bit TGA pixels"),
    };
    let width = u32::from(u16::from_le_bytes([header[12], header[13]]));
    let height = u32::from(u16::from_le_bytes([header[14], header[15]]));
    let pixel_count = width
        .checked_mul(height)
        .filter(|&count| count > 0)
        .ok_or("invalid TGA dimensions")?;
    let linear_size = pixel_count
        .checked_mul(4)
        .filter(|&size| size <= i32::MAX as u32 - 140)
        .ok_or("DDS is too large")?;
    let id_size = usize::from(header[0]);
    let row_size = width as usize * bytes_per_pixel;
    let image = rest.get(id_size..).ok_or("truncated TGA image ID")?;
    let pixels = if header[2] == 10 {
        std::borrow::Cow::Owned(decode_tga_rle(
            image,
            bytes_per_pixel,
            pixel_count as usize,
        )?)
    } else {
        std::borrow::Cow::Borrowed(
            image
                .get(..row_size * height as usize)
                .ok_or("truncated TGA pixel data")?,
        )
    };

    let mut dds_header = [0u32; 32];
    dds_header[0] = u32::from_le_bytes(*b"DDS ");
    dds_header[1] = 124;
    dds_header[2] = 0x81007;
    dds_header[3] = height;
    dds_header[4] = width;
    dds_header[5] = linear_size;
    dds_header[19] = 32;
    dds_header[20] = 0x41; // DDPF_RGB | DDPF_ALPHAPIXELS
    dds_header[22] = 32;
    dds_header[23..27].copy_from_slice(&[0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000]);
    dds_header[27] = 0x1000; // DDSCAPS_TEXTURE

    let mut dds = Vec::with_capacity(128 + linear_size as usize);
    dds.extend(dds_header.into_iter().flat_map(u32::to_le_bytes));
    let top_origin = header[17] & 0x20 != 0;
    for y in 0..height as usize {
        let source_y = if top_origin {
            y
        } else {
            height as usize - 1 - y
        };
        let row = &pixels[source_y * row_size..(source_y + 1) * row_size];
        if bytes_per_pixel == 4 {
            dds.extend_from_slice(row);
        } else {
            for &[b, g, r] in row.as_chunks::<3>().0 {
                dds.extend([b, g, r, 0xff]);
            }
        }
    }
    Ok(dds)
}

fn decode_tga_rle(
    mut image: &[u8],
    bytes_per_pixel: usize,
    mut pixels_left: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut pixels = Vec::new();
    while pixels_left > 0 {
        let (&packet, rest) = image.split_first().ok_or("truncated TGA RLE packet")?;
        let count = usize::from(packet & 0x7f) + 1;
        if count > pixels_left {
            return Err("TGA RLE packet exceeds image size");
        }
        let repeated = packet & 0x80 != 0;
        let stored_pixels = if repeated { 1 } else { count };
        let (payload, rest) = rest
            .split_at_checked(stored_pixels * bytes_per_pixel)
            .ok_or("truncated TGA RLE pixels")?;
        if repeated {
            for _ in 0..count {
                pixels.extend_from_slice(payload);
            }
        } else {
            pixels.extend_from_slice(payload);
        }
        pixels_left -= count;
        image = rest;
    }
    Ok(pixels)
}

fn compile_tga(bytes: &[u8], mipmaps: bool, format: &str) -> Result<Vec<u8>, &'static str> {
    let mut decoded = tga_to_dds(bytes)?;
    let (mut width, mut height) = (
        u32::from_le_bytes(decoded[16..20].try_into().unwrap()) as usize,
        u32::from_le_bytes(decoded[12..16].try_into().unwrap()) as usize,
    );
    if width > 16384 || height > 16384 {
        return Err("invalid TGA dimensions");
    }
    // These EA conversion paths round up to powers of two even when resize is disabled.
    if (!width.is_power_of_two() || !height.is_power_of_two())
        && matches!(format, "A8R8G8B8" | "DXT1")
    {
        decoded = resize::uncompressed_dds(decoded, width, height)?;
        width = width.next_power_of_two();
        height = height.next_power_of_two();
    }
    let compressed = match format {
        "DXT1" => true,
        "A8R8G8B8" => false,
        _ => return Err("unsupported TGA OutputFormat"),
    };
    if !compressed {
        return if mipmaps {
            resize::append_uncompressed_mips(decoded, width, height)
        } else {
            Ok(decoded)
        };
    }

    let (header, pixels) = decoded.split_at_mut(128);
    let levels = if mipmaps {
        usize::BITS - width.max(height).leading_zeros()
    } else {
        1
    };
    let mut dds = header.to_vec();
    let mut word = |index: usize, value: u32| {
        dds[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
    };
    word(2, 0x81007 | if mipmaps { 0x2_0000 } else { 0 });
    word(7, if mipmaps { levels } else { 0 });
    word(27, if mipmaps { 0x401008 } else { 0x1000 });
    word(5, (width.div_ceil(4) * height.div_ceil(4) * 8) as u32);
    word(20, 4);
    word(21, u32::from_le_bytes(*b"DXT1"));
    for index in 22..27 {
        word(index, 0);
    }

    bc1::append_levels(&mut dds, pixels.as_chunks::<4>().0, width, height, levels)?;
    Ok(dds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tga_rle_decodes_run_and_raw_packets_in_image_order() {
        let mut tga = vec![0; 18];
        tga[0] = 1; // Image ID length.
        tga[2] = 10;
        tga[12] = 2;
        tga[14] = 2;
        tga[16] = 32;
        tga[17] = 8; // Bottom-left origin, eight alpha bits.
        tga.extend([0, 0x81, 1, 2, 3, 4, 1, 5, 6, 7, 8, 9, 10, 11, 12]);

        let dds = tga_to_dds(&tga).unwrap();
        assert_eq!(
            &dds[128..],
            &[5, 6, 7, 8, 9, 10, 11, 12, 1, 2, 3, 4, 1, 2, 3, 4]
        );

        tga.pop();
        assert_eq!(tga_to_dds(&tga), Err("truncated TGA RLE pixels"));
        tga[19] = 0x84;
        assert_eq!(tga_to_dds(&tga), Err("TGA RLE packet exceeds image size"));
    }

    #[test]
    fn tga_resize_matches_ea_on_narrow_odd_rows() {
        let mut tga = vec![0; 18];
        tga[2] = 2;
        tga[12] = 2;
        tga[14] = 3;
        tga[16] = 32;
        tga[17] = 0x28;
        tga.extend([
            0, 0, 0, 23, 13, 0, 31, 62, 7, 31, 0, 64, 20, 31, 31, 103, 14, 62, 0, 105, 27, 62, 31,
            144,
        ]);
        let expected = [
            6, 12, 8, 49, 12, 12, 23, 68, 8, 20, 8, 59, 14, 19, 23, 78, 13, 43, 8, 89, 19, 42, 23,
            108, 15, 50, 8, 99, 21, 50, 23, 118,
        ];
        let dds = compile_tga(&tga, false, "A8R8G8B8").unwrap();
        assert_eq!(&dds[12..24], &[4, 0, 0, 0, 2, 0, 0, 0, 32, 0, 0, 0]);
        assert_eq!(&dds[128..], &expected);

        let mipped = compile_tga(&tga, true, "A8R8G8B8").unwrap();
        validate_dds(&mipped).unwrap();
        assert_eq!(&mipped[8..12], &0xa1007u32.to_le_bytes());
        assert_eq!(&mipped[28..32], &3u32.to_le_bytes());
        assert_eq!(&mipped[108..112], &0x401008u32.to_le_bytes());
        assert_eq!(&mipped[128..160], &expected);
        assert_eq!(
            &mipped[160..],
            &[11, 20, 16, 69, 16, 42, 16, 99, 14, 31, 16, 84]
        );

        tga[12] = 1;
        tga[14] = 1;
        tga.truncate(22);
        let single = compile_tga(&tga, true, "A8R8G8B8").unwrap();
        validate_dds(&single).unwrap();
        assert_eq!(&single[8..12], &0xa1007u32.to_le_bytes());
        assert_eq!(&single[28..32], &1u32.to_le_bytes());
        assert_eq!(&single[108..112], &0x401008u32.to_le_bytes());
        assert_eq!(&single[128..], &tga[18..]);
    }

    #[test]
    fn tga_resized_dxt1_matches_ea_through_the_last_mip() {
        let mut tga = vec![0; 18];
        tga[2] = 2;
        tga[12] = 3;
        tga[14] = 5;
        tga[16] = 32;
        tga[17] = 0x28;
        tga.extend([
            64, 65, 243, 255, 84, 135, 216, 255, 108, 102, 159, 255, 204, 191, 224, 255, 231, 61,
            126, 255, 115, 32, 173, 255, 10, 117, 112, 255, 3, 36, 30, 255, 117, 34, 16, 255, 169,
            36, 121, 255, 142, 248, 109, 255, 67, 242, 124, 255, 242, 208, 97, 255, 48, 49, 220,
            255, 181, 216, 210, 255,
        ]);
        let dds = compile_tga(&tga, true, "DXT1").unwrap();
        validate_dds(&dds).unwrap();
        assert_eq!(&dds[12..24], &[8, 0, 0, 0, 4, 0, 0, 0, 16, 0, 0, 0]);
        assert_eq!(&dds[28..32], &4u32.to_le_bytes());
        assert_eq!(
            &dds[128..],
            &[
                175, 203, 110, 73, 0, 160, 248, 94, 178, 174, 12, 99, 85, 141, 47, 190, 79, 157,
                78, 130, 255, 87, 171, 2, 110, 109, 14, 147, 85, 170, 170, 170, 78, 139, 46, 132,
                170, 171, 171, 171,
            ]
        );
    }

    #[test]
    fn dds_checks_cube_faces_and_volume_mip_slices() {
        for (caps, depth, levels, blocks) in [
            (0xfe00, 0, 2, 30),       // Six faces with 8x8 and 4x4 levels.
            (0x0600, 0, 2, 5),        // One face with the same levels.
            (0x0020_0000, 16, 5, 79), // Volume down to 1x1x1.
        ] {
            let mut header = [0u32; 32];
            header[0] = u32::from_le_bytes(*b"DDS ");
            header[1] = 124;
            header[3] = 8;
            header[4] = 8;
            header[6] = depth;
            header[7] = levels;
            header[19] = 32;
            header[20] = 4;
            header[21] = u32::from_le_bytes(*b"DXT3");
            header[28] = caps;
            let mut dds: Vec<_> = header.into_iter().flat_map(u32::to_le_bytes).collect();
            dds.resize(128 + blocks * 16, 0);
            assert_eq!(validate_dds(&dds), Ok(()));
            dds.pop();
            assert_eq!(validate_dds(&dds), Err("truncated DDS pixel data"));
        }
    }

    #[test]
    fn static_tga_encodes_dxt1_mipmaps_and_uncompressed_alpha() {
        let mut tga = vec![0; 18];
        tga[2] = 2;
        tga[12] = 4;
        tga[14] = 4;
        tga[16] = 32;
        tga[17] = 0x28;
        tga.extend([0, 0, 255, 255].repeat(16));

        let dxt1 = compile_tga(&tga, true, "DXT1").unwrap();
        validate_dds(&dxt1).unwrap();
        assert_eq!(&dxt1[84..88], b"DXT1");
        assert_eq!(dxt1[28], 3);
        assert_eq!(dxt1.len(), 128 + 3 * 8);
        assert_eq!(
            &dxt1[128..],
            &[
                1, 0xf8, 0, 0xf8, 0x55, 0x55, 0x55, 0x55, 1, 0xf8, 0, 0xf8, 0x55, 0x55, 0x5d, 0x5d,
                1, 0xf8, 0, 0xf8, 0x55, 0x57, 0x57, 0x57,
            ]
        );

        tga[21] = 127;
        let rgba = compile_tga(&tga, false, "A8R8G8B8").unwrap();
        validate_dds(&rgba).unwrap();
        assert_eq!(rgba.len(), 128 + 16 * 4);
        assert_eq!(&rgba[128..132], &[0, 0, 255, 127]);
    }
}
