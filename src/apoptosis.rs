//! Apoptosis protocol for FLUX agent lifecycle management.
//!
//! Models controlled agent shutdown based on metabolic failure signals,
//! with neighborhood trust quorum veto power.

use crate::pool::FleetPool;

/// Reasons an agent might be flagged for apoptosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApoptosisTrigger {
    EnergyBelowThreshold,
    TrustRevoked,
    StarvationCycles,
}

/// Apoptosis decision for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApoptosisDecision {
    Continue,
    PrepareShutdown,
    Execute,
}

/// Shutdown step — sequential cleanup actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownStep {
    DrainEnergy { recipient: String, amount: f64 },
    RevokeTrust { agent_id: String },
    NotifyFleet { agent_id: String, reason: String },
    MarkTombstone { agent_id: String },
}

/// Ordered list of shutdown steps.
#[derive(Debug, Clone)]
pub struct ShutdownSteps {
    pub steps: Vec<ShutdownStep>,
}

impl ShutdownSteps {
    pub fn new(steps: Vec<ShutdownStep>) -> Self {
        Self { steps }
    }
}

/// Apoptosis thresholds.
#[derive(Debug, Clone)]
pub struct ApoptosisConfig {
    /// Fraction of max ATP below which apoptosis triggers. Default: 0.05 (5%)
    pub energy_threshold: f64,
    /// Number of consecutive starvation cycles before shutdown. Default: 3
    pub starvation_limit: u32,
    /// Trust score below which agent is flagged. Default: 0.1
    pub trust_floor: f64,
}

impl Default for ApoptosisConfig {
    fn default() -> Self {
        Self {
            energy_threshold: 0.05,
            starvation_limit: 3,
            trust_floor: 0.1,
        }
    }
}

/// Check if an agent should undergo apoptosis.
///
/// ## Decision Logic
/// - `trust < trust_floor` → **Execute** (immediate)
/// - `charge_ratio < energy_threshold` AND `starvation >= starvation_limit` → **Execute**
/// - `charge_ratio < energy_threshold` OR `starvation > 0` → **PrepareShutdown**
/// - Otherwise → **Continue**
///
/// Opcodes: `0xC0` (check), `0xC1` (veto), `0xC2` (execute)
pub fn check_agent(
    energy_ratio: f64,
    trust: f64,
    starvation_count: u32,
    config: &ApoptosisConfig,
) -> ApoptosisDecision {
    if trust < config.trust_floor {
        return ApoptosisDecision::Execute;
    }
    if energy_ratio < config.energy_threshold && starvation_count >= config.starvation_limit {
        return ApoptosisDecision::Execute;
    }
    if energy_ratio < config.energy_threshold || starvation_count > 0 {
        return ApoptosisDecision::PrepareShutdown;
    }
    ApoptosisDecision::Continue
}

/// Neighbor trust quorum can veto a proposed shutdown.
///
/// ## Formula
/// ```
/// quorum_score = Σ(trust_i) / N
/// ```
/// Shutdown is vetoed if `quorum_score > 0.6` (60% trust quorum).
pub fn neighborhood_veto(
    _vessel_id: &str,
    neighbors: &[(String, f64)],
    proposed_shutdown: ApoptosisDecision,
) -> bool {
    if proposed_shutdown != ApoptosisDecision::Execute {
        return false;
    }
    if neighbors.is_empty() {
        return false; // no one to veto
    }
    let n = neighbors.len() as f64;
    let quorum_score: f64 = neighbors.iter().map(|(_, t)| t).sum::<f64>() / n;
    quorum_score > 0.6
}

/// Generate graceful shutdown sequence for an agent.
///
/// Steps:
/// 1. Drain remaining energy to fleet pool
/// 2. Revoke trust credentials
/// 3. Notify fleet members
/// 4. Mark tombstone
pub fn graceful_shutdown_sequence(vessel_id: &str, fleet_pool: &FleetPool) -> ShutdownSteps {
    let drain_amount = fleet_pool.reserve * 0.1; // proportional drain
    let recipient = "fleet_pool".to_string();

    ShutdownSteps::new(vec![
        ShutdownStep::DrainEnergy {
            recipient,
            amount: drain_amount,
        },
        ShutdownStep::RevokeTrust {
            agent_id: vessel_id.to_string(),
        },
        ShutdownStep::NotifyFleet {
            agent_id: vessel_id.to_string(),
            reason: "apoptosis: metabolic failure".to_string(),
        },
        ShutdownStep::MarkTombstone {
            agent_id: vessel_id.to_string(),
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apoptosis_continue() {
        let cfg = ApoptosisConfig::default();
        assert_eq!(check_agent(0.5, 0.8, 0, &cfg), ApoptosisDecision::Continue);
    }

    #[test]
    fn apoptosis_prepare() {
        let cfg = ApoptosisConfig::default();
        assert_eq!(check_agent(0.02, 0.8, 0, &cfg), ApoptosisDecision::PrepareShutdown);
    }

    #[test]
    fn apoptosis_execute_starvation() {
        let cfg = ApoptosisConfig::default();
        assert_eq!(check_agent(0.02, 0.8, 3, &cfg), ApoptosisDecision::Execute);
    }

    #[test]
    fn apoptosis_execute_trust_revoked() {
        let cfg = ApoptosisConfig::default();
        assert_eq!(check_agent(0.5, 0.05, 0, &cfg), ApoptosisDecision::Execute);
    }

    #[test]
    fn veto_prevents_shutdown() {
        let neighbors = vec![
            ("a".into(), 0.9),
            ("b".into(), 0.8),
        ];
        assert!(neighborhood_veto("v1", &neighbors, ApoptosisDecision::Execute));
    }

    #[test]
    fn veto_fails_low_trust() {
        let neighbors = vec![
            ("a".into(), 0.3),
            ("b".into(), 0.2),
        ];
        assert!(!neighborhood_veto("v1", &neighbors, ApoptosisDecision::Execute));
    }

    #[test]
    fn shutdown_sequence_steps() {
        let pool = FleetPool::new(1000.0);
        let steps = graceful_shutdown_sequence("v1", &pool);
        assert_eq!(steps.steps.len(), 4);
        assert!(matches!(&steps.steps[0], ShutdownStep::DrainEnergy { .. }));
    }
}
