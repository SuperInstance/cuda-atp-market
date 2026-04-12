//! # cuda-atp-market
//!
//! ATP Energy Market system for FLUX agent fleets.
//! Implements metabolic economics with trust-weighted double auctions,
//! circadian rhythm modulation, apoptosis protocols, and fleet energy pools.

pub mod apoptosis;
pub mod circadian;
pub mod pool;

use std::collections::HashMap;

/// Agent energy budget — tracks current ATP, capacity, and generation rate.
///
/// ## Invariants
/// - `atp` ∈ [0, `max_atp`]
/// - `generation_rate` ≥ 0
#[derive(Debug, Clone)]
pub struct EnergyBudget {
    pub atp: f64,
    pub max_atp: f64,
    pub generation_rate: f64,
}

impl EnergyBudget {
    pub fn new(atp: f64, max_atp: f64, generation_rate: f64) -> Self {
        Self {
            atp: atp.min(max_atp),
            max_atp,
            generation_rate: generation_rate.max(0.0),
        }
    }

    /// Generate ATP for one tick: `atp = min(atp + generation_rate, max_atp)`
    pub fn tick(&mut self) {
        self.atp = (self.atp + self.generation_rate).min(self.max_atp);
    }

    /// Consume ATP. Returns `false` if insufficient.
    pub fn consume(&mut self, amount: f64) -> bool {
        if self.atp >= amount {
            self.atp -= amount;
            true
        } else {
            false
        }
    }

    /// Fraction of max capacity remaining: `atp / max_atp`
    pub fn charge_ratio(&self) -> f64 {
        if self.max_atp <= 0.0 { 0.0 } else { self.atp / self.max_atp }
    }
}

/// A completed energy transaction between two agents.
#[derive(Debug, Clone)]
pub struct EnergyTransaction {
    pub from: String,
    pub to: String,
    pub amount: f64,
    pub price: f64,
    pub timestamp: u64,
    pub trust_weight: f64,
}

/// Effective price after trust weighting:
/// `effective_price = price * (1.0 + trust_weight * 0.1)`
/// Higher trust_score → lower effective buy price / higher effective sell priority.
impl EnergyTransaction {
    pub fn effective_price(&self) -> f64 {
        self.price * (1.0 + self.trust_weight * 0.1)
    }
}

/// Trust-weighted effective price for a buy order.
/// `effective = max_price * (2.0 - trust_score)` — higher trust pays less per unit.
fn effective_buy_price(max_price: f64, trust_score: f64) -> f64 {
    max_price * (2.0 - trust_score.clamp(0.0, 1.0))
}

/// Trust-weighted effective price for a sell order.
/// `effective = min_price * (0.5 + 0.5 * trust_score)` — higher trust gets better fill.
fn effective_sell_price(min_price: f64, trust_score: f64) -> f64 {
    min_price * (0.5 + 0.5 * trust_score.clamp(0.0, 1.0))
}

#[derive(Debug, Clone)]
pub struct BuyOrder {
    pub agent_id: String,
    pub amount: f64,
    pub max_price: f64,
    pub trust_score: f64,
}

#[derive(Debug, Clone)]
pub struct SellOrder {
    pub agent_id: String,
    pub amount: f64,
    pub min_price: f64,
    pub trust_score: f64,
}

#[derive(Debug, Clone)]
pub enum Order {
    Buy(BuyOrder),
    Sell(SellOrder),
}

/// The ATP energy market — a trust-weighted double auction.
#[derive(Debug, Clone)]
pub struct AtpMarket {
    pub order_book: Vec<Order>,
    pub history: Vec<EnergyTransaction>,
    pub clearing_price: f64,
}

impl AtpMarket {
    pub fn new() -> Self {
        Self {
            order_book: Vec::new(),
            history: Vec::new(),
            clearing_price: 0.0,
        }
    }

