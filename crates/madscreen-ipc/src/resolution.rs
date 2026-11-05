//! Planejamento de resolução e bitrate sem acesso à GPU ou dependências de plataforma.

use serde::{Deserialize, Serialize};

/// Limites de dimensões do encoder, substituídos no engine por `NvEncGetEncodeCaps`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub min_width: u32,
    pub min_height: u32,
}

/// Valores conservadores para H.264 NVENC até a consulta de capacidades em runtime.
impl Default for EncoderLimits {
    fn default() -> Self {
        Self {
            max_width: 4096,
            max_height: 4096,
            min_width: 160,
            min_height: 64,
        }
    }
}

// H.264, Tabela A-1: (level_idc, MaxMBPS, MaxFS), em macroblocos de 16×16.
const H264_LEVELS: [(u32, u32, u32); 8] = [
    (31, 108_000, 3_600),
    (32, 216_000, 5_120),
    (40, 245_760, 8_192),
    (41, 245_760, 8_192),
    (42, 522_240, 8_704),
    (50, 589_824, 22_080),
    (51, 983_040, 36_864),
    (52, 2_073_600, 36_864),
];

/// MaxBR em bps para Baseline/Main nos niveis usados pelo planejador.
/// High usa o fator 1,25 no encoder; nivel desconhecido nao recebe teto inventado.
pub fn h264_max_bitrate_bps(level_idc: u32) -> Option<u32> {
    let kbps = match level_idc {
        31 => 14_000,
        32 | 40 => 20_000,
        41 | 42 => 50_000,
        50 => 135_000,
        51 | 52 => 240_000,
        _ => return None,
    };
    Some(kbps * 1000)
}

/// Menor nível H.264 entre 3.1 e 5.2 que comporta dimensões e taxa de quadros.
/// Dimensões nulas e FPS não positivo ou não finito não são suportados.
pub fn h264_level(width: u32, height: u32, fps: f64) -> Option<u32> {
    if width == 0 || height == 0 || !fps.is_finite() || fps <= 0.0 {
        return None;
    }

    let columns = width.div_ceil(16);
    let rows = height.div_ceil(16);
    let macroblocks = u64::from(columns) * u64::from(rows);
    H264_LEVELS
        .iter()
        .find(|&&(_, max_mbps, max_fs)| {
            let max_dimension = (8 * max_fs).isqrt();
            macroblocks <= u64::from(max_fs)
                && macroblocks as f64 * fps <= f64::from(max_mbps)
                && columns <= max_dimension
                && rows <= max_dimension
        })
        .map(|&(level, _, _)| level)
}

/// Identificador SDP do perfil High, sem flags de restrição (`6400LL`).
/// É o que NVENC, AMF, QuickSync e x264 gravam no SPS com `profile=high`.
pub fn h264_profile_level_id(level_idc: u32) -> String {
    format!("6400{level_idc:02x}")
}

/// Mantém perfil e constraint flags e grava só o `level_idc` nos dois últimos dígitos.
/// `42e01f` no nível 4.0 vira `42e028`, não `640028`: trocar o perfil faz o libwebrtc
/// rejeitar a resposta do SFU.
pub fn h264_with_level(profile_level_id: &str, level_idc: u32) -> Option<String> {
    let bytes = profile_level_id.as_bytes();
    if bytes.len() != 6 || !bytes.iter().all(|byte| byte.is_ascii_hexdigit()) || level_idc > 0xff {
        return None;
    }
    Some(format!(
        "{}{level_idc:02x}",
        profile_level_id[..4].to_ascii_lowercase()
    ))
}

/// `level_idc` dos dois últimos dígitos hexadecimais.
pub fn h264_level_from_profile_level_id(profile_level_id: &str) -> Option<u32> {
    if profile_level_id.len() != 6 || !profile_level_id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(&profile_level_id[4..], 16).ok()
}

/// Escolha de resolução nativa da fonte.
pub const HEIGHT_NATIVE: u32 = 0;
/// Alturas oferecidas antes da opção Nativa.
pub const HEIGHT_CHOICES: [u32; 4] = [720, 1080, 1440, 2160];

/// Dimensões reais da saída e informações para negociação e apresentação na UI.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OutputPlan {
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    pub native: bool,
    pub ultrawide: bool,
    pub reduced: bool,
    pub level: u32,
    pub profile_level_id: String,
    pub notice: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error(
        "a fonte tem apenas {source_height}p; a resolução pedida exigiria ampliar a imagem (sem upscale)"
    )]
    Unavailable { source_height: u32 },
    #[error("a fonte é pequena demais para o encoder")]
    TooSmall,
    #[error("nenhuma resolução com essa proporção cabe nos limites do encoder")]
    Unsupported,
}

