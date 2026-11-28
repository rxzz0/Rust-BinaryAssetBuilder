use super::static_assets::{declaration_id, significant_children, validate_attributes};
use super::texture::tga_to_dds;
use super::{
    CompiledAsset, Compiler, SAGE_NS, hash_source_file, hash_xml, invalid, memory_stream_hash,
    required,
};
use roxmltree::Node;
use std::{io, path::Path};

const TYPE_HASH: u32 = 0x18e9_13ea;
const TILE_SIZE: usize = 256;
const BORDER: usize = 16;
const STRIDE: usize = TILE_SIZE + 2 * BORDER;
const MIP_LEVELS: usize = 5;
const MAX_SIZE: usize = 8192;

type Pixel = [u8; 4];

impl Compiler<'_> {
    pub(super) fn compile_terrain_atlas(
        &self,
        node: Node<'_, '_>,
        path: &Path,
        source: &str,
    ) -> io::Result<CompiledAsset> {
        let (name, id) = declaration_id(node, path)?;
        validate_attributes(node, &["id", "AtlasSize", "AllowLossyCompression"], path)?;
        let nominal = integer(node.attribute("AtlasSize").unwrap_or("2048"), path)? as usize;
        if nominal > MAX_SIZE {
            return Err(invalid(path, "terrain atlas size exceeds 8192"));
        }
        if !matches!(
            node.attribute("AllowLossyCompression")
                .unwrap_or("true")
                .trim(),
            "true" | "false" | "1" | "0"
        ) {
            return Err(invalid(path, "invalid terrain AllowLossyCompression"));
        }
        let tiles: Vec<_> = significant_children(node).collect();
        if tiles.is_empty() {
            return Err(invalid(path, "terrain atlas requires at least one tile"));
        }
        let (width, height) = atlas_dimensions(nominal, tiles.len(), path)?;
        let mut base = vec![[0; 4]; width * height];
        let mut normal = vec![[0; 4]; width * height];
        let columns = width / STRIDE;
        let mut records = Vec::with_capacity(tiles.len());
        let mut file_hashes = Vec::with_capacity(tiles.len() * 8);
        for (index, tile) in tiles.into_iter().enumerate() {
            if !tile.has_tag_name((SAGE_NS, "Tile")) {
                return Err(invalid(path, "expected terrain atlas Tile"));
            }
            validate_attributes(tile, &["TextureID", "BaseTexture", "NormalTexture"], path)?;
            for cell in significant_children(tile) {
                if !cell.has_tag_name((SAGE_NS, "Cell")) || super::has_content(cell) {
                    return Err(invalid(path, "expected terrain atlas Cell"));
                }
                validate_attributes(cell, &["IndexX", "IndexY"], path)?;
                for axis in ["IndexX", "IndexY"] {
                    required(cell, axis, path)?
                        .trim()
                        .parse::<u8>()
                        .map_err(|_| invalid(path, "invalid terrain atlas cell index"))?;
                }
            }
            let x = index % columns * STRIDE;
            let y = index / columns * STRIDE;
            for (attribute, pixels) in [("BaseTexture", &mut base), ("NormalTexture", &mut normal)]
            {
                let image = self.resolve(path, required(tile, attribute, path)?)?;
                let mut bytes = Vec::new();
                let file_hash = hash_source_file(&image, &mut bytes)?;
                file_hashes.extend(file_hash.to_le_bytes());
                let dds = tga_to_dds(&bytes).map_err(|error| invalid(&image, error))?;
                if dds[12..20] != [0, 1, 0, 0, 0, 1, 0, 0] {
                    return Err(invalid(&image, "expected a 256×256 terrain tile"));
                }
                let (tile, _) = dds[128..].as_chunks::<4>();
                copy_tile(pixels, width, x, y, tile);
            }
            let left = (x + BORDER) as u16;
            let top = (y + BORDER) as u16;
            records.push((
                integer(required(tile, "TextureID", path)?, path)?,
                [left, top, left + TILE_SIZE as u16, top + TILE_SIZE as u16],
            ));
        }
        records.sort_by_key(|&(id, _)| id);

        let mut payload = vec![0; 28];
        let base_size = write_dds(&mut payload, base, width, height, true);
        let normal_offset = payload.len() as u32;
        let normal_size = write_dds(&mut payload, normal, width, height, false);
        let tile_offset = payload.len() as u32;
        let count = records.len() as u32;
        for (id, bounds) in records {
            payload.extend(id.to_le_bytes());
            for edge in bounds {
                payload.extend(edge.to_le_bytes());
            }
        }
        for (slot, value) in payload[..28].as_chunks_mut::<4>().0.iter_mut().zip([
            0,
            count,
            tile_offset,
            28,
            base_size,
            normal_offset,
            normal_size,
        ]) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        let offsets = [12, 20, 8, u32::MAX];
        let relocations = offsets
            .iter()
            .flat_map(|offset| offset.to_le_bytes())
            .collect();
        let instance_hash = hash_xml(node, 0x467d_00b6, path)? ^ memory_stream_hash(file_hashes);
        Ok(CompiledAsset {
            id,
            type_hash: TYPE_HASH,
            instance_hash,
            name: format!("TerrainTextureAtlas:{name}"),
            source: source.to_owned(),
            payload,
            relocations,
            imports: Vec::new(),
            references: Vec::new(),
            weak_references: Vec::new(),
            custom_data: None,
            tokenized: false,
            sort_order: 79,
        })
    }
}

