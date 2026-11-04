//! Conversão NV12 → I420 em buffers do chamador, sem alocação por frame.

use crate::EncodeError;

/// Planos independentes; os strides podem incluir padding entre linhas.
///
/// A última linha de cada slice precisa conter somente os bytes ativos.
/// Nenhum byte de padding é alterado.
pub struct I420Planes<'a> {
    pub y: &'a mut [u8],
    pub y_stride: usize,
    pub u: &'a mut [u8],
    pub u_stride: usize,
    pub v: &'a mut [u8],
    pub v_stride: usize,
}

fn plane_required(stride: usize, rows: usize, active: usize) -> crate::Result<usize> {
    // The caller validates rows >= 1 before calculating any plane footprint.
    stride
        .checked_mul(rows - 1)
        .and_then(|prefix| prefix.checked_add(active))
        .ok_or(EncodeError::InvalidConfig(
            "overflow no tamanho de plano I420",
        ))
}

fn require_capacity(capacity: usize, required: usize) -> crate::Result<()> {
    if capacity < required {
        return Err(EncodeError::BufferTooSmall { required, capacity });
    }
    Ok(())
}

/// Separa NV12 (Y e UV intercalado) em I420 (Y, U, V).
///
/// A origem ocupa row_pitch * height bytes de Y, seguidos por
/// row_pitch * (height / 2) bytes de UV, incluindo o padding da última linha.
/// Dimensões, strides, aritmética e todas as capacidades são validados antes
/// de qualquer escrita nos três destinos.
pub fn nv12_to_i420(
    src: &[u8],
    row_pitch: usize,
    width: usize,
    height: usize,
    dst: &mut I420Planes<'_>,
) -> crate::Result<()> {
    if width < 2 || height < 2 || width % 2 != 0 || height % 2 != 0 {
        return Err(EncodeError::InvalidConfig(
            "dimensoes NV12/I420 devem ser pares e >= 2",
        ));
    }
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    if row_pitch < width
        || dst.y_stride < width
        || dst.u_stride < chroma_width
        || dst.v_stride < chroma_width
    {
        return Err(EncodeError::InvalidConfig(
            "stride menor que a largura ativa NV12/I420",
        ));
    }

    let y_span = row_pitch
        .checked_mul(height)
        .ok_or(EncodeError::InvalidConfig(
            "overflow no tamanho da origem NV12",
        ))?;
    let uv_span = row_pitch
        .checked_mul(chroma_height)
        .ok_or(EncodeError::InvalidConfig(
            "overflow no tamanho da origem NV12",
        ))?;
    let src_required = y_span
        .checked_add(uv_span)
        .ok_or(EncodeError::InvalidConfig(
            "overflow no tamanho da origem NV12",
        ))?;
    let y_required = plane_required(dst.y_stride, height, width)?;
    let u_required = plane_required(dst.u_stride, chroma_height, chroma_width)?;
    let v_required = plane_required(dst.v_stride, chroma_height, chroma_width)?;

    require_capacity(src.len(), src_required)?;
    require_capacity(dst.y.len(), y_required)?;
    require_capacity(dst.u.len(), u_required)?;
    require_capacity(dst.v.len(), v_required)?;

    // All row offsets and active ranges below are bounded by the checked
    // footprints above; no validation or fallible operation follows a write.
    for row in 0..height {
        let source = row * row_pitch;
        let target = row * dst.y_stride;
        dst.y[target..target + width].copy_from_slice(&src[source..source + width]);
    }
    for row in 0..chroma_height {
        let source = y_span + row * row_pitch;
        let u = row * dst.u_stride;
        let v = row * dst.v_stride;
        for column in 0..chroma_width {
            dst.u[u + column] = src[source + 2 * column];
            dst.v[v + column] = src[source + 2 * column + 1];
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{I420Planes, nv12_to_i420};
    use crate::EncodeError;

    // Four distinct Y rows, two distinct UV rows, and unrelated source padding.
    const SOURCE: [u8; 36] = [
        1, 2, 3, 4, 201, 202, 11, 12, 13, 14, 203, 204, 21, 22, 23, 24, 205, 206, 31, 32, 33, 34,
        207, 208, 71, 171, 72, 172, 209, 210, 81, 181, 82, 182, 211, 212,
    ];
    const SENTINEL: u8 = 0xa5;

    #[test]
    fn separates_chroma_and_preserves_every_destination_padding_byte() {
        let mut y = [SENTINEL; 28];
        let mut u = [SENTINEL; 10];
        let mut v = [SENTINEL; 8];
        let result = nv12_to_i420(
            &SOURCE,
            6,
            4,
            4,
            &mut I420Planes {
                y: &mut y,
                y_stride: 7,
                u: &mut u,
                u_stride: 5,
                v: &mut v,
                v_stride: 4,
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            y,
            [
                1, 2, 3, 4, SENTINEL, SENTINEL, SENTINEL, 11, 12, 13, 14, SENTINEL, SENTINEL,
                SENTINEL, 21, 22, 23, 24, SENTINEL, SENTINEL, SENTINEL, 31, 32, 33, 34, SENTINEL,
                SENTINEL, SENTINEL,
            ]
        );
        assert_eq!(
            u,
            [
                71, 72, SENTINEL, SENTINEL, SENTINEL, 81, 82, SENTINEL, SENTINEL, SENTINEL
            ]
        );
        assert_eq!(
            v,
            [171, 172, SENTINEL, SENTINEL, 181, 182, SENTINEL, SENTINEL]
        );
    }

    #[test]
    fn accepts_minimal_last_rows_and_a_single_chroma_row_with_large_strides() {
        // Odd source pitch is valid: UV bytes are pairs, row padding is arbitrary.
        let source = [1, 2, 90, 3, 4, 91, 10, 20, 92];
        let mut y = [SENTINEL; 7];
        let mut u = [SENTINEL; 1];
        let mut v = [SENTINEL; 1];
        assert!(
            nv12_to_i420(
                &source,
                3,
                2,
                2,
                &mut I420Planes {
                    y: &mut y,
                    y_stride: 5,
                    u: &mut u,
                    u_stride: usize::MAX,
                    v: &mut v,
                    v_stride: usize::MAX,
                },
            )
            .is_ok()
        );
        assert_eq!(y, [1, 2, SENTINEL, SENTINEL, SENTINEL, 3, 4]);
        assert_eq!(u, [10]);
        assert_eq!(v, [20]);
    }

    fn rejected_unchanged(
        src: &[u8],
        source_pitch: usize,
        dimensions: (usize, usize),
        pitches: (usize, usize, usize),
        capacities: (usize, usize, usize),
    ) -> EncodeError {
        let mut y = vec![SENTINEL; capacities.0];
        let mut u = vec![SENTINEL; capacities.1];
        let mut v = vec![SENTINEL; capacities.2];
        let result = nv12_to_i420(
            src,
            source_pitch,
            dimensions.0,
            dimensions.1,
            &mut I420Planes {
                y: &mut y,
                y_stride: pitches.0,
                u: &mut u,
                u_stride: pitches.1,
                v: &mut v,
                v_stride: pitches.2,
            },
        );
        assert!(y.iter().all(|&byte| byte == SENTINEL));
        assert!(u.iter().all(|&byte| byte == SENTINEL));
        assert!(v.iter().all(|&byte| byte == SENTINEL));
        match result {
            Err(error) => error,
            Ok(()) => panic!("invalid input was accepted"),
        }
    }

    #[test]
    fn rejects_truncated_source_and_each_plane_before_touching_any_plane() {
        let error = rejected_unchanged(&SOURCE[..35], 6, (4, 4), (7, 5, 4), (25, 7, 6));
        assert!(matches!(
            error,
            EncodeError::BufferTooSmall {
                required: 36,
                capacity: 35
            }
        ));
        for (capacities, required, capacity) in
            [((24, 7, 6), 25, 24), ((25, 6, 6), 7, 6), ((25, 7, 5), 6, 5)]
        {
            let error = rejected_unchanged(&SOURCE, 6, (4, 4), (7, 5, 4), capacities);
            assert!(
                matches!(error, EncodeError::BufferTooSmall { required: r, capacity: c } if r == required && c == capacity)
            );
        }
    }

    #[test]
    fn requires_full_source_pitch_including_final_uv_padding() {
        let source = [1, 2, 90, 3, 4, 91, 10, 20];
        let error = rejected_unchanged(&source, 3, (2, 2), (2, 1, 1), (4, 1, 1));
        assert!(matches!(
            error,
            EncodeError::BufferTooSmall {
                required: 9,
                capacity: 8
            }
        ));
    }

    #[test]
    fn rejects_odd_or_small_dimensions_and_insufficient_strides_without_writes() {
        for dimensions in [(3, 4), (4, 3), (0, 4), (4, 0), (1, 2), (2, 1)] {
            let error = rejected_unchanged(&SOURCE, 6, dimensions, (7, 5, 4), (28, 10, 8));
            assert!(matches!(error, EncodeError::InvalidConfig(_)));
        }
        for (source_pitch, pitches) in [
            (3, (7, 5, 4)),
            (6, (3, 5, 4)),
            (6, (7, 1, 4)),
            (6, (7, 5, 1)),
        ] {
            let error = rejected_unchanged(&SOURCE, source_pitch, (4, 4), pitches, (28, 10, 8));
            assert!(matches!(error, EncodeError::InvalidConfig(_)));
        }
    }

    #[test]
    fn rejects_source_and_destination_overflow_before_any_write() {
        // Source multiplication overflow, then addition overflow with valid Y span.
        for pitch in [usize::MAX, usize::MAX / 4] {
            let error = rejected_unchanged(&SOURCE, pitch, (2, 4), (2, 1, 1), (8, 2, 2));
            assert!(matches!(error, EncodeError::InvalidConfig(_)));
        }
        for pitches in [(usize::MAX, 5, 4), (7, usize::MAX, 4), (7, 5, usize::MAX)] {
            let error = rejected_unchanged(&SOURCE, 6, (4, 4), pitches, (28, 10, 8));
            assert!(matches!(error, EncodeError::InvalidConfig(_)));
        }
    }
}
