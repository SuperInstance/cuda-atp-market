//! Circadian rhythm modulation for FLUX agent energy generation.
//!
//! Models metabolic cycles as sinusoidal oscillations over phase ∈ [0, 1).
//!
//! Phase mapping:
//! - 0.00 = midnight (Resting)
//! - 0.25 = dawn    (Awake)
//! - 0.50 = noon    (Active)
//! - 0.75 = dusk    (Tiring)

use std::f64::consts::PI;

/// Circadian state of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircadianState {
    Resting,  // phase ∈ [0.75, 1.0) ∪ [0.0, 0.125) — night
    Awake,    // phase ∈ [0.125, 0.375) — dawn ramp
    Active,   // phase ∈ [0.375, 0.625) — peak metabolism
    Tiring,   // phase ∈ [0.625, 0.75) — wind-down
}

/// Agent circadian rhythm state.
#[derive(Debug, Clone)]
pub struct CircadianRhythm {
    /// Phase ∈ [0.0, 1.0). Wraps on overflow.
    pub phase: f64,
    /// Period in ticks for one full cycle.
    pub period: u64,
}

impl CircadianRhythm {
    pub fn new(phase: f64, period: u64) -> Self {
        Self {
            phase: phase.rem_euclid(1.0),
            period: if period == 0 { 1 } else { period },
        }
    }

    /// Get the current circadian state based on phase.
    pub fn get_state(&self) -> CircadianState {
        let p = self.phase.rem_euclid(1.0);
        if p < 0.125 || p >= 0.75 {
            CircadianState::Resting
        } else if p < 0.375 {
            CircadianState::Awake
        } else if p < 0.625 {
            CircadianState::Active
        } else {
            CircadianState::Tiring
        }
    }
}

/// Modulate energy generation rate by circadian phase.
///
/// ## Formula
/// ```
/// rate_effective = base_rate * (0.5 + 0.5 * cos(2π * phase))
/// ```
///
/// - At phase = 0.0 (midnight): cos(0) = 1.0 → rate = base_rate (full, resting regeneration)
/// - At phase = 0.5 (noon): cos(π) = -1.0 → rate = 0.0 (active, not generating)
///
/// Wait — for ATP *generation*, we want the opposite: generate MORE during active periods.
/// The formula above models metabolic *activity*.
///
/// For generation modulation:
/// ```
/// gen = base_rate * (0.3 + 0.7 * sin(2π * phase))
/// ```
/// - phase=0.5 (noon): sin(π)=0... no.
///
/// Let's use the original specification exactly:
/// `rate * (0.5 + 0.5 * cos(2π * phase))`
/// This gives: midnight=full, noon=zero. Energy generation is REST-focused.
pub fn modulate_generation(circadian: &CircadianRhythm, base_rate: f64, _cycle_count: u64) -> f64 {
    let modulation = 0.5 + 0.5 * (2.0 * PI * circadian.phase).cos();
    base_rate * modulation
}

/// Advance phase by `ticks`. Returns new phase.
///
/// ```
/// new_phase = (phase + ticks / period) mod 1.0
/// ```
pub fn advance_phase(circadian: &mut CircadianRhythm, ticks: u64) -> f64 {
    let delta = (ticks as f64) / (circadian.period as f64);
    circadian.phase = (circadian.phase + delta).rem_euclid(1.0);
    circadian.phase
}

/// Measure fleet phase synchronization.
///
/// ## Formula
/// Uses the magnitude of the mean resultant vector (circular statistics):
/// ```
/// R = |1/N * Σ exp(2πi * phase_k)|
/// ```
/// - R ≈ 0: phases are uniformly distributed (random)
/// - R = 1: all phases identical (perfect sync)
pub fn fleet_sync(circadians: &[CircadianRhythm]) -> f64 {
    if circadians.is_empty() { return 0.0; }
    let n = circadians.len() as f64;
    let (sum_cos, sum_sin) = circadians.iter().fold((0.0, 0.0), |(sc, ss), c| {
        let angle = 2.0 * PI * c.phase;
        (sc + angle.cos(), ss + angle.sin())
    });
    let mean_cos = sum_cos / n;
    let mean_sin = sum_sin / n;
    (mean_cos * mean_cos + mean_sin * mean_sin).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circadian_modulation_midnight() {
        let c = CircadianRhythm::new(0.0, 1000);
        // cos(0) = 1.0 → modulation = 1.0
        assert!((modulate_generation(&c, 10.0, 0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn circadian_modulation_noon() {
        let c = CircadianRhythm::new(0.5, 1000);
        // cos(π) = -1.0 → modulation = 0.0
        assert!(modulate_generation(&c, 10.0, 0).abs() < 1e-9);
    }

    #[test]
    fn circadian_advance() {
        let mut c = CircadianRhythm::new(0.0, 1000);
        advance_phase(&mut c, 500);
        assert!((c.phase - 0.5).abs() < 1e-9);
    }

    #[test]
    fn circadian_sync_perfect() {
        let circs = vec![
            CircadianRhythm::new(0.5, 1000),
            CircadianRhythm::new(0.5, 1000),
            CircadianRhythm::new(0.5, 1000),
        ];
        assert!((fleet_sync(&circs) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn circadian_state() {
        assert_eq!(CircadianRhythm::new(0.0, 100).get_state(), CircadianState::Resting);
        assert_eq!(CircadianRhythm::new(0.25, 100).get_state(), CircadianState::Awake);
        assert_eq!(CircadianRhythm::new(0.5, 100).get_state(), CircadianState::Active);
        assert_eq!(CircadianRhythm::new(0.7, 100).get_state(), CircadianState::Tiring);
    }
}