fn integer(text: &str, path: &Path) -> io::Result<u32> {
    let text = text.trim();
    text.strip_prefix("0x")
        .map_or_else(|| text.parse(), |hex| u32::from_str_radix(hex, 16))
        .map_err(|_| invalid(path, format!("invalid terrain atlas number {text:?}")))
}

fn atlas_dimensions(nominal: usize, count: usize, path: &Path) -> io::Result<(usize, usize)> {
    let nominal = nominal.next_power_of_two().max(512);
    let (mut width, mut height) = (nominal, nominal);
    while (width / STRIDE) * (height / STRIDE) < count {
        if width == height {
            width *= 2;
        } else {
            height *= 2;
        }
        if width > MAX_SIZE || height > MAX_SIZE {
            return Err(invalid(path, "terrain tiles exceed the maximum atlas size"));
        }
    }
    Ok((width, height))
}

fn copy_tile(pixels: &mut [Pixel], width: usize, x: usize, y: usize, tile: &[Pixel]) {
    for row in 0..STRIDE {
        let source_y = (row + TILE_SIZE - BORDER) % TILE_SIZE;
        let source = &tile[source_y * TILE_SIZE..(source_y + 1) * TILE_SIZE];
        let start = (y + row) * width + x;
        let destination = &mut pixels[start..start + STRIDE];
        destination[..BORDER].copy_from_slice(&source[TILE_SIZE - BORDER..]);
        destination[BORDER..BORDER + TILE_SIZE].copy_from_slice(source);
        destination[BORDER + TILE_SIZE..].copy_from_slice(&source[..BORDER]);
    }
}

fn write_dds(out: &mut Vec<u8>, pixels: Vec<Pixel>, width: usize, height: usize, bc3: bool) -> u32 {
    let start = out.len();
    let mut header = [0u32; 32];
    header[..8].copy_from_slice(&[
        u32::from_le_bytes(*b"DDS "),
        124,
        0x000a_1007,
        height as u32,
        width as u32,
        (width * height * if bc3 { 1 } else { 2 }) as u32,
        0,
        MIP_LEVELS as u32,
    ]);
    header[19] = 32;
    if bc3 {
        header[20] = 4;
        header[21] = u32::from_le_bytes(*b"DXT5");
    } else {
        header[20] = 0x40;
        header[22..26].copy_from_slice(&[16, 0x7c00, 0x03e0, 0x001f]);
    }
    header[27] = 0x0040_1008;
    out.extend(header.into_iter().flat_map(u32::to_le_bytes));
    for level in 0..MIP_LEVELS {
        let scale = 1 << level;
        let (mip_width, mip_height) = (width / scale, height / scale);
        if bc3 {
            let mip;
            let pixels = if level == 0 {
                &pixels[..]
            } else {
                mip = triangle_mip(&pixels, width, height, scale, |pixel, divisor| {
                    pixel.map(|value| ((value + divisor / 2) / divisor) as u8)
                });
                &mip
            };
            for y in (0..mip_height).step_by(4) {
                for x in (0..mip_width).step_by(4) {
                    let block =
                        std::array::from_fn(|i| pixels[(y + i / 4) * mip_width + x + i % 4]);
                    out.extend(encode_bc3(&block));
                }
            }
        } else if level == 0 {
            for pixel in &pixels {
                out.extend(normal_word(pixel.map(u32::from), 1).to_le_bytes());
            }
        } else {
            let mip = triangle_mip(&pixels, width, height, scale, normal_word);
            for word in mip {
                out.extend(word.to_le_bytes());
            }
        }
    }
    (out.len() - start) as u32
}

