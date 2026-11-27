use super::static_assets::{declaration_id, validate_attributes};
use super::{
    CompiledAsset, Compiler, CustomData, has_content, hash_source_file, hash_xml, invalid,
    memory_stream_hash, required,
};
use roxmltree::Node;
use std::{io, path::Path};

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
        if has_content(node)
            || node.attribute("Type").unwrap_or("StandardTexture") != "StandardTexture"
        {
            return Err(invalid(path, "expected a standard on-demand texture"));
        }
        let bool_attr = |name| match node.attribute(name).unwrap_or("true").trim() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(invalid(path, format!("invalid {name}"))),
        };
        let mipmaps = bool_attr("GenerateMipMaps")?;
        let resizing = bool_attr("AllowAutomaticResize")?;
        let input = self.resolve(path, required(node, "File", path)?)?;
        let mut bytes = Vec::new();
        let file_hash = hash_source_file(&input, &mut bytes)?;
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
            bytes = compile_tga(&bytes, mipmaps, resizing, format)
                .map_err(|error| invalid(&input, error))?;
        }
        // EA preserves the DDS bytes even when the XML requests another format.
        prepare_dds(&bytes, mipmaps, resizing).map_err(|error| invalid(&input, error))?;
        let instance_hash = hash_xml(node, type_hash ^ 4, path)?
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
            payload: vec![0; 8],
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

pub(super) fn prepare_dds(bytes: &[u8], mipmaps: bool, resizing: bool) -> Result<(), &'static str> {
    let (header, pixels) = bytes
        .split_first_chunk::<128>()
        .ok_or("truncated DDS header")?;
    let word = |index| u32::from_le_bytes(header.as_chunks::<4>().0[index]);
    if word(0) != u32::from_le_bytes(*b"DDS ") || word(1) != 124 || word(19) != 32 || word(28) != 0
    {
        return Err("expected a 2D DDS texture");
    }
    let (width, height) = (word(4), word(3));
    if width == 0 || height == 0 || width > 16384 || height > 16384 {
        return Err("invalid DDS dimensions");
    }
    if resizing && (!width.is_power_of_two() || !height.is_power_of_two()) {
        return Err("DDS resizing is unsupported");
    }
    let levels = word(7).max(1);
    let full_mips = u32::BITS - width.max(height).leading_zeros();
    if levels > full_mips || mipmaps && levels != full_mips {
        return Err("unsupported DDS mipmap layout");
    }
    let level_size = |w: u32, h: u32| -> Result<usize, &'static str> {
        if word(20) & 4 != 0 {
            let block_size = match &word(21).to_le_bytes() {
                b"DXT1" => 8,
                b"DXT3" | b"DXT5" => 16,
                _ => return Err("unsupported DDS compression"),
            };
            Ok(w.div_ceil(4) as usize * h.div_ceil(4) as usize * block_size)
        } else {
            let pixel_bytes = match word(22) {
                16 => 2,
                24 => 3,
                32 => 4,
                _ => return Err("unsupported DDS pixel size"),
            };
            Ok(w as usize * h as usize * pixel_bytes)
        }
    };
    let mut size = 0;
    for i in 0..levels {
        size += level_size((width >> i).max(1), (height >> i).max(1))?;
    }
    if pixels.len() < size {
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

fn compile_tga(
    bytes: &[u8],
    mipmaps: bool,
    resizing: bool,
    format: &str,
) -> Result<Vec<u8>, &'static str> {
    let mut decoded = tga_to_dds(bytes)?;
    let (width, height) = (
        u32::from_le_bytes(decoded[16..20].try_into().unwrap()) as usize,
        u32::from_le_bytes(decoded[12..16].try_into().unwrap()) as usize,
    );
    if width > 16384 || height > 16384 {
        return Err("invalid TGA dimensions");
    }
    if resizing && (!width.is_power_of_two() || !height.is_power_of_two()) {
        return Err("TGA resizing is unsupported");
    }
    let compressed = match format {
        "DXT1" => true,
        "A8R8G8B8" => false,
        _ => return Err("unsupported TGA OutputFormat"),
    };
    if !compressed && !mipmaps {
        return Ok(decoded);
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
    let storage = if compressed { 0x80000 } else { 0x8 };
    word(2, 0x1007 | storage | if levels > 1 { 0x2_0000 } else { 0 });
    word(7, if levels > 1 { levels } else { 0 });
    word(27, if levels > 1 { 0x401008 } else { 0x1000 });
    if compressed {
        word(5, (width.div_ceil(4) * height.div_ceil(4) * 8) as u32);
        word(20, 4);
        word(21, u32::from_le_bytes(*b"DXT1"));
        for index in 22..27 {
            word(index, 0);
        }
    } else {
        word(5, (width * 4) as u32);
    }

    let (mut width, mut height) = (width, height);
    let mut pixels = std::borrow::Cow::Borrowed(pixels.as_chunks::<4>().0);
    for level in 0..levels {
        if compressed {
            for y in (0..height).step_by(4) {
                for x in (0..width).step_by(4) {
                    let block = std::array::from_fn(|i| {
                        pixels[(y + i / 4).min(height - 1) * width + (x + i % 4).min(width - 1)]
                    });
                    // The BC3 color block is also a BC1 block for opaque pixels.
                    let encoded = super::terrain_atlas::encode_bc3(&block);
                    dds.extend_from_slice(&encoded[8..]);
                }
            }
        } else {
            dds.extend_from_slice(pixels.as_flattened());
        }
        if level + 1 < levels {
            let next_width = (width / 2).max(1);
            let next_height = (height / 2).max(1);
            let mut next = Vec::with_capacity(next_width * next_height);
            for y in 0..next_height {
                for x in 0..next_width {
                    let a = pixels[(y * 2) * width + x * 2];
                    let b = pixels[(y * 2) * width + (x * 2 + 1).min(width - 1)];
                    let c = pixels[(y * 2 + 1).min(height - 1) * width + x * 2];
                    let d =
                        pixels[(y * 2 + 1).min(height - 1) * width + (x * 2 + 1).min(width - 1)];
                    next.push(std::array::from_fn(|i| {
                        ((u16::from(a[i])
                            + u16::from(b[i])
                            + u16::from(c[i])
                            + u16::from(d[i])
                            + 2)
                            / 4) as u8
                    }));
                }
            }
            pixels = std::borrow::Cow::Owned(next);
            width = next_width;
            height = next_height;
        }
    }
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
    fn static_tga_encodes_dxt1_mipmaps_and_uncompressed_alpha() {
        let mut tga = vec![0; 18];
        tga[2] = 2;
        tga[12] = 4;
        tga[14] = 4;
        tga[16] = 32;
        tga[17] = 0x28;
        tga.extend([0, 0, 255, 127].repeat(16));

        let dxt1 = compile_tga(&tga, true, true, "DXT1").unwrap();
        prepare_dds(&dxt1, true, true).unwrap();
        assert_eq!(&dxt1[84..88], b"DXT1");
        assert_eq!(dxt1[28], 3);
        assert_eq!(dxt1.len(), 128 + 3 * 8);
        for block in dxt1[128..].as_chunks::<8>().0 {
            assert_eq!(block, &[0, 0xf8, 0, 0, 0, 0, 0, 0]);
        }

        let rgba = compile_tga(&tga, false, false, "A8R8G8B8").unwrap();
        prepare_dds(&rgba, false, false).unwrap();
        assert_eq!(rgba.len(), 128 + 16 * 4);
        assert_eq!(&rgba[128..132], &[0, 0, 255, 127]);
    }
}
