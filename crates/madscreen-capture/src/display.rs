//! Metadados de cor da saida, sem captura de pixels nem mudanca de configuracao.
#[cfg(windows)]
#[path = "display_ffi.rs"]
mod ffi;
#[cfg(windows)]
pub use ffi::{monitor_color_info, monitor_for_target};

/// Campos independentes: ausencia de informacao nao equivale a SDR/sem HDR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayColorInfo {
    pub hdr_supported: Option<bool>,
    pub hdr_enabled: Option<bool>,
    /// Valor numerico DXGI_COLOR_SPACE_TYPE, quando GetDesc1 esta disponivel.
    pub color_space: Option<i32>,
    pub bits: Option<u32>,
}

#[cfg(any(windows, test))]
pub(crate) fn first_hdr_warning(
    info: DisplayColorInfo,
    warned: &std::sync::atomic::AtomicBool,
) -> bool {
    info.hdr_enabled == Some(true) && !warned.swap(true, std::sync::atomic::Ordering::AcqRel)
}

#[derive(Clone, Copy, Debug)]
#[cfg(any(windows, test))]
pub(crate) struct AdvancedColor {
    pub version2: bool,
    pub flags: u32,
    pub bits: u32,
    pub active_mode: u32,
}

#[cfg(any(windows, test))]
pub(crate) fn decide(
    advanced: Option<AdvancedColor>,
    dxgi: Option<(i32, u32)>,
) -> DisplayColorInfo {
    let pq = dxgi.map(|(space, _)| space == 12); // RGB_FULL_G2084_NONE_P2020
    let (hdr_supported, hdr_enabled) = match advanced {
        Some(a) if a.version2 => (
            Some(a.flags & (1 << 4) != 0),
            Some(a.flags & 2 != 0 && a.active_mode == 2),
        ),
        Some(a) => {
            // V1 includes Advanced Color on SDR monitors. Never call that HDR support.
            let supported = if pq == Some(true) {
                Some(true)
            } else if a.flags & 1 == 0 {
                Some(false)
            } else {
                None
            };
            (
                supported,
                pq.or_else(|| (a.flags & 1 == 0).then_some(false)),
            )
        }
        None => (pq.filter(|enabled| *enabled), pq),
    };
    DisplayColorInfo {
        hdr_supported,
        hdr_enabled,
        color_space: dxgi.map(|(space, _)| space),
        bits: advanced
            .map(|a| a.bits)
            .filter(|bits| *bits > 0)
            .or_else(|| dxgi.map(|(_, bits)| bits).filter(|bits| *bits > 0)),
    }
}

/// DisplayConfig clone paths can share a GDI name: do not guess a physical target.
#[cfg(any(windows, test))]
pub(crate) fn unique_match<T>(mut matches: impl Iterator<Item = T>) -> Option<T> {
    let first = matches.next()?;
    matches.next().is_none().then_some(first)
}

#[cfg(any(windows, test))]
pub(crate) fn correlated_target<'a, T>(
    monitor_name: &[u16],
    sources: impl Iterator<Item = (&'a [u16], T)>,
) -> Option<T> {
    let end = monitor_name
        .iter()
        .position(|c| *c == 0)
        .unwrap_or(monitor_name.len());
    if end == 0 {
        return None;
    }
    unique_match(sources.filter_map(|(name, target)| {
        let name_end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
        (monitor_name[..end] == name[..name_end]).then_some(target)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn v2_distinguishes_hdr_capability_user_preference_and_active_mode() {
        let info = |flags, active_mode| {
            decide(
                Some(AdvancedColor {
                    version2: true,
                    flags,
                    bits: 8,
                    active_mode,
                }),
                Some((0, 8)),
            )
        };
        assert_eq!(info(1 | 16, 0).hdr_supported, Some(true));
        assert_eq!(info(1 | 16 | 32, 0).hdr_enabled, Some(false));
        assert_eq!(info(1 | 2 | 16 | 32, 2).hdr_enabled, Some(true));
        assert_eq!(info(1 | 2, 1).hdr_supported, Some(false));
        assert_eq!(info(1 | 2, 1).hdr_enabled, Some(false));
    }
    #[test]
    fn legacy_advanced_color_on_sdr_does_not_claim_hdr_support() {
        let legacy = Some(AdvancedColor {
            version2: false,
            flags: 3,
            bits: 10,
            active_mode: 0,
        });
        assert_eq!(decide(legacy, Some((0, 8))).hdr_supported, None);
        assert_eq!(decide(legacy, Some((0, 8))).hdr_enabled, Some(false));
        assert_eq!(decide(legacy, Some((12, 10))).hdr_supported, Some(true));
        assert_eq!(decide(legacy, Some((12, 10))).hdr_enabled, Some(true));
        assert_eq!(decide(legacy, None).hdr_enabled, None);
    }
    #[test]
    fn failed_apis_remain_unknown_and_dxgi_only_has_partial_knowledge() {
        assert_eq!(decide(None, None), DisplayColorInfo::default());
        assert_eq!(decide(None, Some((0, 8))).hdr_supported, None);
        assert_eq!(decide(None, Some((12, 10))).hdr_enabled, Some(true));
        assert_eq!(decide(None, Some((0, 8))).bits, Some(8));
    }
    #[test]
    fn correlation_refuses_missing_or_ambiguous_clone_paths() {
        assert_eq!(unique_match([].into_iter()), None::<u32>);
        assert_eq!(unique_match([7].into_iter()), Some(7));
        assert_eq!(unique_match([7, 8].into_iter()), None);
    }
    #[test]
    fn correlation_uses_gdi_name_not_enumeration_index_or_trailing_bytes() {
        let a = [65, 0, 0];
        let b = [66, 0, 0];
        let b_with_tail = [66, 0, 90];
        let paths = [(a, 42), (b, 17)];
        assert_eq!(
            correlated_target(
                &b_with_tail,
                paths.iter().map(|(name, id)| (name.as_slice(), *id))
            ),
            Some(17)
        );
        let clones = [(b, 17), (b, 18)];
        assert_eq!(
            correlated_target(&b, clones.iter().map(|(name, id)| (name.as_slice(), *id))),
            None
        );
        assert_eq!(
            correlated_target(&[0], paths.iter().map(|(name, id)| (name.as_slice(), *id))),
            None
        );
    }
    #[test]
    fn hdr_warning_is_once_even_across_recreation_and_sdr_roundtrip() {
        let warned = std::sync::atomic::AtomicBool::new(false);
        let hdr = DisplayColorInfo {
            hdr_enabled: Some(true),
            ..Default::default()
        };
        let sdr = DisplayColorInfo {
            hdr_enabled: Some(false),
            ..Default::default()
        };
        assert!(!first_hdr_warning(DisplayColorInfo::default(), &warned));
        assert!(!first_hdr_warning(sdr, &warned));
        assert!(first_hdr_warning(hdr, &warned));
        assert!(!first_hdr_warning(hdr, &warned));
        assert!(!first_hdr_warning(sdr, &warned));
        assert!(!first_hdr_warning(hdr, &warned));
    }
}