// Arredonda diretamente para o par mais próximo, sem arredondamento intermediário.
fn proportional_width(source: (u32, u32), height: u32) -> u32 {
    let (width, source_height) = source;
    let numerator = u64::from(height) * u64::from(width) + u64::from(source_height);
    let even_width = numerator / (2 * u64::from(source_height)) * 2;
    // O par acima de u32::MAX não é representável; preserva o alinhamento NV12.
    even_width.min(u64::from(u32::MAX - 1)) as u32
}

/// Planeja a saída sem upscale, preservando a proporção com arredondamento para NV12.
pub fn plan_output(
    source: (u32, u32),
    height_choice: u32,
    fps: f64,
    limits: EncoderLimits,
) -> Result<OutputPlan, PlanError> {
    let (source_width, source_height) = source;
    if source_width == 0 || source_height == 0 {
        return Err(PlanError::TooSmall);
    }

    let native = height_choice == HEIGHT_NATIVE;
    let (target_width, target_height) = if native {
        (source_width & !1, source_height & !1)
    } else {
        if height_choice > (source_height & !1) {
            return Err(PlanError::Unavailable { source_height });
        }
        (proportional_width(source, height_choice), height_choice)
    };
    if target_width == 0 || target_height == 0 {
        return Err(PlanError::TooSmall);
    }

    let dimension_limited = target_width > limits.max_width || target_height > limits.max_height;
    let (mut width, mut height) = (target_width, target_height);
    let mut level = h264_level(width, height, fps);
    // Cada tentativa diminui a altura em dois: no máximo target_height / 2 tentativas.
    while width > limits.max_width || height > limits.max_height || level.is_none() {
        height = height
            .checked_sub(2)
            .filter(|&height| height > 0)
            .ok_or(PlanError::Unsupported)?;
        width = proportional_width(source, height);
        level = h264_level(width, height, fps);
    }
    if width < limits.min_width || height < limits.min_height {
        return Err(PlanError::TooSmall);
    }

    let level = level.ok_or(PlanError::Unsupported)?;
    let reduced = (width, height) != (target_width, target_height);
    let notice = reduced.then(|| {
        let reason = if dimension_limited {
            if target_width > limits.max_width {
                format!(
                    "o H.264 do NVENC desta GPU limita a largura a {} px",
                    limits.max_width
                )
            } else {
                format!(
                    "o H.264 do NVENC desta GPU limita a altura a {} px",
                    limits.max_height
                )
            }
        } else {
            format!("a resolução pedida excede o nível 5.2 do H.264 a {fps} FPS")
        };
        format!(
            "Reduzido de {target_width}x{target_height} para {width}x{height} mantendo a proporção: {reason}."
        )
    });

    Ok(OutputPlan {
        width,
        height,
        source_width,
        source_height,
        native,
        ultrawide: u64::from(source_width) >= 2 * u64::from(source_height),
        reduced,
        level,
        profile_level_id: h264_profile_level_id(level),
        notice,
    })
}

/// Opção de resolução com dimensões reais e explicação de eventuais restrições.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OutputOption {
    pub choice: u32,
    pub label: String,
    pub available: bool,
    pub width: u32,
    pub height: u32,
    pub reduced: bool,
    pub ultrawide: bool,
    pub note: Option<String>,
}

/// Opções 720p, 1080p, 1440p, 2160p e Nativa, nessa ordem.
pub fn output_options(source: (u32, u32), fps: f64, limits: EncoderLimits) -> Vec<OutputOption> {
    HEIGHT_CHOICES
        .into_iter()
        .chain(std::iter::once(HEIGHT_NATIVE))
        .map(|choice| {
            let label = if choice == HEIGHT_NATIVE {
                "Nativa".to_owned()
            } else {
                format!("{choice}p")
            };
            match plan_output(source, choice, fps, limits) {
                Ok(plan) => {
                    let ultra = if plan.ultrawide { " ultrawide" } else { "" };
                    OutputOption {
                        choice,
                        label: format!("{label}{ultra} {}x{}", plan.width, plan.height),
                        available: true,
                        width: plan.width,
                        height: plan.height,
                        reduced: plan.reduced,
                        ultrawide: plan.ultrawide,
                        note: plan.notice,
                    }
                }
                Err(error) => OutputOption {
                    choice,
                    label,
                    available: false,
                    width: 0,
                    height: 0,
                    reduced: false,
                    ultrawide: u64::from(source.0) >= 2 * u64::from(source.1),
                    note: Some(error.to_string()),
                },
            }
        })
        .collect()
}

