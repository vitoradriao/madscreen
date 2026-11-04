use crate::{FrameRate, Result};
/// Horário absoluto em ticks, relógio fornecido pelo chamador. Sem recuperação em rajadas.
pub struct Pacer {
    origin: i64,
    frequency: i64,
    fps: FrameRate,
    next: u64,
}
impl Pacer {
    pub fn new(origin: i64, frequency: i64, fps: FrameRate) -> Result<Self> {
        FrameRate::new(fps.num, fps.den)?;
        if frequency <= 0 {
            return Err(crate::EncodeError::InvalidConfig(
                "frequência de relógio inválida",
            ));
        }
        Ok(Self {
            origin,
            frequency,
            fps,
            next: 0,
        })
    }
    pub fn deadline(&self) -> i64 {
        let ticks = i128::from(self.next) * i128::from(self.frequency) * i128::from(self.fps.den)
            / i128::from(self.fps.num);
        (i128::from(self.origin) + ticks).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }
    /// Retorna quantos intervalos foram pulados; nunca acumula frames atrasados.
    pub fn advance(&mut self, now: i64) -> u64 {
        let elapsed = i128::from(now)
            .saturating_sub(i128::from(self.origin))
            .max(0);
        let due = elapsed * i128::from(self.fps.num)
            / (i128::from(self.frequency) * i128::from(self.fps.den));
        let previous = self.next;
        self.next = self
            .next
            .saturating_add(1)
            .max((due.min(i128::from(u64::MAX)) as u64).saturating_add(1));
        self.next.saturating_sub(previous).saturating_sub(1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn injected_clock_skips_without_drift() -> Result<()> {
        let mut p = Pacer::new(0, 10_000_000, FrameRate::new(60000, 1001)?)?;
        assert_eq!(p.deadline(), 0);
        assert_eq!(p.advance(0), 0);
        assert_eq!(p.deadline(), 166833);
        assert_eq!(p.advance(10_000_000), 58);
        assert_eq!(p.deadline(), 10_010_000);
        let mut p = Pacer::new(100, 1_000_000, FrameRate::new(144, 1)?)?;
        for i in 0..144 {
            p.advance(100 + i * 1_000_000 / 144);
        }
        assert_eq!(p.deadline(), 1_000_100);
        Ok(())
    }
}
