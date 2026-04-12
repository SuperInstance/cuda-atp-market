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

// ─── TRIPLE-CONDITION TRIGGER ──────────────────────────────────────────────
//
// The `check_agent` function uses a tiered decision model:
//
//   1. trust < trust_floor           → Execute (immediate)
//   2. energy < threshold AND starvation >= limit → Execute (confirmed failure)
//   3. energy < threshold OR starvation > 0       → PrepareShutdown (warning)
//   4. otherwise                     → Continue
//
// ## Why ALL THREE Conditions Must Be True for Execution (Rule 2)
//
// The second execution path requires BOTH low energy AND sustained starvation.
// This is a triple-condition because it checks:
//   (a) energy_ratio < 0.05   — current energy is critically low
//   (b) starvation_count >= 3  — this isn't a transient dip
//   (c) trust >= trust_floor   — trust hasn't been revoked (else rule 1 fires)
//
// Without this AND logic, a single condition could trigger false positives:
//
//   - Energy alone: an agent might dip below 5% briefly after a large action,
//     then recover from the pool or market. Killing it immediately wastes its
//     accumulated trust and contributions.
//   - Starvation alone: the agent might be temporarily unable to trade (thin
//     market, network hiccup) but still have reserves.
//
// The AND ensures the system only kills agents that are BOTH depleted AND
// unable to recover — a genuine metabolic failure, not a transient spike.
//
// The PrepareShutdown warning tier (rule 3) gives agents a grace period:
// they get at least one cycle to find energy before execution is considered.
// This prevents the system from being too aggressive while still catching
// sustained failures quickly.

/// Shutdown step — sequential cleanup actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownStep {
    DrainEnergy { recipient: String, amount: f64 },
    RevokeTrust { agent_id: String },
    NotifyFleet { agent_id: String, reason: String },
    MarkTombstone { agent_id: String },
}

// ─── GRACEFUL SHUTDOWN SEQUENCE ───────────────────────────────────────────
//
// The four shutdown steps execute in a specific order, and THE ORDER MATTERS:
//
// 1. DrainEnergy — Transfer remaining ATP to the fleet pool.
//    WHY FIRST: The agent is still authenticated and its identity is valid.
//    If we revoked trust first, the pool might reject the transfer.
//    If we notified the fleet first, other agents might try to trade with
//    a shutting-down agent, creating failed transactions.
//
// 2. RevokeTrust — Invalidate the agent's credentials.
//    WHY SECOND: After energy is drained, no further transactions should be
//    allowed. Revoking trust prevents the agent from submitting new orders,
//    requesting pool withdrawals, or voting in veto rounds.
//
// 3. NotifyFleet — Broadcast the shutdown to neighbors.
//    WHY THIRD: Now the fleet knows the agent is gone and won't try to
//    interact with it. Coming after trust revocation means the notification
//    carries weight — the fleet knows the agent's credentials are invalid.
//
// 4. MarkTombstone — Create the final audit record.
//    WHY LAST: This is the irreversible commit point. Everything before this
//    is potentially reversible (energy could be refunded, trust could be
//    restored). The tombstone is permanent — it's the death certificate.
//
// Violating this order creates vulnerabilities:
//   - Notify before drain → race conditions, agents try to trade with dying agent
//   - Revoke before drain → energy transfer rejected, ATP lost
//   - Tombstone before notify → fleet never learns agent is gone

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
    // Immediate execution: trust revoked. This is a security action,
    // not a metabolic one — the agent is compromised or exiled.
    if trust < config.trust_floor {
        return ApoptosisDecision::Execute;
    }
    // Confirmed metabolic failure: low energy AND sustained starvation.
    // Both conditions required to prevent false positives from transient dips.
    if energy_ratio < config.energy_threshold && starvation_count >= config.starvation_limit {
        return ApoptosisDecision::Execute;
    }
    // Warning tier: either condition alone triggers preparation.
    // Gives the agent one more cycle to recover before the next check.
    if energy_ratio < config.energy_threshold || starvation_count > 0 {
        return ApoptosisDecision::PrepareShutdown;
    }
    ApoptosisDecision::Continue
}

// ─── NEIGHBORHOOD QUORUM VETO ──────────────────────────────────────────────
//
// ## Why >0.6 (60% Trust Quorum)?
//
// The veto threshold controls how easy it is for neighbors to save a dying agent:
//
//   quorum_score = (1/N) × Σ trust_i    (mean neighbor trust)
//   veto = quorum_score > 0.6
//
// ### Threshold Analysis
//
//   Threshold  Effect
//   ─────────  ─────────────────────────────────────────────────────────
//   > 0.3      Very easy to veto. A single trusted neighbor (trust=0.9)
//              can block execution even if others disagree. Bad for
//              resource efficiency — dying agents drain fleet resources.
//
//   > 0.5      Moderate. Requires majority consensus but a determined
//              minority can't block it. Still somewhat lenient.
//
//   > 0.6      Current value. Requires clear supermajority. In a 5-neighbor
//              scenario with average trust 0.6, veto succeeds. But if two
//              neighbors are low-trust (0.3 each), the average drops to
//              (0.6×3 + 0.3×2)/5 = 0.48 → no veto. This means the system
//              tolerates some dissent but requires genuine majority concern.
//
//   > 0.8      Nearly impossible to veto. Defeats the purpose of having
//              a veto at all — neighbors can almost never override.
//
//   > 0.9      Pointless. No practical scenario achieves this with
//              heterogeneous trust scores.
//
// ### Why a Veto at All?
//
// The apoptosis check is a heuristic, not omniscient. A neighbor might know
// that the dying agent is about to receive a large energy transfer, or that
// the starvation was caused by a temporary market freeze. The veto is a
// "sanity check" against automated shutdown — the collective judgment of
// nearby agents can override a statistical trigger.
//
// But we don't want veto power to be abused: a cartel of agents could keep
// each other alive indefinitely, draining the fleet pool. The 0.6 threshold
// makes this expensive — you'd need most neighbors to be high-trust AND agree
// to veto, which requires sustained good behavior.

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
    // Only Execute decisions can be vetoed. PrepareShutdown and Continue
    // don't need intervention — they're not killing anyone yet.
    if proposed_shutdown != ApoptosisDecision::Execute {
        return false;
    }
    // An agent with no neighbors can't be vetoed — isolation means no
    // social safety net. This is by design: orphan agents don't get
    // protection from a community that doesn't exist.
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

    // Order matters: drain → revoke → notify → tombstone.
    // See the detailed comment block on ShutdownStep for the rationale.
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
        };
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
