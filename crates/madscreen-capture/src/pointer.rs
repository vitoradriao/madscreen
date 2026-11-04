//! Validação e conversão dos pequenos bitmaps de cursor DXGI, sem dependência de Windows.
pub(crate) fn decode_shape(
    kind: u32,
    width: u32,
    height: u32,
    pitch: u32,
    bytes: &[u8],
) -> Option<(Vec<u8>, (u32, u32))> {
    if width == 0 || height == 0 || width > 4096 || height > 8192 || ![1, 2, 4].contains(&kind) {
        return None;
    }
    let monochrome = kind == 1;
    if monochrome && height % 2 != 0 {
        return None;
    }
    let row_bytes = if monochrome {
        width.div_ceil(8)
    } else {
        width.checked_mul(4)?
    };
    if pitch < row_bytes || bytes.len() < (pitch as usize).checked_mul(height as usize)? {
        return None;
    }
    let output_height = if monochrome { height / 2 } else { height };
    let mut output = vec![
        0;
        (width as usize)
            .checked_mul(output_height as usize)?
            .checked_mul(4)?
    ];
    for y in 0..output_height as usize {
        for x in 0..width as usize {
            let dst = (y * width as usize + x) * 4;
            if monochrome {
                let bit = 0x80 >> (x % 8);
                let and = if bytes[y * pitch as usize + x / 8] & bit != 0 {
                    255
                } else {
                    0
                };
                let xor = if bytes[(y + output_height as usize) * pitch as usize + x / 8] & bit != 0
                {
                    255
                } else {
                    0
                };
                output[dst..dst + 4].copy_from_slice(&[0, xor, and, 255]);
            } else {
                let src = y * pitch as usize + x * 4;
                output[dst..dst + 4].copy_from_slice(&bytes[src..src + 4]);
            }
        }
    }
    Some((output, (width, output_height)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monochrome_encodes_black_white_transparent_and_inverted_pixels()
    -> Result<(), Box<dyn std::error::Error>> {
        // AND/XOR: 00 black, 01 white, 10 unchanged, 11 inverted.
        let (pixels, size) =
            decode_shape(1, 4, 2, 1, &[0b00110000, 0b01010000]).ok_or("forma invalida")?;
        assert_eq!(size, (4, 1));
        assert_eq!(
            pixels,
            [
                0, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 255, 255, 255
            ]
        );
        Ok(())
    }
    #[test]
    fn color_and_masked_shapes_strip_pitch_padding() -> Result<(), Box<dyn std::error::Error>> {
        for kind in [2, 4] {
            let (pixels, size) = decode_shape(
                kind,
                1,
                2,
                8,
                &[1, 2, 3, 0, 99, 99, 99, 99, 4, 5, 6, 255, 88, 88, 88, 88],
            )
            .ok_or("forma invalida")?;
            assert_eq!(size, (1, 2));
            assert_eq!(pixels, [1, 2, 3, 0, 4, 5, 6, 255]);
        }
        Ok(())
    }
    #[test]
    fn invalid_metadata_is_rejected_before_indexing_or_allocating() {
        for (kind, width, height, pitch) in [
            (0, 1, 1, 4),
            (2, 0, 1, 4),
            (1, 8, 3, 1),
            (1, 9, 2, 1),
            (2, 2, 1, 4),
            (2, u32::MAX, 1, 4),
            (4, 1, u32::MAX, 4),
            (2, 1, 1, 4),
        ] {
            assert!(decode_shape(kind, width, height, pitch, &[0; 3]).is_none());
        }
    }
}
