//! Refino de tela parada: depois de um IDR sem conteúdo novo, recodifica o último quadro
//! algumas vezes como P. Com a mesma imagem o resíduo é pequeno e o controle de taxa baixa o
//! QP, então a imagem converge para nítida. Sem isso, a imagem final de uma tela parada é o
//! IDR de repouso, limitado ao VBV de um quadro (bitrate/fps).
//!
//! Lógica pura, sem relógio próprio: o chamador informa tempos monotônicos em ms.

/// Repetições depois de cada IDR. x264 do produto (ultrafast, CB, VBV de 1 quadro), 1080p30,
/// tela de texto estática, IDR + repetições (ensaio CLI de 6/nov, sem WebRTC): a 5 Mbps o PSNR
/// sobe de 22,1 dB no IDR para 34,0 dB em 8 e 55,6 dB em 30; a 2,5 Mbps, de 19,9 para 35,4 dB.
/// Cada repetição gasta cerca de um quadro de orçamento: 30 equivalem a 1 s de taxa a 30 FPS.
/// QuickSync/AMF/NVENC: não medido [VERIFICAR].
pub const REFINE_FRAMES: u32 = 30;
/// Com a imagem já refinada, uma repetição periódica mantém a entrega abaixo do keepalive da
/// Cloudflare (10 s, `KEEPALIVE_INTERVAL`), que senão reenviaria o IDR antigo e desfaria o refino.
pub const SETTLED_REPEAT_MS: u64 = 5000;
/// Pedido sem AU correspondente (fonte sem quadro, transporte recusou) expira.
const STALE_REQUEST_MS: u64 = 500;

#[derive(Debug, Default)]
pub struct StaticRefine {
    remaining: u32,
    /// Refino concluído depois do último IDR, sem conteúdo novo.
    settled: bool,
    requested_at_ms: Option<u64>,
    content_since_keyframe: bool,
    /// Repetições entregues desde o início, para o diagnóstico.
    pub frames: u64,
}

impl StaticRefine {
    pub fn new() -> Self {
        Self::default()
    }

    /// AU aceito pelo transporte. Devolve verdadeiro quando era uma repetição de refino.
    pub fn on_access_unit(&mut self, keyframe: bool) -> bool {
        let refinement = self.requested_at_ms.take().is_some() && !keyframe;
        if keyframe {
            // Todo IDR (repouso, PLI, keepalive) recomeça o refino da imagem atual.
            self.remaining = REFINE_FRAMES;
            self.settled = false;
            self.content_since_keyframe = false;
        } else if refinement {
            self.frames += 1;
            self.settled = self.remaining == 0;
        } else {
            // Conteúdo novo: o refino perde sentido até o próximo IDR.
            self.remaining = 0;
            self.settled = false;
            self.content_since_keyframe = true;
        }
        refinement
    }

    /// Delta de conteúdo real depois do último IDR; repetições não contam.
    /// Mantém o IDR de repouso sem rearmá-lo a cada refino.
    pub fn content_since_keyframe(&self) -> bool {
        self.content_since_keyframe
    }

    /// Pede uma repetição do último quadro quando a tela ficou parada por `spacing_ms`.
    /// `blocked` cobre IDR pendente ou outro redesenho em andamento.
    pub fn wants(&mut self, now_ms: u64, idle_ms: u64, spacing_ms: u64, blocked: bool) -> bool {
        if let Some(at) = self.requested_at_ms {
            if now_ms.saturating_sub(at) < STALE_REQUEST_MS {
                return false;
            }
            self.requested_at_ms = None;
        }
        if blocked {
            return false;
        }
        if self.remaining > 0 && idle_ms >= spacing_ms {
            self.remaining -= 1;
        } else if !(self.settled && idle_ms >= SETTLED_REPEAT_MS) {
            return false;
        }
        self.requested_at_ms = Some(now_ms);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simula a tela parada: cada pedido vira um AU logo depois.
    fn drain(refine: &mut StaticRefine, mut now: u64, spacing: u64) -> (u32, u64) {
        let mut repeats = 0;
        let mut last_au = now;
        // Janela curta o bastante para não incluir a repetição de tela já refinada.
        for _ in 0..REFINE_FRAMES as u64 * (spacing + 2) {
            now += 1;
            if refine.wants(now, now - last_au, spacing, false) {
                assert!(refine.on_access_unit(false));
                repeats += 1;
                last_au = now;
            }
        }
        (repeats, now)
    }

    #[test]
    fn every_keyframe_is_followed_by_bounded_refinement() {
        let mut refine = StaticRefine::new();
        assert!(!refine.wants(0, 10_000, 33, false), "sem IDR não refina");
        refine.on_access_unit(true);
        let (repeats, now) = drain(&mut refine, 0, 33);
        assert_eq!(repeats, REFINE_FRAMES);
        assert_eq!(refine.frames, u64::from(REFINE_FRAMES));
        // Imagem refinada: só a repetição periódica, abaixo do keepalive de 10 s.
        assert!(!refine.wants(now + 1, SETTLED_REPEAT_MS - 1, 33, false));
        assert!(refine.wants(now + 2, SETTLED_REPEAT_MS, 33, false));
        assert!(refine.on_access_unit(false));
        assert_eq!(refine.frames, u64::from(REFINE_FRAMES) + 1);
        // Repetições não rearmam o IDR de repouso (senão IDR + refino em laço).
        assert!(!refine.content_since_keyframe());
        // IDR de PLI/keepalive recomeça o refino.
        refine.on_access_unit(true);
        assert_eq!(drain(&mut refine, 5000, 33).0, REFINE_FRAMES);
    }

    #[test]
    fn new_content_cancels_refinement_and_arms_idle_keyframe() {
        let mut refine = StaticRefine::new();
        refine.on_access_unit(true);
        assert!(refine.wants(40, 40, 33, false));
        // O quadro entregue depois do pedido conta como repetição.
        assert!(refine.on_access_unit(false));
        // Conteúdo novo sem pedido em voo.
        assert!(!refine.on_access_unit(false));
        assert!(refine.content_since_keyframe());
        assert!(!refine.wants(1000, 1000, 33, false));
        assert!(
            !refine.wants(9000, 9000, 33, false),
            "sem IDR novo, nada de repetição"
        );
    }

    #[test]
    fn waits_for_spacing_blocking_and_in_flight_request() {
        let mut refine = StaticRefine::new();
        refine.on_access_unit(true);
        assert!(!refine.wants(10, 10, 33, false), "antes do espaçamento");
        assert!(!refine.wants(40, 40, 33, true), "IDR pendente");
        assert!(refine.wants(40, 40, 33, false));
        assert!(!refine.wants(80, 80, 33, false), "pedido ainda em voo");
        // Pedido sem AU (fonte não entregou) expira e o refino continua.
        assert!(refine.wants(40 + STALE_REQUEST_MS, 600, 33, false));
        assert!(refine.on_access_unit(false));
        assert_eq!(refine.frames, 1);
    }

    #[test]
    fn keyframe_answering_a_request_restarts_instead_of_counting() {
        let mut refine = StaticRefine::new();
        refine.on_access_unit(true);
        assert!(refine.wants(40, 40, 33, false));
        // PLI chegou e o redesenho saiu como IDR.
        assert!(!refine.on_access_unit(true));
        assert_eq!(refine.frames, 0);
        assert_eq!(drain(&mut refine, 100, 33).0, REFINE_FRAMES);
    }
}