/// Referência de bitrate automático: 14 Mbps para 1920×1080 a 60 FPS.
pub const REFERENCE_BITRATE_BPS: u32 = 14_000_000;
/// Piso do bitrate automático: 2 Mbps.
pub const MIN_AUTO_BITRATE_BPS: u32 = 2_000_000;
/// Teto do bitrate automático: 40 Mbps.
pub const MAX_AUTO_BITRATE_BPS: u32 = 40_000_000;

/// Escala a referência por pixels e FPS, arredonda a 100 kbps e aplica piso/teto.
pub fn auto_bitrate_bps(width: u32, height: u32, fps: f64) -> u32 {
    let bitrate = f64::from(REFERENCE_BITRATE_BPS) * f64::from(width) * f64::from(height) * fps
        / (1920.0 * 1080.0 * 60.0);
    let rounded = (bitrate / 100_000.0).round() * 100_000.0;
    (rounded as u32).clamp(MIN_AUTO_BITRATE_BPS, MAX_AUTO_BITRATE_BPS)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn max_bitrate_matches_every_planned_level() {
        for (level, mbps) in [
            (31, 14),
            (32, 20),
            (40, 20),
            (41, 50),
            (42, 50),
            (50, 135),
            (51, 240),
            (52, 240),
        ] {
            assert_eq!(h264_max_bitrate_bps(level), Some(mbps * 1_000_000));
        }
        assert_eq!(h264_max_bitrate_bps(0), None);
        assert_eq!(h264_max_bitrate_bps(53), None);
    }

    fn limits() -> EncoderLimits {
        EncoderLimits::default()
    }

    #[test]
    fn levels_cover_720_through_2160_at_30_and_60() {
        // (largura, altura, fps, level_idc, profile-level-id High)
        let cases = [
            (1280, 720, 30.0, 31, "64001f"),
            (1280, 720, 60.0, 32, "640020"),
            (1920, 1080, 30.0, 40, "640028"),
            (1920, 1080, 60.0, 42, "64002a"),
            (2560, 1080, 30.0, 50, "640032"),
            (2560, 1440, 30.0, 50, "640032"),
            (2560, 1440, 60.0, 51, "640033"),
            (3840, 2160, 30.0, 51, "640033"),
            (3840, 2160, 60.0, 52, "640034"),
            (3440, 1440, 60.0, 52, "640034"),
        ];
        for (width, height, fps, level, profile) in cases {
            assert_eq!(
                h264_level(width, height, fps),
                Some(level),
                "{width}x{height}@{fps}"
            );
            assert_eq!(h264_profile_level_id(level), profile);
            let plan = plan_output((width, height), height, fps, limits()).expect("plano");
            assert_eq!(plan.level, level, "{width}x{height}@{fps}");
            assert_eq!(plan.profile_level_id, profile);
        }
    }

    #[test]
    fn sdp_keeps_offered_profile_and_only_raises_level() {
        assert_eq!(h264_with_level("42e01f", 40).as_deref(), Some("42e028"));
        assert_eq!(h264_with_level("42001f", 42).as_deref(), Some("42002a"));
        assert_eq!(h264_with_level("4d001f", 51).as_deref(), Some("4d0033"));
        assert_eq!(h264_with_level("640c1f", 52).as_deref(), Some("640c34"));
        assert_eq!(h264_with_level("42E01F", 31).as_deref(), Some("42e01f"));
        assert_ne!(h264_with_level("42e01f", 40).as_deref(), Some("640028"));
        assert_eq!(h264_with_level("42e01", 40), None);
        assert_eq!(h264_with_level("42e01g", 40), None);
        assert_eq!(h264_level_from_profile_level_id("640028"), Some(40));
        assert_eq!(h264_level_from_profile_level_id("640c34"), Some(52));
        assert_eq!(h264_level_from_profile_level_id("42e01f"), Some(31));
        assert_eq!(h264_level_from_profile_level_id("64002"), None);
    }

    #[test]
    fn level_31_is_only_enough_for_720p30() {
        assert_eq!(h264_level(1280, 720, 30.0), Some(31));
        assert!(h264_level(1920, 1080, 30.0).is_some_and(|level| level > 31));
        assert!(h264_level(1920, 1080, 60.0).is_some_and(|level| level > 31));
        assert_eq!(h264_level(0, 720, 30.0), None);
        assert_eq!(h264_level(1280, 720, 0.0), None);
    }
}
