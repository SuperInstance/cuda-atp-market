# cuda-atp-market

ATP Energy Market system for FLUX agent fleets. Implements metabolic economics with trust-weighted double auctions, circadian rhythm modulation, apoptosis protocols, and fleet energy pools.

## Build & Test

```bash
cargo build
cargo test
cargo run --example basic_market  # (add examples as needed)
```

## Architecture

### Core Types (`src/lib.rs`)

**EnergyBudget** — Per-agent energy state:
```
atp ∈ [0, max_atp]
generation_rate ≥ 0
tick():  atp = min(atp + generation_rate, max_atp)
consume(n): atp -= n if atp ≥ n, else fail
charge_ratio = atp / max_atp
```

**AtpMarket** — Trust-weighted double auction:
- Orders sorted by effective price (trust-adjusted)
- Buy effective price: `p_eff = max_price × (2 - trust_score)` — higher trust → lower effective cost
- Sell effective price: `p_eff = min_price × (0.5 + 0.5 × trust_score)` — higher trust → better fill priority
- Clearing: match while `best_buy_eff ≥ best_sell_eff`, price = midpoint

| Opcode | Operation |
|--------|-----------|
| `0x98` | Submit buy order |
| `0x99` | Submit sell order |
| `0x9A` | Clear market |

### Circadian Rhythm (`src/circadian.rs`)

Phase ∈ [0.0, 1.0) maps to metabolic cycle:

| Phase | State |
|-------|-------|
| 0.00 | Midnight (Resting) |
| 0.25 | Dawn (Awake) |
| 0.50 | Noon (Active) |
| 0.75 | Dusk (Tiring) |

**Generation modulation:**
```
gen = base_rate × (0.5 + 0.5 × cos(2π × phase))
```
- Midnight (phase=0): full generation (cos=1 → factor=1.0)
- Noon (phase=0.5): zero generation (cos=-1 → factor=0.0)

**Phase advance:**
```
new_phase = (phase + ticks / period) mod 1.0
```

**Fleet synchronization** (circular statistics):
```
R = |1/N × Σ exp(2πi × phase_k)|
```
R ∈ [0, 1]: 0 = random phases, 1 = perfect sync.

### Apoptosis Protocol (`src/apoptosis.rs`)

Controlled agent shutdown on metabolic failure:

| Opcode | Operation |
|--------|-----------|
| `0xC0` | Check agent health |
| `0xC1` | Neighborhood veto vote |
| `0xC2` | Execute shutdown sequence |

**Decision logic:**
- `trust < trust_floor` → **Execute** (immediate)
- `energy_ratio < threshold ∧ starvation ≥ limit` → **Execute**
- `energy_ratio < threshold ∨ starvation > 0` → **PrepareShutdown**
- Otherwise → **Continue**

**Neighborhood veto:**
```
quorum = Σ(trust_i) / N
veto if quorum > 0.6
```

**Shutdown sequence:** DrainEnergy → RevokeTrust → NotifyFleet → MarkTombstone

### Fleet Pool (`src/pool.rs`)

Shared ATP reserve with trust-weighted access:

| Opcode | Operation |
|--------|-----------|
| `0xC3` | Contribute to pool |
| `0xC4` | Request from pool |
| `0xC5` | Toggle crisis mode |

**Trust boost (log-scaled):**
```
trust_boost = ln(1 + total_contributed)
```
Early contributions weighted more heavily (diminishing returns).

**Crisis mode:** caps individual withdrawals at `max_withdrawal` to prevent drain attacks.

Contributors receive a bonus: `bonus = 2 × trust_boost` added to their withdrawal allowance.

## License

MIT
