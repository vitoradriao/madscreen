//! Escolha pura do filtro; reavaliada quando a dimensão da fonte muda.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReductionPreference {
    #[default]
    Automatic,
    Bilinear,
    Box4x4,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReductionFilter {
    #[default]
    Bilinear,
    Box4x4,
    VideoProcessor,
}
impl ReductionFilter {
    pub fn name(self) -> &'static str {
        match self {
            Self::Bilinear => "bilinear",
            Self::Box4x4 => "box4x4",
            Self::VideoProcessor => "video_processor",
        }
    }
}

// Plano box7: aguardar A/B com SSIM maior e aumento p95 da submissão <=0,5ms.
// Sem memória para medir em 6/nov; não promover no caminho do produto ainda.
const DEFAULT_BOX_REDUCTION_QUALIFIED: bool = false;

/// Um opt-in de A/B não pode medir VP com rótulo box. Só Automatic aceita fallback.
pub fn validate_preference(
    preference: ReductionPreference,
    shader_available: bool,
) -> crate::Result<()> {
    if preference == ReductionPreference::Box4x4 && !shader_available {
        return Err(crate::EncodeError::InvalidConfig(
            "filtro box indisponivel no VideoProcessor",
        ));
    }
    Ok(())
}

pub fn reduction_filter(
    source: (u32, u32),
    output: (u32, u32),
    preference: ReductionPreference,
    shader_available: bool,
) -> ReductionFilter {
    select_filter(
        source,
        output,
        preference,
        shader_available,
        DEFAULT_BOX_REDUCTION_QUALIFIED,
    )
}

fn select_filter(
    source: (u32, u32),
    output: (u32, u32),
    preference: ReductionPreference,
    shader_available: bool,
    qualified: bool,
) -> ReductionFilter {
    if !shader_available {
        return ReductionFilter::VideoProcessor;
    }
    let fitted = madscreen_capture::fit_rect(source, output);
    let reducing = fitted.width > 0
        && fitted.height > 0
        && fitted.width <= source.0
        && fitted.height <= source.1
        && (fitted.width < source.0 || fitted.height < source.1);
    let requested = match preference {
        ReductionPreference::Automatic => qualified,
        ReductionPreference::Bilinear => false,
        ReductionPreference::Box4x4 => true,
    };
    if reducing && requested {
        ReductionFilter::Box4x4
    } else {
        ReductionFilter::Bilinear
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_box_without_shader_fails_but_automatic_keeps_fallback() -> crate::Result<()> {
        assert!(matches!(
            validate_preference(ReductionPreference::Box4x4, false),
            Err(crate::EncodeError::InvalidConfig(_))
        ));
        validate_preference(ReductionPreference::Box4x4, true)?;
        validate_preference(ReductionPreference::Bilinear, false)?;
        validate_preference(ReductionPreference::Automatic, false)?;
        assert_eq!(
            select_filter(
                (1920, 1080),
                (1280, 720),
                ReductionPreference::Automatic,
                false,
                true
            ),
            ReductionFilter::VideoProcessor
        );
        Ok(())
    }
    #[test]
    fn native_and_upscale_never_change_even_with_explicit_box() {
        for preference in [
            ReductionPreference::Automatic,
            ReductionPreference::Bilinear,
            ReductionPreference::Box4x4,
        ] {
            for output in [(1920, 1080), (3840, 2160)] {
                assert_eq!(
                    select_filter((1920, 1080), output, preference, true, true),
                    ReductionFilter::Bilinear
                );
            }
        }
    }
    #[test]
    fn reduction_obeys_override_and_qualification_including_letterbox() {
        for (source, output) in [((1920, 1080), (1280, 720)), ((1280, 1080), (1920, 720))] {
            assert_eq!(
                select_filter(source, output, ReductionPreference::Automatic, true, true),
                ReductionFilter::Box4x4
            );
            assert_eq!(
                reduction_filter(source, output, ReductionPreference::Automatic, true),
                ReductionFilter::Bilinear
            );
            assert_eq!(
                reduction_filter(source, output, ReductionPreference::Box4x4, true),
                ReductionFilter::Box4x4
            );
            assert_eq!(
                select_filter(source, output, ReductionPreference::Bilinear, true, true),
                ReductionFilter::Bilinear
            );
            assert_eq!(
                select_filter(source, output, ReductionPreference::Box4x4, false, true),
                ReductionFilter::VideoProcessor
            );
        }
    }
    #[test]
    fn resize_recalculates_without_latching_box_or_changing_native() {
        let sizes = [
            (1920, 1080),
            (1280, 720),
            (640, 360),
            (2560, 1440),
            (1280, 720),
        ];
        let filters: Vec<_> = sizes
            .into_iter()
            .map(|source| reduction_filter(source, (1280, 720), ReductionPreference::Box4x4, true))
            .collect();
        assert_eq!(
            filters,
            [
                ReductionFilter::Box4x4,
                ReductionFilter::Bilinear,
                ReductionFilter::Bilinear,
                ReductionFilter::Box4x4,
                ReductionFilter::Bilinear
            ]
        );
    }
    #[test]
    fn invalid_sizes_do_not_enable_box() {
        for (source, output) in [
            ((0, 1080), (1280, 720)),
            ((1920, 0), (1280, 720)),
            ((1920, 1080), (0, 720)),
        ] {
            assert_eq!(
                reduction_filter(source, output, ReductionPreference::Box4x4, true),
                ReductionFilter::Bilinear
            );
        }
    }
}
