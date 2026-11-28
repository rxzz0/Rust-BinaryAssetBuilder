use super::{Wave, block_header};

const FRAME_SAMPLES: usize = 128;
const CHANNEL_FRAME_BYTES: usize = 76;

pub(super) fn encode(wave: &Wave<'_>, streamed: bool, header: &[u8]) -> Vec<u8> {
    let channel_order: &[usize] = match wave.channels {
        1 => &[0],
        2 => &[0, 1],
        4 => &[0, 1, 3, 2],
        6 => &[0, 2, 1, 4, 5, 3],
        _ => unreachable!("WAV channel count was validated"),
    };
    let channels = channel_order.len();
    let frames = wave.frames() as usize;
    let packet_bytes = channels * CHANNEL_FRAME_BYTES;
    let packet_count = frames.div_ceil(FRAME_SAMPLES);
    // The stream encoder batches input samples and carries incomplete XAS packets
    // into the next batch.
    let batch_frames = 2040 * FRAME_SAMPLES / packet_bytes;
    let mut out = Vec::new();
    if !streamed {
        out.extend_from_slice(header);
    }
    let mut start = 0;
    let mut submitted = 0;
    while start < packet_count {
        submitted += batch_frames;
        let last = !streamed || submitted >= frames;
        let end_packet = if last {
            packet_count
        } else {
            submitted / FRAME_SAMPLES
        };
        let packets = end_packet - start;
        let count = (frames - start * FRAME_SAMPLES).min(packets * FRAME_SAMPLES);
        // The final stream block has room for the terminator before alignment.
        let payload = packets * packet_bytes;
        let padding = if streamed && last && (out.len() + 8 + payload).is_multiple_of(64) {
            64
        } else {
            0
        };
        let end = block_header(&mut out, payload + padding, count, streamed && last);
        for packet in start..end_packet {
            for &channel in channel_order {
                let samples = std::array::from_fn(|i| {
                    let sample_frame = (packet * FRAME_SAMPLES + i).min(frames - 1);
                    wave.sample(sample_frame * channels + channel)
                });
                out.extend(encode_channel(&samples));
            }
        }
        out.resize(end, 0);
        start = end_packet;
    }
    out
}

fn encode_channel(samples: &[i16; FRAME_SAMPLES]) -> [u8; CHANNEL_FRAME_BYTES] {
    let mut out = [0; CHANNEL_FRAME_BYTES];
    for (lane, input) in samples.as_chunks::<32>().0.iter().enumerate() {
        let pcm = input.map(|sample| f32::from(sample) * (32_767.0 / 32_768.0));
        let seeds = [pcm[0], pcm[1]].map(|sample| (((sample + 8.0) as i32) & !15).min(32_752));
        let (predictor, shift, codes) = encode_lane(&pcm, seeds);
        let first = seeds[0] as i16 | predictor as i16;
        let second = seeds[1] as i16 | shift as i16;
        out[lane * 4..lane * 4 + 2].copy_from_slice(&first.to_le_bytes());
        out[lane * 4 + 2..lane * 4 + 4].copy_from_slice(&second.to_le_bytes());
        for (pair, &[high, low]) in codes.as_chunks::<2>().0.iter().enumerate() {
            out[16 + pair * 4 + lane] = high << 4 | low;
        }
    }
    out
}

fn encode_lane(input: &[f32; 32], seeds: [i32; 2]) -> (usize, u32, [u8; 30]) {
    const PREDICTORS: [(f32, f32); 4] = [
        (0.0, 0.0),
        (240.0 / 256.0, 0.0),
        (460.0 / 256.0, -208.0 / 256.0),
        (392.0 / 256.0, -220.0 / 256.0),
    ];
    let mut best_peak = f32::INFINITY;
    let mut predictor = 0;
    for (index, &(recent, older)) in PREDICTORS.iter().enumerate() {
        let [mut previous, mut current] = seeds;
        let mut peak = 0.0_f32;
        for &sample in &input[2..] {
            let prediction = recent * current as f32 + older * previous as f32;
            peak = peak.max((sample - prediction).abs());
            previous = current;
            current = sample as i32;
        }
        if peak < best_peak {
            best_peak = peak;
            predictor = index;
        }
        if index == 0 && peak <= 7.0 {
            break;
        }
    }
    let peak = (best_peak as i32).min(32_767);
    let shift = (0..12)
        .find(|&shift| (peak + (2048 >> shift)) & (16_384 >> shift) != 0)
        .unwrap_or(12);
    let (recent, older) = PREDICTORS[predictor];
    let [mut previous, mut current] = seeds.map(|sample| sample as f32);
    let mut codes = [0; 30];
    for (&sample, code) in input[2..].iter().zip(&mut codes) {
        let prediction = recent * current + older * previous;
        let residual = ((sample - prediction) * (1 << shift) as f32) as i32;
        let quantized = ((residual + 2048) & !4095).clamp(-32_768, 32_767);
        *code = ((quantized >> 12) & 15) as u8;
        previous = current;
        current = prediction + (quantized >> shift) as f32;
    }
    (predictor, shift, codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_exact_constant_samples() {
        let frame = encode_channel(&[2048; FRAME_SAMPLES]);
        // Quantization may choose another exact predictor/shift pair. Verify
        // the frame against the XAS decoding equation for each lane.
        for lane in 0..4 {
            let first = i16::from_le_bytes(frame[lane * 4..lane * 4 + 2].try_into().unwrap());
            let second = i16::from_le_bytes(frame[lane * 4 + 2..lane * 4 + 4].try_into().unwrap());
            let (a, b) = [(0, 0), (240, 0), (460, -208), (392, -220)][(first & 15) as usize];
            let mut previous = i32::from(first & !15);
            let mut current = i32::from(second & !15);
            assert_eq!([previous, current], [2048, 2048]);
            for pair in 0..15 {
                let byte = frame[16 + pair * 4 + lane];
                for nibble in [byte >> 4, byte & 15] {
                    let residual = i32::from((nibble as i8) << 4 >> 4);
                    let next = ((a * current + b * previous + 128) >> 8)
                        + (residual << (12 - (second & 15)));
                    assert_eq!(next, 2048);
                    previous = current;
                    current = next;
                }
            }
        }
    }
}
