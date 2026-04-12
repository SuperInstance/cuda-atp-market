//! Integration tests for cuda-atp-market.
//!
//! 10 tests covering market clearing, circadian math, apoptosis, and pool ops.

use cuda_atp_market::*;

#[test]
fn test_01_energy_budget_operations() {
    let mut b = EnergyBudget::new(50.0, 100.0, 10.0);
    assert!(b.consume(30.0));
    assert_eq!(b.atp, 20.0);
    assert!(!b.consume(25.0)); // not enough
    assert_eq!(b.atp, 20.0);
    assert!((b.charge_ratio() - 0.2).abs() < 1e-9);
}

#[test]
fn test_02_market_double_auction_clearing() {
    let mut m = AtpMarket::new();
    m.submit_buy(BuyOrder { agent_id: "b1".into(), amount: 5.0, max_price: 10.0, trust_score: 0.8 }).unwrap();
    m.submit_buy(BuyOrder { agent_id: "b2".into(), amount: 5.0, max_price: 8.0, trust_score: 0.5 }).unwrap();
    m.submit_sell(SellOrder { agent_id: "s1".into(), amount: 8.0, min_price: 5.0, trust_score: 0.7 }).unwrap();

    let txs = m.clear(1000);
    assert_eq!(txs.len(), 2); // two trades
    let total: f64 = txs.iter().map(|t| t.amount).sum();
    assert!((total - 8.0).abs() < 1e-9);
    assert!(m.get_price() > 0.0);
}

#[test]
fn test_03_market_no_cross() {
    let mut m = AtpMarket::new();
    m.submit_buy(BuyOrder { agent_id: "cheap".into(), amount: 10.0, max_price: 2.0, trust_score: 0.5 }).unwrap();
    m.submit_sell(SellOrder { agent_id: "dear".into(), amount: 10.0, min_price: 5.0, trust_score: 0.5 }).unwrap();
    assert!(m.clear(1000).is_empty());
    assert_eq!(m.get_price(), 0.0);
}

#[test]
fn test_04_circadian_modulation_dawn_noon() {
    let c_dawn = circadian::CircadianRhythm::new(0.25, 1000);
    let gen_dawn = circadian::modulate_generation(&c_dawn, 10.0, 1);
    // cos(π/2) = 0 → modulation = 0.5
    assert!((gen_dawn - 5.0).abs() < 1e-9);

    let c_noon = circadian::CircadianRhythm::new(0.5, 1000);
    let gen_noon = circadian::modulate_generation(&c_noon, 10.0, 1);
    assert!(gen_noon.abs() < 1e-9);
}

#[test]
fn test_05_circadian_fleet_sync() {
    let synced = (0..10).map(|_| {
        circadian::CircadianRhythm::new(0.5, 1000)
    }).collect::<Vec<_>>();
    assert!((circadian::fleet_sync(&synced) - 1.0).abs() < 1e-9);

    // Uniformly distributed → R ≈ 0
    let spread: Vec<circadian::CircadianRhythm> = (0..10)
        .map(|i| circadian::CircadianRhythm::new(i as f64 / 10.0, 1000))
        .collect();
    let r = circadian::fleet_sync(&spread);
    assert!(r < 0.3); // very low sync
}

#[test]
fn test_06_circadian_phase_advance_wrap() {
    let mut c = circadian::CircadianRhythm::new(0.8, 1000);
    circadian::advance_phase(&mut c, 500);
    // 0.8 + 0.5 = 1.3 → mod 1.0 = 0.3
    assert!((c.phase - 0.3).abs() < 1e-9);
}

#[test]
fn test_07_apoptosis_full_lifecycle() {
    let cfg = apoptosis::ApoptosisConfig::default();
    // Healthy
    assert_eq!(apoptosis::check_agent(0.5, 0.9, 0, &cfg), apoptosis::ApoptosisDecision::Continue);
    // Low energy
    assert_eq!(apoptosis::check_agent(0.03, 0.9, 0, &cfg), apoptosis::ApoptosisDecision::PrepareShutdown);
    // Starvation + low energy
    assert_eq!(apoptosis::check_agent(0.03, 0.9, 5, &cfg), apoptosis::ApoptosisDecision::Execute);
    // Trust revoked
    assert_eq!(apoptosis::check_agent(0.5, 0.05, 0, &cfg), apoptosis::ApoptosisDecision::Execute);
}

#[test]
fn test_08_apoptosis_veto() {
    let neighbors = vec![
        ("n1".into(), 1.0),
        ("n2".into(), 1.0),
        ("n3".into(), 1.0),
    ];
    // High-trust neighbors veto execution
    assert!(apoptosis::neighborhood_veto("v1", &neighbors, apoptosis::ApoptosisDecision::Execute));
    // But can't veto PrepareShutdown (only Execute)
    assert!(!apoptosis::neighborhood_veto("v1", &neighbors, apoptosis::ApoptosisDecision::PrepareShutdown));
}

#[test]
fn test_09_pool_contribute_request_cycle() {
    let mut pool = pool::FleetPool::new(0.0);
    let boost1 = pool.contribute("a1", 100.0, 100);
    let boost2 = pool.contribute("a2", 100.0, 100);
    assert!((boost1 - boost2).abs() < 1e-9); // same contribution, same boost

    match pool.request("a1", 50.0, "operational", 200) {
        pool::PoolDecision::Approved { amount } => assert_eq!(amount, 50.0),
        other => panic!("expected approved, got {:?}", other),
    }
    assert!((pool.reserve - 150.0).abs() < 1e-9);
}

#[test]
fn test_10_pool_crisis_mode() {
    let mut pool = pool::FleetPool::new(500.0);
    pool.contribute("a1", 200.0, 100);
    pool.crisis_mode_toggle(true);
    pool.max_withdrawal = 25.0;

    // Normal request capped
    match pool.request("a1", 100.0, "crisis", 200) {
        pool::PoolDecision::Approved { amount } => assert!((amount - 25.0).abs() < 1e-9),
        other => panic!("expected approved capped, got {:?}", other),
    }

    pool.crisis_mode_toggle(false);
    // Now full amount
    match pool.request("a1", 100.0, "normal", 300) {
        pool::PoolDecision::Approved { amount } => assert!((amount - 100.0).abs() < 1e-9),
        other => panic!("expected approved full, got {:?}", other),
    }
}
