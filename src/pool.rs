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
        entry.trust_boost = (1.0 + entry.total_contributed).ln();
        entry.last_contribution = now;
        entry.trust_boost
    }

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

        let effective_amount = if self.crisis_mode {
            amount.min(self.max_withdrawal)
        } else {
            amount
        };

        let actual = effective_amount.min(self.reserve);

        // Contributor bonus: if agent has contributed, allow up to 2x their trust_boost extra
        let bonus = self.contributors.get(agent_id)
            .map(|c| c.trust_boost * 2.0)
            .unwrap_or(0.0);
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