    /// Submit a buy order. Inserted in trust-weighted priority order.
    /// Opcode: `0x98`
    pub fn submit_buy(&mut self, order: BuyOrder) -> Result<(), &'static str> {
        if order.amount <= 0.0 || order.max_price <= 0.0 {
            return Err("invalid buy order: amount and max_price must be positive");
        }
        let effective = effective_buy_price(order.max_price, order.trust_score);
        // Insert sorted by effective price descending (best buyers first)
        let idx = self.order_book.iter().position(|o| match o {
            Order::Buy(b) => effective_buy_price(b.max_price, b.trust_score) < effective,
            Order::Sell(_) => false, // buys go before sells
        }).unwrap_or_else(|| {
            // Count sells at end
            self.order_book.iter().filter(|o| matches!(o, Order::Sell(_))).count()
        });
        self.order_book.insert(idx, Order::Buy(order));
        Ok(())
    }

    /// Submit a sell order. Inserted in trust-weighted priority order.
    /// Opcode: `0x99`
    pub fn submit_sell(&mut self, order: SellOrder) -> Result<(), &'static str> {
        if order.amount <= 0.0 || order.min_price < 0.0 {
            return Err("invalid sell order: amount must be positive, min_price non-negative");
        }
        let effective = effective_sell_price(order.min_price, order.trust_score);
        // Insert sorted by effective price ascending (best sellers first)
        let idx = self.order_book.iter().position(|o| match o {
            Order::Sell(s) => effective_sell_price(s.min_price, s.trust_score) > effective,
            Order::Buy(_) => false,
        }).unwrap_or(self.order_book.len());
        self.order_book.insert(idx, Order::Sell(order));
        Ok(())
    }

    /// Clear the market using double auction matching.
    /// Opcode: `0x9A`
    ///
    /// ## Algorithm
    /// 1. Collect buy orders sorted by effective price descending (willingness to pay)
    /// 2. Collect sell orders sorted by effective price ascending (willingness to accept)
    /// 3. Match while `best_buy_effective >= best_sell_effective`
    /// 4. Clearing price = midpoint: `p_clear = (p_buy + p_sell) / 2`
    /// 5. Amount = min(buy.amount, sell.amount)
    pub fn clear(&mut self, now: u64) -> Vec<EnergyTransaction> {
        let mut buys: Vec<(usize, f64, f64)> = Vec::new(); // (idx, effective_price, amount_remaining)
        let mut sells: Vec<(usize, f64, f64)> = Vec::new();

        for (idx, order) in self.order_book.iter().enumerate() {
            match order {
                Order::Buy(b) => buys.push((idx, effective_buy_price(b.max_price, b.trust_score), b.amount)),
                Order::Sell(s) => sells.push((idx, effective_sell_price(s.min_price, s.trust_score), s.amount)),
            }
        }

        // Sort: buys descending by effective price, sells ascending
        buys.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        sells.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut transactions = Vec::new();
        let mut filled_indices = Vec::new();

        let mut bi = 0;
        let mut si = 0;
        while bi < buys.len() && si < sells.len() {
            let (buy_idx, buy_eff, buy_amt) = &mut buys[bi];
            let (sell_idx, sell_eff, sell_amt) = &mut sells[si];

            if *buy_eff < *sell_eff {
                break; // no more matches
            }

            let trade_amount = buy_amt.min(*sell_amt);
            // Clearing price = midpoint of effective prices
            let p_clear = (buy_eff + sell_eff) / 2.0;

            let (from_id, from_trust) = match &self.order_book[*sell_idx] {
                Order::Sell(s) => (s.agent_id.clone(), s.trust_score),
                _ => unreachable!(),
            };
            let (to_id, to_trust) = match &self.order_book[*buy_idx] {
                Order::Buy(b) => (b.agent_id.clone(), b.trust_score),
                _ => unreachable!(),
            };
            let trust_weight = (from_trust + to_trust) / 2.0;

            transactions.push(EnergyTransaction {
                from: from_id,
                to: to_id,
                amount: trade_amount,
                price: p_clear,
                timestamp: now,
                trust_weight,
            });

            *buy_amt -= trade_amount;
            *sell_amt -= trade_amount;
            if *buy_amt <= f64::EPSILON { bi += 1; }
            if *sell_amt <= f64::EPSILON { si += 1; }

            filled_indices.push(*buy_idx);
            filled_indices.push(*sell_idx);
            self.clearing_price = p_clear;
        }

        // Remove filled orders (reverse sort to preserve indices)
        filled_indices.sort_unstable();
        filled_indices.dedup();
        for idx in filled_indices.into_iter().rev() {
            self.order_book.remove(idx);
        }

        self.history.extend(transactions.clone());
        transactions
    }

    /// Current clearing price. Returns 0.0 if no clears have occurred.
    pub fn get_price(&self) -> f64 {
        self.clearing_price
    }
}

/// Opcode: `0x98` — Submit buy order
/// Opcode: `0x99` — Submit sell order
/// Opcode: `0x9A` — Clear market

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_budget_tick() {
        let mut b = EnergyBudget::new(5.0, 100.0, 10.0);
        b.tick();
        assert_eq!(b.atp, 15.0);
        b.tick();
        assert_eq!(b.atp, 25.0);
    }

    #[test]
    fn energy_budget_capped() {
        let mut b = EnergyBudget::new(95.0, 100.0, 20.0);
        b.tick();
        assert_eq!(b.atp, 100.0); // capped at max
    }

    #[test]
    fn market_clear_simple() {
        let mut m = AtpMarket::new();
        m.submit_buy(BuyOrder { agent_id: "buyer1".into(), amount: 10.0, max_price: 5.0, trust_score: 0.5 }).unwrap();
        m.submit_sell(SellOrder { agent_id: "seller1".into(), amount: 10.0, min_price: 3.0, trust_score: 0.5 }).unwrap();
        let txs = m.clear(1000);
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].amount, 10.0);
        assert!(m.get_price() > 0.0);
    }

    #[test]
    fn market_clear_no_match() {
        let mut m = AtpMarket::new();
        m.submit_buy(BuyOrder { agent_id: "cheap".into(), amount: 10.0, max_price: 1.0, trust_score: 0.5 }).unwrap();
        m.submit_sell(SellOrder { agent_id: "expensive".into(), amount: 10.0, min_price: 5.0, trust_score: 0.5 }).unwrap();
        let txs = m.clear(1000);
        assert!(txs.is_empty());
    }

    #[test]
    fn market_trust_priority() {
        let mut m = AtpMarket::new();
        // Trusted buyer should get better effective price
        m.submit_buy(BuyOrder { agent_id: "trusted".into(), amount: 10.0, max_price: 5.0, trust_score: 1.0 }).unwrap();
        m.submit_buy(BuyOrder { agent_id: "untrusted".into(), amount: 10.0, max_price: 5.0, trust_score: 0.0 }).unwrap();
        // One sell, limited amount
        m.submit_sell(SellOrder { agent_id: "seller".into(), amount: 10.0, min_price: 1.0, trust_score: 0.5 }).unwrap();
        let txs = m.clear(1000);
        assert_eq!(txs.len(), 1);
        assert_eq!(txs[0].to, "trusted");
    }
}
