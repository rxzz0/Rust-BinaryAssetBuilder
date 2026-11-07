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
