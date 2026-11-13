//! Opções FFmpeg sem FFI; usadas só ao abrir/reabrir o encoder.
#![cfg_attr(not(windows), allow(dead_code))]
use crate::{EncoderConfig, FrameRate, backend::BackendKind, config::H264Profile};

// Owner's 6 Nov decision enables this compatibility default before A/B;
// quality and CPU benefits remain unmeasured.
const X264_SCREEN_PRESET_QUALIFIED: bool = true;
const X264_SCREEN_MAX_FPS: u64 = 30;

fn x264_preset(fps: FrameRate, qualified: bool) -> &'static str {
    if qualified
        && fps.den != 0
        && fps.num != 0
        && u64::from(fps.num) <= X264_SCREEN_MAX_FPS * u64::from(fps.den)
    {
        "superfast"
    } else {
        "ultrafast"
    }
}

pub(crate) fn backend_options(
    kind: BackendKind,
    config: &EncoderConfig,
) -> Vec<(&'static str, String)> {
    let options = match kind {
        BackendKind::Amf => vec![
            ("usage", "ultralowlatency"),
            ("quality", "balanced"),
            ("rc", "cbr"),
            ("preanalysis", "0"),
            // FFmpeg 8.1.2 amfenc.c: sem LOW_DELAY retém a saída mesmo sem B-frames.
            ("flags", "+low_delay"),
            // AV_PICTURE_TYPE_I sozinho não pede IDR nem reinsere SPS/PPS;
            // necessário no probe e na recuperação solicitada pelo viewer.
            ("forced_idr", "1"),
            ("async_depth", "1"),
        ],
        BackendKind::Qsv => {
            let mut options = vec![(
                "preset",
                if config.compat_tuning {
                    "medium"
                } else {
                    "veryfast"
                },
            )];
            if config.compat_tuning {
                // FFmpeg 8.1.2 h264_qsv: displayremoting=1; MF uses display_remoting.
                options.push(("scenario", "displayremoting"));
            }
            options.extend([
                ("async_depth", "1"),
                ("look_ahead", "0"),
                ("forced_idr", "1"),
            ]);
            options
        }
        BackendKind::Mf => vec![
            ("hw_encoding", "false"),
            ("rate_control", "ld_vbr"),
            ("scenario", "display_remoting"),
            // mfenc.c só liga CODECAPI_AVLowLatencyMode com LOW_DELAY;
            // o nome do rate control sozinho não configura essa propriedade MFT.
            ("flags", "+low_delay"),
        ],
        // libopenh264enc.c (760d1060): rc_mode padrão é quality; frame skip
        // devolveria pacote vazio. O probe medido usava uma fatia por thread;
        // com fatia única OpenH264 ignora iMultipleThreadIdc (threads=2).
        // O wrapper força BackgroundDetection/AdaptiveQuant=1, mas conserva
        // iComplexityMode/SceneChangeDetect nos padrões do SDK, sem opção exposta.
        BackendKind::OpenH264 => vec![
            ("rc_mode", "bitrate"),
            ("allow_skip_frames", "0"),
            ("slices", "2"),
        ],
        BackendKind::X264 => vec![
            (
                "preset",
                x264_preset(
                    config.fps,
                    X264_SCREEN_PRESET_QUALIFIED
                        && config.compat_tuning
                        && config.x264_screen_preset_override.unwrap_or(true),
                ),
            ),
            ("tune", "zerolatency"),
            ("forced-idr", "1"),
            (
                // ultrafast desliga o deblocking (blocos visíveis); 0,0 o religa com o padrão do x264.
                "x264-params",
                match config.h264_profile {
                    H264Profile::ConstrainedBaseline => {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=0:8x8dct=0:deblock=0,0"
                    }
                    H264Profile::Main => {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=1:8x8dct=0:deblock=0,0"
                    }
                    H264Profile::High => {
                        "repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac=1:8x8dct=1:deblock=0,0"
                    }
                },
            ),
        ],
        BackendKind::Nvenc => vec![],
    };
    options
        .into_iter()
        .map(|(key, value)| (key, value.to_owned()))
        .collect()
}

/// Só a dica QSV é opcional. Retorna sua rejeição para o chamador logar uma vez;
/// as opções obrigatórias continuam propagando erro para a seleção de fallback.
pub(crate) fn apply_backend_options<E>(
    kind: BackendKind,
    config: &EncoderConfig,
    mut set: impl FnMut(&str, &str) -> std::result::Result<(), E>,
) -> std::result::Result<Option<E>, E> {
    let mut rejected_hint = None;
    for (key, value) in backend_options(kind, config) {
        if let Err(error) = set(key, &value) {
            if kind == BackendKind::Qsv && key == "scenario" {
                rejected_hint = Some(error);
            } else {
                return Err(error);
            }
        }
    }
    Ok(rejected_hint)
}

