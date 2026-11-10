//! Política de seleção: hardware no adapter da captura, outro hardware, CPU.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Nvenc,
    Amf,
    Qsv,
    X264,
    /// Explicit laboratory candidate, never selected automatically.
    Mf,
    /// Explicit CPU candidate (FFmpeg libopenh264, I420); never selected automatically.
    OpenH264,
}
impl BackendKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nvenc => "NVIDIA NVENC",
            Self::Amf => "AMD AMF",
            Self::Qsv => "Intel QuickSync",
            Self::X264 => "CPU (x264)",
            Self::Mf => "CPU candidate (Media Foundation)",
            Self::OpenH264 => "CPU candidate (OpenH264)",
        }
    }
    /// Identificador estável, igual à serialização serde (diagnóstico exportado).
    pub fn id(self) -> &'static str {
        match self {
            Self::Nvenc => "nvenc",
            Self::Amf => "amf",
            Self::Qsv => "qsv",
            Self::X264 => "x264",
            Self::Mf => "mf",
            Self::OpenH264 => "openh264",
        }
    }
}

pub fn candidates(vendor_id: u32, requested: Option<BackendKind>) -> Vec<BackendKind> {
    use BackendKind::*;
    if let Some(backend) = requested {
        return vec![backend];
    }
    match vendor_id {
        0x1002 => vec![Amf, Nvenc, Qsv, X264],
        0x8086 => vec![Qsv, Nvenc, Amf, X264],
        _ => vec![Nvenc, Amf, Qsv, X264],
    }
}

/// Valor da opção `level` do FFmpeg.
/// x264 recebe a forma decimal (`"4.2"`). AMF e QuickSync são inteiros: a constante
/// nomeada `"4.2"` vale 42, mas `av_opt_set` lê a string `"4.2"` como o número 4.
/// O texto certo para esses dois é o `level_idc` (`"42"`).
pub fn ffmpeg_h264_level_value(kind: BackendKind, level_idc: u32) -> String {
    match kind {
        BackendKind::X264 => format!("{}.{}", level_idc / 10, level_idc % 10),
        BackendKind::Amf
        | BackendKind::Qsv
        | BackendKind::Nvenc
        | BackendKind::Mf
        | BackendKind::OpenH264 => level_idc.to_string(),
    }
}

/// Só o sucesso da criação/teste real encerra a busca; a enumeração do driver não basta.
pub fn select<T, E>(
    order: impl IntoIterator<Item = BackendKind>,
    mut create: impl FnMut(BackendKind) -> std::result::Result<T, E>,
) -> std::result::Result<(BackendKind, T), Vec<(BackendKind, E)>> {
    let mut errors = Vec::new();
    for backend in order {
        match create(backend) {
            Ok(encoder) => return Ok((backend, encoder)),
            Err(error) => errors.push((backend, error)),
        }
    }
    Err(errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hardware_failure_continues_to_cpu_and_keeps_diagnostics() {
        let order = candidates(0x1002, None);
        let mut tried = Vec::new();
        let result = select(order, |kind| {
            tried.push(kind);
            if kind == BackendKind::X264 {
                Ok(42)
            } else {
                Err("driver falhou")
            }
        });
        assert_eq!(
            tried,
            [
                BackendKind::Amf,
                BackendKind::Nvenc,
                BackendKind::Qsv,
                BackendKind::X264
            ]
        );
        assert_eq!(result, Ok((BackendKind::X264, 42)));
        assert_eq!(
            select(candidates(0, None), |_| Err::<(), _>("falhou")).map_err(|e| e.len()),
            Err(4)
        );
    }
    #[test]
    fn id_matches_serde_name() {
        use BackendKind::*;
        // serde(rename_all = "lowercase") serializa o nome da variante em minúsculas.
        for kind in [Nvenc, Amf, Qsv, X264, Mf, OpenH264] {
            assert_eq!(kind.id(), format!("{kind:?}").to_lowercase());
        }
    }
    #[test]
    fn explicit_cpu_candidates_never_enter_automatic_selection() {
        for vendor in [0, 0x10de, 0x1002, 0x8086] {
            let order = candidates(vendor, None);
            assert!(!order.contains(&BackendKind::Mf));
            assert!(!order.contains(&BackendKind::OpenH264));
        }
        assert_eq!(
            candidates(0x10de, Some(BackendKind::OpenH264)),
            [BackendKind::OpenH264]
        );
    }

    #[test]
    fn prefers_capture_adapter_and_stops_at_first_working_encoder() {
        for (vendor, expected) in [
            (0x10de, BackendKind::Nvenc),
            (0x1002, BackendKind::Amf),
            (0x8086, BackendKind::Qsv),
        ] {
            let mut attempts = 0;
            let result = select(candidates(vendor, None), |_| {
                attempts += 1;
                Ok::<_, ()>(())
            });
            assert_eq!(result, Ok((expected, ())));
            assert_eq!(attempts, 1);
            assert_eq!(
                candidates(vendor, Some(BackendKind::X264)),
                [BackendKind::X264]
            );
        }
    }

    #[test]
    fn ffmpeg_level_matches_level_idc_on_every_backend() {
        // 1080p30 = 4.0, 1080p60 = 4.2, 1440p60/2160p30 = 5.1, 2160p60 = 5.2, 720p30 = 3.1.
        for (level, decimal) in [
            (31, "3.1"),
            (40, "4.0"),
            (42, "4.2"),
            (50, "5.0"),
            (51, "5.1"),
            (52, "5.2"),
        ] {
            assert_eq!(ffmpeg_h264_level_value(BackendKind::X264, level), decimal);
            for kind in [BackendKind::Amf, BackendKind::Qsv, BackendKind::Nvenc] {
                assert_eq!(ffmpeg_h264_level_value(kind, level), level.to_string());
                assert_ne!(ffmpeg_h264_level_value(kind, level), decimal);
            }
        }
    }
}
