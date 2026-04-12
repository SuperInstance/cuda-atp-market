# cuda-atp-market — Energy Economy Architecture

## Overview

`cuda-atp-market` is a metabolic economics engine for FLUX agent fleets. Each agent carries an **ATP budget** (energy) that it generates over time, consumes for actions, and trades on a shared market when local supplies run short. The design mirrors biological energy metabolism: agents have finite reserves, circadian rhythms modulate their generation rate, and catastrophic failure triggers programmed cell death (apoptosis).

## Core Modules

### `lib.rs` — Trust-Weighted Double Auction Market

Agents submit **buy orders** (need ATP) and **sell orders** (surplus ATP) to a central order book. On each clear, the market runs a **double auction**:

1. Sort buy orders by effective willingness-to-pay (descending).
2. Sort sell orders by effective willingness-to-accept (ascending).
3. Match while the best buyer's effective price ≥ best seller's effective price.
4. Clearing price = midpoint of the two effective prices.

**Trust weighting** is the key innovation: an agent's `trust_score` (range [0, 1]) modulates its effective prices so that high-trust agents get priority. Buyers with high trust pay *less* per unit (formula: `max_price × (2 − trust_score)`). Sellers with high trust get *better fill priority* (formula: `min_price × (0.5 + 0.5 × trust_score)`). This creates an incentive structure: agents that behave well earn cheaper energy access, while bad actors are priced out.

The `EnergyTransaction` record includes a `trust_weight` field (average of counterparty trust scores), which feeds back into long-term trust calculations.

### `circadian.rs` — Metabolic Rhythm Modulation

Agent energy generation is not constant — it follows a **sinusoidal circadian rhythm** parameterized by a phase ∈ [0, 1):

```
rate_effective = base_rate × (0.5 + 0.5 × cos(2π × phase))
```

Phase maps to time-of-day:
- **0.00** = midnight → cos(0) = 1 → full generation rate (rest/recharge)
- **0.25** = dawn → cos(π/2) = 0 → half-rate ramp
- **0.50** = noon → cos(π) = −1 → zero generation (fully active, not regenerating)
- **0.75** = dusk → cos(3π/2) = 0 → half-rate ramp-down

Cosine (not sine) was chosen because energy generation peaks at midnight (phase=0), matching a rest-and-recharge model. During active periods (noon), agents don't regenerate — they consume. This creates natural scarcity pressure that drives market trading.

**Fleet synchronization** is measured via the **Rayleigh test** for circular uniformity: `R = |1/N × Σ exp(2πi × φ_k)|`. R ≈ 0 means phases are random (unsynchronized fleet); R ≈ 1 means perfect alignment. A synchronized fleet can coordinate market activity (e.g., collective buying at dawn when generation is low).

**Phase advance**: each tick advances phase by `1/period` units. For `period = 1000`, one full cycle takes 1000 ticks, so each tick = 0.001 phase units.

### `apoptosis.rs` — Controlled Agent Shutdown

When an agent can no longer sustain itself, the apoptosis protocol orchestrates its graceful removal:

**Decision logic** uses a tiered system:
- **Execute** (immediate shutdown): trust below floor, OR (energy below threshold AND starvation ≥ limit)
- **PrepareShutdown** (warning): energy low OR some starvation cycles
- **Continue**: healthy

The triple-condition trigger for execution (`energy_ratio < threshold AND starvation_count >= limit`) prevents false positives from transient dips. A single low-energy reading might be noise; three consecutive starvation cycles confirms sustained failure.

**Neighborhood veto**: neighboring agents can collectively block a shutdown if their average trust score exceeds **0.6** (60% quorum). This threshold balances:
- **Too low** (e.g., 0.3): any small coalition could keep dying agents alive, draining fleet resources.
- **Too high** (e.g., 0.9): nearly impossible to veto, removing a safety net against misdiagnosis.
- **0.6**: requires clear majority consensus; a single bad actor can't block it, but a concerned neighborhood can.

**Shutdown sequence order matters**:
1. **Drain energy** → move remaining ATP to fleet pool (resource preservation)
2. **Revoke trust** → invalidate credentials (security)
3. **Notify fleet** → broadcast departure (coordination)
4. **Mark tombstone** → final record (audit trail)

Energy is drained BEFORE trust revocation so the agent's identity is still valid during the transfer. Trust is revoked BEFORE notification so fleet members know the agent is no longer authenticated. The tombstone is last because it's the irreversible commitment.

### `pool.rs` — Shared Fleet Energy Reserve

The `FleetPool` is a communal ATP bank that agents can contribute to and draw from:

**Trust boost from contributions** uses a logarithmic scale:
```
trust_boost = ln(1 + total_contributed)
```

Log scaling provides **diminishing returns**: the first 10 ATP contributed gives `ln(11) ≈ 2.4` boost, while going from 1000→1010 gives only `ln(1011) − ln(1001) ≈ 0.01`. This prevents gaming (a rich agent can't buy infinite priority) while still rewarding consistent contributors.

**Crisis mode** activates rationing: withdrawals are capped at `max_withdrawal` (default 100 ATP per request). This prevents a single agent from draining the shared reserve during fleet-wide energy scarcity. Contributors still get a bonus (`2 × trust_boost` extra ATP on top of the capped amount), but within the rationing framework.

## Opcode Map

| Opcode | Module      | Action                    |
|--------|-------------|---------------------------|
| `0x98` | lib.rs      | Submit buy order          |
| `0x99` | lib.rs      | Submit sell order         |
| `0x9A` | lib.rs      | Clear market              |
| `0xC0` | apoptosis   | Check agent health        |
| `0xC1` | apoptosis   | Submit veto               |
| `0xC2` | apoptosis   | Execute shutdown          |
| `0xC3` | pool        | Contribute to pool        |
| `0xC4` | pool        | Request from pool         |
| `0xC5` | pool        | Toggle crisis mode        |

## Design Principles

1. **Scarcity drives cooperation**: circadian modulation creates predictable energy troughs that push agents toward market trading and pool contributions.
2. **Trust is currency**: high-trust agents get better market prices, bigger pool withdrawals, and veto power. Low trust means market exclusion and eventual apoptosis.
3. **Graceful degradation**: the PrepareShutdown state gives agents time to recover before execution. Crisis mode in the pool prevents catastrophic reserve depletion.
4. **Biological metaphor**: ATP, circadian rhythms, and apoptosis aren't just naming — they encode real metabolic dynamics (finite energy, periodic cycles, controlled cell death).