/// Só uma falha de abertura com a dica aplicada habilita a tentativa compatível.
/// Falhas de configuração/alocação ou erros sem dica não são repetidos.
pub(crate) struct OpenFailure<E> {
    pub error: E,
    pub hint_applied: bool,
}
impl<E> From<E> for OpenFailure<E> {
    fn from(error: E) -> Self {
        Self {
            error,
            hint_applied: false,
        }
    }
}

pub(crate) fn open_backend<T, E>(
    kind: BackendKind,
    mut attempt: impl FnMut(bool) -> std::result::Result<T, OpenFailure<E>>,
    mut on_retry: impl FnMut(&E),
) -> std::result::Result<T, E> {
    match attempt(kind == BackendKind::Qsv) {
        Err(failure) if kind == BackendKind::Qsv && failure.hint_applied => {
            on_retry(&failure.error);
            attempt(false).map_err(|failure| failure.error)
        }
        result => result.map_err(|failure| failure.error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn return_switch_restores_qsv_and_preserves_amf_nvenc_options() {
        let enabled = EncoderConfig::default();
        let disabled = EncoderConfig {
            compat_tuning: false,
            ..enabled.clone()
        };
        assert!(
            backend_options(BackendKind::Qsv, &enabled)
                .contains(&("scenario", "displayremoting".into()))
        );
        let qsv_off = backend_options(BackendKind::Qsv, &disabled);
        assert!(qsv_off.contains(&("preset", "veryfast".into())));
        assert!(!qsv_off.iter().any(|(key, _)| *key == "scenario"));
        assert_eq!(
            backend_options(BackendKind::Amf, &enabled),
            backend_options(BackendKind::Amf, &disabled)
        );
        assert_eq!(
            backend_options(BackendKind::Nvenc, &enabled),
            backend_options(BackendKind::Nvenc, &disabled)
        );
    }

    #[test]
    fn qsv_open_retries_once_without_hint_and_preserves_other_options() {
        let config = EncoderConfig::default();
        let mut attempts = Vec::new();
        let mut warnings = 0;
        let result = open_backend(
            BackendKind::Qsv,
            |hint| {
                let options: Vec<_> = backend_options(BackendKind::Qsv, &config)
                    .into_iter()
                    .filter(|(key, _)| hint || *key != "scenario")
                    .collect();
                attempts.push(options);
                if hint {
                    Err(OpenFailure {
                        error: "driver refused hint",
                        hint_applied: true,
                    })
                } else {
                    Ok("opened")
                }
            },
            |_| warnings += 1,
        );
        assert_eq!(result, Ok("opened"));
        assert_eq!(warnings, 1);
        assert_eq!(attempts.len(), 2);
        assert_eq!(
            attempts[0]
                .iter()
                .filter(|(key, _)| *key != "scenario")
                .collect::<Vec<_>>(),
            attempts[1].iter().collect::<Vec<_>>()
        );
    }
    #[test]
    fn qsv_failure_without_hint_propagates_and_other_backends_never_retry() {
        for kind in [
            BackendKind::Qsv,
            BackendKind::X264,
            BackendKind::Amf,
            BackendKind::Mf,
            BackendKind::OpenH264,
        ] {
            let mut hints = Vec::new();
            let result: std::result::Result<(), _> = open_backend(
                kind,
                |hint| {
                    hints.push(hint);
                    Err(OpenFailure {
                        error: "open failed",
                        hint_applied: kind == BackendKind::Qsv && hint,
                    })
                },
                |_| {},
            );
            assert_eq!(result, Err("open failed"));
            assert_eq!(
                hints,
                if kind == BackendKind::Qsv {
                    vec![true, false]
                } else {
                    vec![false]
                }
            );
        }
        let mut count = 0;
        let result: std::result::Result<(), _> = open_backend(
            BackendKind::Qsv,
            |_| {
                count += 1;
                Err(OpenFailure::from("hint not applied"))
            },
            |_| {},
        );
        assert_eq!(result, Err("hint not applied"));
        assert_eq!(count, 1);
    }

    fn options(kind: BackendKind) -> Vec<(&'static str, String)> {
        backend_options(kind, &EncoderConfig::default())
    }
    #[test]
    fn backend_contracts_preserve_required_options() {
        let expected: &[(BackendKind, &[(&str, &str)])] = &[
            (
                BackendKind::Amf,
                &[
                    ("usage", "ultralowlatency"),
                    ("quality", "balanced"),
                    ("rc", "cbr"),
                    ("preanalysis", "0"),
                    ("flags", "+low_delay"),
                    ("forced_idr", "1"),
                    ("async_depth", "1"),
                ],
            ),
            (
                BackendKind::Qsv,
                &[
                    ("preset", "medium"),
                    ("scenario", "displayremoting"),
                    ("async_depth", "1"),
                    ("look_ahead", "0"),
                    ("forced_idr", "1"),
                ],
            ),
            (
                BackendKind::Mf,
                &[
                    ("hw_encoding", "false"),
                    ("rate_control", "ld_vbr"),
                    ("scenario", "display_remoting"),
                    ("flags", "+low_delay"),
                ],
            ),
            (
                BackendKind::OpenH264,
                &[
                    ("rc_mode", "bitrate"),
                    ("allow_skip_frames", "0"),
                    ("slices", "2"),
                ],
            ),
            (BackendKind::Nvenc, &[]),
        ];
        for &(kind, wanted) in expected {
            assert_eq!(
                options(kind),
                wanted
                    .iter()
                    .map(|&(k, v)| (k, v.to_owned()))
                    .collect::<Vec<_>>()
            );
        }
    }
    #[test]
    fn screen_preset_requires_qualification_and_at_most_thirty_fps() {
        for (num, den, qualified_preset) in [
            (15, 1, "superfast"),
            (30, 1, "superfast"),
            (30000, 1001, "superfast"),
            (30001, 1000, "ultrafast"),
            (60, 1, "ultrafast"),
            (0, 1, "ultrafast"),
            (1, 0, "ultrafast"),
        ] {
            let fps = FrameRate { num, den };
            assert_eq!(x264_preset(fps, true), qualified_preset);
            assert_eq!(x264_preset(fps, false), "ultrafast");
            let config = EncoderConfig {
                fps,
                ..Default::default()
            };
            assert!(
                backend_options(BackendKind::X264, &config)
                    .contains(&("preset", qualified_preset.to_owned()))
            );
            let rollback = EncoderConfig {
                compat_tuning: false,
                ..config
            };
            assert!(
                backend_options(BackendKind::X264, &rollback)
                    .contains(&("preset", "ultrafast".to_owned()))
            );
        }
    }
    #[test]
    fn x264_profiles_keep_latency_headers_and_do_not_disable_screen_tools() {
        for (profile, cabac, dct) in [
            (H264Profile::ConstrainedBaseline, 0, 0),
            (H264Profile::Main, 1, 0),
            (H264Profile::High, 1, 1),
        ] {
            let config = EncoderConfig {
                h264_profile: profile,
                ..Default::default()
            };
            let values = backend_options(BackendKind::X264, &config);
            assert!(values.contains(&("tune", "zerolatency".to_owned())));
            assert!(values.contains(&("forced-idr", "1".to_owned())));
            let params = values
                .iter()
                .find(|(k, _)| *k == "x264-params")
                .map(|(_, v)| v.as_str());
            assert_eq!(params, Some(format!("repeat-headers=1:annexb=1:scenecut=0:sliced-threads=1:cabac={cabac}:8x8dct={dct}:deblock=0,0").as_str()));
        }
    }
    #[test]
    fn absent_qsv_hint_continues_but_required_or_mf_errors_propagate() {
        let mut applied = Vec::new();
        let result =
            apply_backend_options(BackendKind::Qsv, &EncoderConfig::default(), |key, _| {
                applied.push(key.to_owned());
                if key == "scenario" {
                    Err("unsupported")
                } else {
                    Ok(())
                }
            });
        assert_eq!(result, Ok(Some("unsupported")));
        assert_eq!(applied.last().map(String::as_str), Some("forced_idr"));
        for kind in [
            BackendKind::Qsv,
            BackendKind::Mf,
            BackendKind::Amf,
            BackendKind::X264,
            BackendKind::OpenH264,
        ] {
            assert_eq!(
                apply_backend_options(kind, &EncoderConfig::default(), |_, _| Err("required")),
                Err("required")
            );
        }
        assert_eq!(
            apply_backend_options(BackendKind::Mf, &EncoderConfig::default(), |key, _| {
                if key == "scenario" {
                    Err("required")
                } else {
                    Ok(())
                }
            }),
            Err("required")
        );
    }
}
