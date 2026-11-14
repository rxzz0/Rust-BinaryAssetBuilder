use super::static_assets::{declaration_id, validate_attributes};
use super::{
    CompiledAsset, Compiler, CustomData, has_content, hash_source_file, hash_xml, invalid,
    memory_stream_hash, required,
};
use roxmltree::Node;
use std::{io, path::Path};

impl Compiler<'_> {
    pub(super) fn compile_on_demand_texture(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        const TYPE_HASH: u32 = 0xf848_8ede;
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
        // EA preserves the DDS bytes even when the XML requests another format.
        prepare_dds(&bytes, mipmaps, resizing).map_err(|error| invalid(&input, error))?;
        let instance_hash = hash_xml(node, TYPE_HASH ^ 4, path)?
            ^ memory_stream_hash(file_hash.to_le_bytes().to_vec());
        let size =
            u32::try_from(bytes.len()).map_err(|_| invalid(&input, "DDS sidecar is too large"))?;
        let words = [
            id.type_id,
            id.instance_id,
            TYPE_HASH,
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
            type_hash: TYPE_HASH,
            instance_hash,
            name: format!("OnDemandTexture:{name}"),
            source: source.to_owned(),
            payload: vec![0; 8],
            relocations: Vec::new(),
            imports: Vec::new(),
            references: Vec::new(),
            custom_data: Some(CustomData::Bytes(cdata)),
            tokenized: false,
            sort_order: 40,
        })
    }
}

fn prepare_dds(bytes: &[u8], mipmaps: bool, resizing: bool) -> Result<(), &'static str> {
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

/// Converts the SDK's uncompressed 24-bit and 32-bit AptUI TGA inputs to
/// single-level A8R8G8B8 DDS data.
pub(super) fn tga_to_dds(bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
    let (header, rest) = bytes
        .split_first_chunk::<18>()
        .ok_or("truncated TGA header")?;
    if header[1] != 0 || header[2] != 2 || header[17] & 0xd0 != 0 {
        return Err("expected an uncompressed, left-origin true-color TGA");
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
    let pixels = rest
        .get(id_size..id_size + row_size * height as usize)
        .ok_or("truncated TGA pixel data")?;

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
