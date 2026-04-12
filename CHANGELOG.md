# CHANGELOG

## 0.1.0 — Annotated Edition (2026-04-11)

### Added
- **ARCHITECTURE.md**: Comprehensive overview of the energy economy design, covering all four modules, the trust-weighting incentive structure, circadian rhythm rationale, apoptosis safety mechanisms, and the fleet pool's anti-gaming properties.

### Annotated (no code changes, documentation only)

#### `src/lib.rs` — Double Auction Market
- Added detailed block comment on effective pricing formulas:
  - Buy: `p_eff = max_price × (2 − trust_score)` with examples at trust 0.0, 0.5, 1.0
  - Sell: `p_eff = min_price × (0.5 + 0.5 × trust_score)` with examples
  - Clearing price: `p_clear = (p_eff_buy + p_eff_sell) / 2` (midpoint rule)
- Added block comment on why trust-weighting matters (prevents race-to-bottom, creates reputational economy)
- Added block comment on the matching algorithm (5-step walkthrough)
- Added inline comments on `trust_weight` field (average counterparty trust for downstream reputation)

#### `src/circadian.rs` — Circadian Rhythm
- Added detailed block comment tracing the sinusoidal formula through all four phase points (midnight, dawn, noon, dusk)
- Explained why cosine (not sine): rest-focused model, peak generation at midnight, zero at noon
- Explained raised cosine: `(0.5 + 0.5 × cos(θ))` ensures non-negative output
- Added block comment on phase advance: `δ = 1/period` per tick, with numerical examples
- Added extensive block comment on fleet sync via Rayleigh test:
  - Formula expansion into Cartesian components: `R = √(C̄² + S̄²)`
  - Interpretation: R=0 (random), R=1 (perfect sync), R=0.5 (moderate)
  - Economic implications: synchronized → predictable patterns, desynchronized → stable but thin
  - Statistical significance: critical values at α=0.05 for different N

#### `src/apoptosis.rs` — Controlled Shutdown
- Added block comment on triple-condition trigger: why energy AND starvation (prevents false positives from transient dips)
- Added block comment on quorum veto threshold (>0.6):
  - Threshold analysis table: 0.3 through 0.9 with behavioral implications
  - Why veto exists at all (heuristic vs omniscient)
  - Abuse prevention (cartel resistance)
- Added block comment on shutdown sequence order:
  - Drain → Revoke → Notify → Tombstone
  - Why each step must come before the next
  - What goes wrong if the order is violated

#### `src/pool.rs` — Fleet Energy Reserve
- Added block comment on log-scaled trust boost:
  - Comparison table: linear vs log for 10, 100, 1000, 10000 ATP
  - Diminishing returns rationale
  - Gaming prevention (whale can't buy infinite influence)
  - Practical example: group of small contributors vs single whale
  - Bonus calculation in requests
- Added block comment on crisis mode rationing formula:
  - Three worked examples (normal, crisis, crisis+contributor)
  - Why contributor bonus persists but is proportionally small during crisis