fn normal_word([b, g, r, _]: [u32; 4], divisor: u32) -> u16 {
    // EA quantizes directly from the filtered values into the 5-bit DDS channels.
    let denominator = 255 * divisor;
    let quantize = |value| ((31 * value + denominator / 2) / denominator) as u16;
    quantize(r) << 10 | quantize(g) << 5 | quantize(b)
}

fn triangle_mip<T>(
    pixels: &[Pixel],
    width: usize,
    height: usize,
    scale: usize,
    convert: impl Fn([u32; 4], u32) -> T,
) -> Vec<T> {
    let (mip_width, mip_height) = (width / scale, height / scale);
    let weights: Vec<_> = (0..scale * 2)
        .map(|i| (2 * i.min(scale * 2 - 1 - i) + 1) as u32)
        .collect();
    let taps = |index: usize, extent: usize| {
        let origin = index * scale;
        let first = origin.saturating_sub(scale / 2);
        let skipped = (scale / 2).saturating_sub(origin);
        let end = (skipped + extent - first).min(weights.len());
        (first, &weights[skipped..end])
    };
    let columns: Vec<_> = (0..mip_width)
        .map(|x| {
            let (first, weights) = taps(x, width);
            (first, weights, weights.iter().sum::<u32>())
        })
        .collect();
    let mut horizontal = vec![[0u32; 4]; mip_width * height];
    for (source, target) in pixels
        .chunks_exact(width)
        .zip(horizontal.chunks_exact_mut(mip_width))
    {
        for (result, &(first, weights, _)) in target.iter_mut().zip(&columns) {
            for (sample, &weight) in source[first..].iter().zip(weights) {
                for (channel, &sample) in result.iter_mut().zip(sample) {
                    *channel += u32::from(sample) * weight;
                }
            }
        }
    }
    let mut output = Vec::with_capacity(mip_width * mip_height);
    for y in 0..mip_height {
        let (first, weights) = taps(y, height);
        let vertical_divisor = weights.iter().sum::<u32>();
        for (x, &(_, _, horizontal_divisor)) in columns.iter().enumerate() {
            let mut channels = [0u32; 4];
            for (tap, &weight) in weights.iter().enumerate() {
                for (channel, &sample) in channels
                    .iter_mut()
                    .zip(&horizontal[(first + tap) * mip_width + x])
                {
                    *channel += sample * weight;
                }
            }
            output.push(convert(channels, horizontal_divisor * vertical_divisor));
        }
    }
    output
}

pub(super) fn encode_bc3(pixels: &[Pixel; 16]) -> [u8; 16] {
    if pixels[1..].iter().all(|pixel| *pixel == pixels[0]) {
        let mut block = [0; 16];
        block[0] = pixels[0][3];
        block[8..10].copy_from_slice(&rgb565(pixels[0]).to_le_bytes());
        return block;
    }
    let mut alpha_min = u8::MAX;
    let mut alpha_max = 0;
    let mut darkest = pixels[0];
    let mut lightest = pixels[0];
    let mut min_luma = luminance(darkest);
    let mut max_luma = min_luma;
    for &pixel in pixels {
        alpha_min = alpha_min.min(pixel[3]);
        alpha_max = alpha_max.max(pixel[3]);
        let luma = luminance(pixel);
        if luma < min_luma {
            min_luma = luma;
            darkest = pixel;
        }
        if luma > max_luma {
            max_luma = luma;
            lightest = pixel;
        }
    }
    let alpha_palette = [
        alpha_max,
        alpha_min,
        blend(alpha_max, alpha_min, 6, 7),
        blend(alpha_max, alpha_min, 5, 7),
        blend(alpha_max, alpha_min, 4, 7),
        blend(alpha_max, alpha_min, 3, 7),
        blend(alpha_max, alpha_min, 2, 7),
        blend(alpha_max, alpha_min, 1, 7),
    ];
    let (mut color0, mut color1) = (rgb565(lightest), rgb565(darkest));
    if color0 < color1 {
        std::mem::swap(&mut color0, &mut color1);
    }
    let c0 = expand565(color0);
    let c1 = expand565(color1);
    let palette = [
        c0,
        c1,
        std::array::from_fn(|i| blend(c0[i], c1[i], 2, 3)),
        std::array::from_fn(|i| blend(c0[i], c1[i], 1, 3)),
    ];
    let mut alpha_indices = 0u64;
    let mut color_indices = 0u32;
    for (i, &[b, g, r, alpha]) in pixels.iter().enumerate() {
        let alpha_index = (0..8)
            .min_by_key(|&i| alpha.abs_diff(alpha_palette[i]))
            .unwrap();
        alpha_indices |= (alpha_index as u64) << (3 * i);
        let index = (0..4)
            .min_by_key(|&i| {
                let [db, dg, dr] = palette[i];
                distance(b, db) + distance(g, dg) + distance(r, dr)
            })
            .unwrap();
        color_indices |= (index as u32) << (2 * i);
    }
    let mut block = [0; 16];
    block[..2].copy_from_slice(&[alpha_max, alpha_min]);
    block[2..8].copy_from_slice(&alpha_indices.to_le_bytes()[..6]);
    block[8..10].copy_from_slice(&color0.to_le_bytes());
    block[10..12].copy_from_slice(&color1.to_le_bytes());
    block[12..].copy_from_slice(&color_indices.to_le_bytes());
    block
}

