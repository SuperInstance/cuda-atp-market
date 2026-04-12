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

// ─── SINUSOIDAL GENERATION MODULATION ─────────────────────────────────────
//
// ## The Formula
//
//   rate_effective = base_rate × (0.5 + 0.5 × cos(2π × phase))
//
// Let's trace through the key phases:
//
//   phase = 0.00 (midnight):  cos(0)       =  1.00  →  0.5 + 0.50 = 1.00  →  full rate
//   phase = 0.25 (dawn):      cos(π/2)     =  0.00  →  0.5 + 0.00 = 0.50  →  half rate
//   phase = 0.50 (noon):      cos(π)       = -1.00  →  0.5 - 0.50 = 0.00  →  zero rate
//   phase = 0.75 (dusk):      cos(3π/2)    =  0.00  →  0.5 + 0.00 = 0.50  →  half rate
//   phase = 1.00 (midnight):  cos(2π)      =  1.00  →  0.5 + 0.50 = 1.00  →  full rate (cycle)
//
// The modulation factor ranges from 0.0 to 1.0, so effective rate ∈ [0, base_rate].
//
// ## Why Cosine (Not Sine)?
//
// cos(0) = 1 at phase 0.0 (midnight). This means energy generation PEAKS at
// midnight and reaches ZERO at noon. This is a REST-AND-RECHARGE model:
//
//   - Midnight (rest): agent is idle, generating ATP at full rate, rebuilding reserves.
//   - Dawn (waking): generation ramps down to 50% as the agent starts consuming energy.
//   - Noon (peak activity): generation drops to 0 — the agent is fully active, burning
//     through its stored ATP reserves. This creates peak demand on the market.
//   - Dusk (winding down): generation returns to 50% as activity subsides.
//
// If we used sin(2π × phase) instead:
//   sin(0) = 0 at midnight, sin(π/2) = 1 at dawn, sin(π) = 0 at noon.
//   This would mean peak generation at dawn — but biologically, rest happens at
//   night, and dawn is when you START spending energy. Cosine correctly models
//   the biological intuition: you regenerate while sleeping, not while waking up.
//
// The (0.5 + 0.5 × ...) scaling centers the cosine around 0.5 instead of 0,
// ensuring the output is always non-negative (no negative generation rates).
// This is equivalent to: amplitude × (1 + cos(θ)) / 2, which is a standard
// "raised cosine" waveform used in signal processing.

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

// ─── PHASE ADVANCE ─────────────────────────────────────────────────────────
//
// Each tick advances phase by δ = 1/period units.
//
//   new_phase = (phase + ticks × (1/period)) mod 1.0
//
// For period = 1000:
//   - 1 tick   → phase advances by 0.001  (0.36° of arc)
//   - 250 ticks → phase = 0.25 (dawn)
//   - 500 ticks → phase = 0.50 (noon)
//   - 1000 ticks → full cycle (back to midnight)
//
// The rem_euclid ensures phase always wraps into [0, 1), even with large
// tick values or accumulated floating-point drift.

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

// ─── FLEET SYNCHRONIZATION (CIRCULAR STATISTICS) ──────────────────────────
//
// ## The Rayleigh Test for Circular Uniformity
//
// Phases are points on a unit circle (angle θ = 2π × phase). To measure
// how synchronized a fleet is, we compute the mean resultant vector:
//
//   R⃗ = (1/N) × Σ exp(2πi × φ_k)     where φ_k is each agent's phase
//
// Expanding into Cartesian components:
//
//   C = (1/N) × Σ cos(2π × φ_k)    (mean cosine component)
//   S = (1/N) × Σ sin(2π × φ_k)    (mean sine component)
//   R = √(C² + S²)                   (magnitude of mean resultant)
//
// ## Interpretation
//
//   R = 0:   Phases are uniformly distributed around the circle (random).
//             The fleet is desynchronized — agents peak and trough at different times.
//   R = 1:   All phases are identical (perfect sync).
//             The fleet moves as one — coordinated market activity.
//   R = 0.5: Moderate clustering. Agents tend to be in similar phases.
//
// ## Why This Matters for the Energy Economy
//
// A synchronized fleet (R → 1) creates PREDICTABLE market patterns:
//   - Everyone generates at midnight → oversupply → prices drop
//   - Everyone runs dry at noon → excess demand → prices spike
//   - The market can smooth this with the pool (save midnight surplus for noon demand)
//
// A desynchronized fleet (R → 0) creates STABLE but THIN markets:
//   - Constant background generation → no extreme price swings
//   - But fewer agents need to trade simultaneously → less liquidity
//
// ## The Rayleigh Test (Statistical Significance)
//
// Under the null hypothesis (phases are uniformly distributed), the expected
// value of R for N agents is approximately √(1/N). For N=10, random phases
// give R ≈ 0.32. So R > 0.5 already indicates non-random synchronization.
//
// The critical value for significance at α=0.05 is approximately:
//   R_crit ≈ √(-ln(α) / N) = √(3.0 / N)
//
// For N=10: R_crit ≈ 0.55. For N=50: R_crit ≈ 0.24.

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
    // Project each phase onto the unit circle as (cos θ, sin θ),
    // then compute the centroid. The distance from origin = synchronization.
    let (sum_cos, sum_sin) = circadians.iter().fold((0.0, 0.0), |(sc, ss), c| {
        let angle = 2.0 * PI * c.phase;
        (sc + angle.cos(), ss + angle.sin())
    });
    let mean_cos = sum_cos / n;
    let mean_sin = sum_sin / n;
    // R = |centroid| = √(C̄² + S̄²)
    // This is the test statistic for the Rayleigh test of circular uniformity.
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
