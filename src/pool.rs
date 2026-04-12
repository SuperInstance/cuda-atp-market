//! Fleet energy pool — shared ATP reserve with trust-weighted access.
//!
//! Opcodes: `0xC3` (contribute), `0xC4` (request), `0xC5` (crisis mode)

use std::collections::HashMap;

/// Pool disbursement decision.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolDecision {
    Approved { amount: f64 },
    Partial { amount: f64 },
    Denied { reason: String },
}

/// Contributor record.
#[derive(Debug, Clone)]
pub struct Contributor {
    pub total_contributed: f64,
    pub trust_boost: f64,
    pub last_contribution: u64,
}

/// Shared fleet energy pool.
#[derive(Debug, Clone)]
pub struct FleetPool {
    pub reserve: f64,
    pub contributors: HashMap<String, Contributor>,
    pub disbursement_log: Vec<PoolDisbursement>,
    /// If true, cap withdrawals at max_withdrawal_per_request.
    pub crisis_mode: bool,
    pub max_withdrawal: f64,
    /// Minimum contributor ratio to unlock full withdrawals.
    pub crisis_unlock_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct PoolDisbursement {
    pub agent_id: String,
    pub amount: f64,
    pub timestamp: u64,
    pub justification: String,
}

impl FleetPool {
    pub fn new(initial_reserve: f64) -> Self {
        Self {
            reserve: initial_reserve,
            contributors: HashMap::new(),
            disbursement_log: Vec::new(),
            crisis_mode: false,
            max_withdrawal: 100.0,
            crisis_unlock_ratio: 0.8,
        }
    }

    // ─── LOG-SCALED TRUST BOOST ────────────────────────────────────────────
    //
    // ## Formula
    //
    //   trust_boost = ln(1 + total_contributed)
    //
    // ## Why Logarithmic Scaling?
    //
    //   Contribution   trust_boost (linear)   trust_boost (log)
    //   ────────────   ────────────────────   ───────────────────
    //   10 ATP         10.0                   2.40
    //   100 ATP        100.0                  4.62
    //   1000 ATP       1000.0                 6.91
    //   10000 ATP      10000.0                9.21
    //
    // ### Diminishing Returns
    //
    // The log function grows slower and slower. Going from 0→10 ATP gives
    // a boost of 2.40, but 1000→1010 gives only 0.01. This means:
    //   - Early contributions are highly rewarded (incentivizes participation)
    //   - Marginal additions have minimal effect (no infinite gaming)
    //   - A whale can't buy disproportionate influence by dumping ATP
    //
    // ### Gaming Prevention
    //
    // With linear scaling, an agent with 10000 ATP could get 1000× the boost
    // of a contributor with 10 ATP. This creates a plutocracy where the
    // richest agent dominates pool politics.
    //
    // With log scaling, the same 10000 ATP gives only ~3.8× the boost.
    // The ratio is bounded: the maximum possible boost advantage is limited
    // by log(1 + max_contrib) / log(1 + 1) = log(1 + C), which grows slowly.
    //
    // ### Practical Example
    //
    // Two agents contribute 10 ATP each: both get boost ≈ 2.40.
    // One agent contributes 1000 ATP: boost ≈ 6.91 (only ~3× more for 100× the ATP).
    // This means a group of small contributors can collectively out-boost a whale.
    //
    // ## Bonus Calculation in Requests
    //
    // Contributors get an extra `2 × trust_boost` ATP on top of their normal
    // withdrawal. For a contributor who's given 100 ATP (boost ≈ 4.62):
    //   bonus = 2 × 4.62 = 9.24 extra ATP per request
    //
    // This bonus is small enough not to distort the market but meaningful
    // enough to reward sustained participation.

    /// Contribute ATP to the fleet pool.
    ///
    /// ## Trust Boost Formula
    /// ```
    /// trust_boost = ln(1 + total_contributed)
    /// ```
    /// Log-scaled so early contributions matter more than marginal additions.
    /// Opcode: `0xC3`
    pub fn contribute(&mut self, agent_id: &str, amount: f64, now: u64) -> f64 {
        if amount <= 0.0 { return 0.0; }
        self.reserve += amount;
        let entry = self.contributors.entry(agent_id.to_string()).or_insert(Contributor {
            total_contributed: 0.0,
            trust_boost: 0.0,
            last_contribution: 0,
        });
        entry.total_contributed += amount;
        // ln(1 + x) where x = total contributed. The +1 ensures ln(0) = 0
        // for zero contributions, and prevents negative values.
        entry.trust_boost = (1.0 + entry.total_contributed).ln();
        entry.last_contribution = now;
        entry.trust_boost
    }

