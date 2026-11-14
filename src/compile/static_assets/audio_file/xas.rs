use super::{Wave, block_header};

const FRAME_SAMPLES: usize = 128;
const CHANNEL_FRAME_BYTES: usize = 76;

pub(super) fn encode(wave: &Wave<'_>, streamed: bool, header: &[u8]) -> Vec<u8> {
    let channels = usize::from(wave.channels);
    let frames = wave.frames() as usize;
    let packet_bytes = channels * CHANNEL_FRAME_BYTES;
    let packet_count = frames.div_ceil(FRAME_SAMPLES);
    let block_packets = if streamed {
        (2048 - 8) / packet_bytes
    } else {
        packet_count
    };
    let mut out = Vec::new();
    if !streamed {
        out.extend_from_slice(header);
    }
    for start in (0..packet_count).step_by(block_packets) {
        let packets = (packet_count - start).min(block_packets);
        let count = (frames - start * FRAME_SAMPLES).min(packets * FRAME_SAMPLES);
        let end = block_header(
            &mut out,
            packets * packet_bytes,
            count,
            streamed && start + packets == packet_count,
        );
        for packet in start..start + packets {
            for channel in 0..channels {
                let samples = std::array::from_fn(|i| {
                    wave.sample((packet * FRAME_SAMPLES + i) * channels + channel)
                });
                out.extend(encode_channel(&samples));
            }
        }
        out.resize(end, 0);
    }
    out
}

fn encode_channel(samples: &[i16; FRAME_SAMPLES]) -> [u8; CHANNEL_FRAME_BYTES] {
    let mut out = [0; CHANNEL_FRAME_BYTES];
    for (lane, input) in samples.as_chunks::<32>().0.iter().enumerate() {
        let (predictor, shift, codes) = encode_lane(input);
        let first = (input[0] & !15) | predictor as i16;
        let second = (input[1] & !15) | shift as i16;
        out[lane * 4..lane * 4 + 2].copy_from_slice(&first.to_le_bytes());
        out[lane * 4 + 2..lane * 4 + 4].copy_from_slice(&second.to_le_bytes());
        for (pair, &[high, low]) in codes.as_chunks::<2>().0.iter().enumerate() {
            out[16 + pair * 4 + lane] = high << 4 | low;
        }
    }
    out
}

fn encode_lane(input: &[i16; 32]) -> (usize, u32, [u8; 30]) {
    // The four XAS predictors use 8 fractional bits. The shift controls the 4-bit residual scale.
    const PREDICTORS: [(i32, i32); 4] = [(0, 0), (240, 0), (460, -208), (392, -220)];
    let mut best = (u64::MAX, 0, 0, [0; 30]);
    for (predictor, &(a, b)) in PREDICTORS.iter().enumerate() {
        for shift in 0..=12 {
            let step = 1 << (12 - shift);
            let mut previous = i32::from(input[0] & !15);
            let mut current = i32::from(input[1] & !15);
            let mut error = 0;
            let mut codes = [0; 30];
            for (&sample, code) in input[2..].iter().zip(&mut codes) {
                let prediction = (a * current + b * previous + 128) >> 8;
                let residual = i32::from(sample) - prediction;
                let quantized = ((residual + step / 2) >> (12 - shift)).clamp(-8, 7);
                previous = current;
                current = (prediction + quantized * step).clamp(i16::MIN.into(), i16::MAX.into());
                let difference = i64::from(i32::from(sample) - current);
                error += (difference * difference) as u64;
                *code = (quantized & 15) as u8;
                if error >= best.0 {
                    break;
                }
            }
            if error < best.0 {
                best = (error, predictor, shift, codes);
            }
        }
    }
    (best.1, best.2, best.3)
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