fn luminance([b, g, r, _]: Pixel) -> u32 {
    29 * u32::from(b) + 150 * u32::from(g) + 77 * u32::from(r)
}

fn blend(a: u8, b: u8, weight: u16, total: u16) -> u8 {
    ((u16::from(a) * weight + u16::from(b) * (total - weight)) / total) as u8
}

fn distance(a: u8, b: u8) -> u32 {
    u32::from(a.abs_diff(b)).pow(2)
}

fn quantize(value: u8, max: u16) -> u16 {
    (u16::from(value) * max + 127) / 255
}

fn rgb565([b, g, r, _]: Pixel) -> u16 {
    quantize(r, 31) << 11 | quantize(g, 63) << 5 | quantize(b, 31)
}

fn expand565(value: u16) -> [u8; 3] {
    let b = (value & 31) as u8;
    let g = (value >> 5 & 63) as u8;
    let r = (value >> 11) as u8;
    [b << 3 | b >> 2, g << 2 | g >> 4, r << 3 | r >> 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_atlas_wraps_tile_edges_and_writes_dds_mips() {
        let path = Path::new("atlas.xml");
        assert_eq!(atlas_dimensions(512, 1, path).unwrap(), (512, 512));
        assert_eq!(atlas_dimensions(512, 3, path).unwrap(), (1024, 512));
        assert_eq!(atlas_dimensions(512, 4, path).unwrap(), (1024, 1024));

        let tile: Vec<Pixel> = (0..TILE_SIZE)
            .flat_map(|y| (0..TILE_SIZE).map(move |x| [x as u8, y as u8, 0, 255]))
            .collect();
        let mut atlas = vec![[0; 4]; STRIDE * STRIDE];
        copy_tile(&mut atlas, STRIDE, 0, 0, &tile);
        assert_eq!(atlas[0], [240, 240, 0, 255]);
        assert_eq!(atlas[BORDER * STRIDE + BORDER], [0, 0, 0, 255]);
        assert_eq!(atlas[STRIDE * STRIDE - 1], [15, 15, 0, 255]);

        let mut dds = Vec::new();
        let pixels = vec![[0, 255, 255, 255]; 64 * 64];
        assert_eq!(write_dds(&mut dds, pixels, 64, 64, false), 11_040);
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(&dds[28..32], &5u32.to_le_bytes());
        assert_eq!(&dds[92..104], &[0, 124, 0, 0, 224, 3, 0, 0, 31, 0, 0, 0]);
        assert!(
            dds[128..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|word| *word == [224, 127])
        );
    }

    #[test]
    fn terrain_normal_mips_trim_edge_taps_and_quantize_after_filtering() {
        let row = [
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 210, 255],
            [0, 0, 0, 255],
        ];
        let pixels = row.repeat(4);
        let mip = triangle_mip(&pixels, 4, 4, 2, normal_word);
        assert_eq!(mip[0], 4 << 10);
        assert_eq!(normal_word([61 * 64 + 32, 0, 0, 0], 64), 7);
    }
}