    // ─── CRISIS MODE: RATIONING FORMULA ────────────────────────────────────
    //
    // ## When Crisis Mode Activates
    //
    // Crisis mode is toggled externally (by fleet consensus or admin action)
    // when the reserve drops below a safe threshold or a systemic event occurs.
    //
    // ## Rationing Formula
    //
    //   effective_amount = min(requested_amount, max_withdrawal)    [when crisis = true]
    //   effective_amount = requested_amount                         [when crisis = false]
    //
    // Then:
    //   actual = min(effective_amount, reserve)
    //   bonus = 2 × trust_boost  (contributors only, capped by remaining reserve)
    //   dispensed = actual + bonus
    //
    // ### Example: Normal Mode
    //   Agent requests 200 ATP, reserve = 500, not contributor
    //   → dispensed = 200 (approved)
    //
    // ### Example: Crisis Mode
    //   Agent requests 200 ATP, max_withdrawal = 100, reserve = 500, not contributor
    //   → effective = min(200, 100) = 100
    //   → dispensed = 100 (approved, capped)
    //
    // ### Example: Crisis Mode + Contributor
    //   Agent requests 200 ATP, max_withdrawal = 100, reserve = 500
    //   Agent is a contributor with trust_boost = 4.62
    //   → effective = 100, actual = 100
    //   → bonus = 2 × 4.62 = 9.24
    //   → dispensed = 109.24 (approved with bonus)
    //
    // The contributor bonus persists during crisis but is proportionally tiny
    // compared to the cap. This ensures contributors still get preferential
    // treatment without undermining the rationing system.

    /// Request ATP from the pool.
    ///
    /// Priority: contributors get preference based on trust_boost.
    /// In crisis mode, withdrawals are capped at `max_withdrawal`.
    /// Opcode: `0xC4`
    pub fn request(&mut self, agent_id: &str, amount: f64, justification: &str, now: u64) -> PoolDecision {
        if amount <= 0.0 {
            return PoolDecision::Denied { reason: "non-positive amount".into() };
        }
        if self.reserve < f64::EPSILON {
            return PoolDecision::Denied { reason: "pool empty".into() };
        }

        // Crisis rationing: cap the requested amount
        let effective_amount = if self.crisis_mode {
            amount.min(self.max_withdrawal)
        } else {
            amount
        };

        // Can't dispense more than what's in reserve
        let actual = effective_amount.min(self.reserve);

        // Contributor bonus: if agent has contributed, allow up to 2x their trust_boost extra
        let bonus = self.contributors.get(agent_id)
            .map(|c| c.trust_boost * 2.0)
            .unwrap_or(0.0);
        // Bonus is also capped by remaining reserve to prevent overdraft
        let total_allowed = actual + bonus.min(self.reserve - actual);

        let dispensed = total_allowed.min(self.reserve);
        if dispensed <= f64::EPSILON {
            return PoolDecision::Denied { reason: "insufficient reserve".into() };
        }

        self.reserve -= dispensed;
        self.disbursement_log.push(PoolDisbursement {
            agent_id: agent_id.to_string(),
            amount: dispensed,
            timestamp: now,
            justification: justification.to_string(),
        });

        // Return Approved if we got ≥99% of what was asked, else Partial
        if dispensed >= amount * 0.99 {
            PoolDecision::Approved { amount: dispensed }
        } else {
            PoolDecision::Partial { amount: dispensed }
        }
    }

    /// Toggle crisis mode (rationing).
    /// Opcode: `0xC5`
    pub fn crisis_mode_toggle(&mut self, enabled: bool) {
        self.crisis_mode = enabled;
    }

    /// Get an agent's trust boost from contributions.
    pub fn get_trust_boost(&self, agent_id: &str) -> f64 {
        self.contributors.get(agent_id).map(|c| c.trust_boost).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contribute_and_trust_boost() {
        let mut pool = FleetPool::new(0.0);
        let boost = pool.contribute("a1", 10.0, 100);
        assert!(boost > 0.0);
        // ln(1 + 10) ≈ 2.398
        assert!((boost - (1.0 + 10.0_f64).ln()).abs() < 1e-9);
    }

    #[test]
    fn request_approved() {
        let mut pool = FleetPool::new(1000.0);
        match pool.request("a1", 50.0, "need energy", 200) {
            PoolDecision::Approved { amount } => assert_eq!(amount, 50.0),
            _ => panic!("expected approved"),
        }
        assert!((pool.reserve - 950.0).abs() < 1e-9);
    }

    #[test]
    fn request_partial() {
        let mut pool = FleetPool::new(30.0);
        match pool.request("a1", 50.0, "need more", 200) {
            PoolDecision::Partial { amount } => assert!((amount - 30.0).abs() < 1e-9),
            _ => panic!("expected partial"),
        }
    }

    #[test]
    fn request_denied_empty() {
        let mut pool = FleetPool::new(0.0);
        match pool.request("a1", 10.0, "nothing left", 200) {
            PoolDecision::Denied { .. } => {},
            _ => panic!("expected denied"),
        }
    }

    #[test]
    fn crisis_mode_caps() {
        let mut pool = FleetPool::new(1000.0);
        pool.crisis_mode_toggle(true);
        pool.max_withdrawal = 50.0;
        match pool.request("a1", 200.0, "crisis", 200) {
            PoolDecision::Approved { amount } => assert!((amount - 50.0).abs() < 1e-9),
            _ => panic!("expected approved with cap"),
        }
    }

    #[test]
    fn contributor_bonus() {
        let mut pool = FleetPool::new(100.0);
        pool.contribute("a1", 100.0, 100); // trust_boost = ln(101) ≈ 4.615
        match pool.request("a1", 95.0, "contributor bonus", 200) {
            PoolDecision::Approved { amount } => {
                // base 95 + bonus 2*4.615 ≈ 9.23 but capped by reserve
                assert!(amount >= 95.0);
            },
            _ => panic!("expected approved"),
        }
    }
}
