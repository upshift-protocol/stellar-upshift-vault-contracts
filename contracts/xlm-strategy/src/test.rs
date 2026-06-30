extern crate std;

use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events as _},
    Address, Env, MuxedAddress, String,
};
use stellar_tokens::fungible::{Base, FungibleToken};

use crate::{XlmStrategy, XlmStrategyClient};

// ── Mock Asset (simple fungible token) ──────────────────────────────────

const INITIAL_SUPPLY: i128 = 10_000_000;

#[contract]
pub struct MockAsset;

#[contractimpl]
impl MockAsset {
    pub fn __constructor(e: &Env, initial_supply: i128, admin: Address) {
        Base::set_metadata(
            e,
            7,
            String::from_str(e, "Mock XLM"),
            String::from_str(e, "XLM"),
        );
        Base::mint(e, &admin, initial_supply);
    }
}

#[contractimpl(contracttrait)]
impl FungibleToken for MockAsset {
    type ContractType = stellar_tokens::fungible::Base;
}

// ── Test harness ────────────────────────────────────────────────────────

struct Setup<'a> {
    e: Env,
    vault: Address,
    controller: Address,
    asset: MockAssetClient<'a>,
    strategy: XlmStrategyClient<'a>,
}

impl<'a> Setup<'a> {
    fn new() -> Self {
        let e = Env::default();
        let admin = Address::generate(&e);
        let vault = Address::generate(&e);
        let controller = Address::generate(&e);

        let asset_addr = e.register(MockAsset, (INITIAL_SUPPLY, &admin));
        let asset = MockAssetClient::new(&e, &asset_addr);

        let strategy_addr = e.register(XlmStrategy, (&asset_addr, &vault, &controller));
        let strategy = XlmStrategyClient::new(&e, &strategy_addr);

        // Allow non-root auth so that recall_from_protocol (which does
        // token.transfer(protocol, strategy, ...) as a sub-invocation) works
        // in tests where the protocol is a plain address.
        e.mock_all_auths_allowing_non_root_auth();

        // Fund the vault address so it can transfer tokens to the strategy.
        asset.transfer(&admin, &vault, &1_000_000);

        Setup {
            e,
            vault,
            controller,
            asset,
            strategy,
        }
    }

    fn strategy_balance(&self) -> i128 {
        self.asset.balance(&self.strategy.address)
    }

    fn vault_balance(&self) -> i128 {
        self.asset.balance(&self.vault)
    }

    /// Send tokens from vault to strategy and notify via `deposit` — the
    /// full vault → strategy flow. Under F1 this is the only path that
    /// increments `local_balance`; a bare token transfer (see
    /// `donate_to_strategy`) leaves the tracker at zero and is treated as
    /// an external donation.
    fn send_to_strategy(&self, amount: i128) {
        self.asset
            .transfer(&self.vault, &self.strategy.address, &amount);
        self.strategy.deposit(&self.vault, &amount);
    }

    /// Simulate a SEP-41 donation: a direct token transfer to the strategy
    /// that bypasses the vault's `deposit` notification. `local_balance`
    /// is unaffected; `get_balance` emits `DonationDetected`; the donation
    /// can only leave the strategy via `recover_donation`.
    fn donate_to_strategy(&self, amount: i128) {
        self.asset
            .transfer(&self.vault, &self.strategy.address, &amount);
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Constructor / view tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_constructor_stores_addresses() {
    let s = Setup::new();
    assert_eq!(s.strategy.get_vault(), s.vault);
    assert_eq!(s.strategy.get_controller(), s.controller);
    assert_eq!(s.strategy.get_asset(), s.asset.address);
}

#[test]
fn test_get_balance_initially_zero() {
    let s = Setup::new();
    assert_eq!(s.strategy.get_balance(), 0);
}

#[test]
fn test_get_balance_reflects_transfers() {
    let s = Setup::new();
    s.send_to_strategy(500);
    assert_eq!(s.strategy.get_balance(), 500);
}

#[test]
fn test_get_balance_includes_deployed_capital() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    // Deploy 4k to protocol — idle=6k, deployed=4k
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &4_000);
    assert_eq!(s.strategy.get_balance(), 10_000); // idle + deployed

    // Recall 2k — idle=8k, deployed=2k
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &2_000);
    assert_eq!(s.strategy.get_balance(), 10_000); // unchanged total
}

#[test]
fn test_deployed_total_initially_zero() {
    let s = Setup::new();
    assert_eq!(s.strategy.get_deployed_total(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_constructor_rejects_vault_equals_controller() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let same_addr = Address::generate(&e);
    let asset_addr = e.register(MockAsset, (INITIAL_SUPPLY, &admin));
    // vault == controller should be rejected
    e.register(XlmStrategy, (&asset_addr, &same_addr, &same_addr));
}

// ══════════════════════════════════════════════════════════════════════════
// deposit (IStrategy) tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deposit_zero_is_noop() {
    let s = Setup::new();
    // Zero-amount deposit should succeed without auth checks.
    s.strategy.deposit(&s.vault, &0);
}

#[test]
fn test_deposit_from_vault_succeeds() {
    let s = Setup::new();
    // `send_to_strategy` covers both the transfer and the `deposit` notify.
    s.send_to_strategy(1_000);
    assert_eq!(s.strategy_balance(), 1_000);
    assert_eq!(s.strategy.get_local_balance(), 1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deposit_negative_amount_fails() {
    let s = Setup::new();
    s.strategy.deposit(&s.vault, &-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_deposit_from_non_vault_fails() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    s.strategy.deposit(&rando, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_deposit_from_controller_fails() {
    let s = Setup::new();
    s.strategy.deposit(&s.controller, &100);
}

// ══════════════════════════════════════════════════════════════════════════
// withdraw (IStrategy) tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_withdraw_zero_returns_zero() {
    let s = Setup::new();
    let result = s.strategy.withdraw(&s.vault, &0);
    assert_eq!(result, 0);
}

#[test]
fn test_withdraw_negative_returns_zero() {
    let s = Setup::new();
    let result = s.strategy.withdraw(&s.vault, &-5);
    assert_eq!(result, 0);
}

#[test]
fn test_withdraw_to_vault_succeeds() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let before = s.vault_balance();
    let actual = s.strategy.withdraw(&s.vault, &3_000);
    assert_eq!(actual, 3_000);
    assert_eq!(s.vault_balance(), before + 3_000);
    assert_eq!(s.strategy_balance(), 2_000);
}

#[test]
fn test_withdraw_capped_by_balance() {
    let s = Setup::new();
    s.send_to_strategy(200);
    let actual = s.strategy.withdraw(&s.vault, &1_000);
    assert_eq!(actual, 200);
    assert_eq!(s.strategy_balance(), 0);
}

#[test]
fn test_withdraw_full_balance() {
    let s = Setup::new();
    s.send_to_strategy(7_777);
    let actual = s.strategy.withdraw(&s.vault, &7_777);
    assert_eq!(actual, 7_777);
    assert_eq!(s.strategy_balance(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_withdraw_to_non_vault_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let rando = Address::generate(&s.e);
    s.strategy.withdraw(&rando, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_withdraw_to_controller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.strategy.withdraw(&s.controller, &500);
}

// ══════════════════════════════════════════════════════════════════════════
// deploy_to_protocol tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deploy_to_protocol_succeeds() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &4_000);

    assert_eq!(s.strategy_balance(), 6_000);
    assert_eq!(s.asset.balance(&protocol), 4_000);
}

#[test]
fn test_deploy_to_protocol_updates_deployed_total() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &4_000);
    assert_eq!(s.strategy.get_deployed_total(), 4_000);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &3_000);
    assert_eq!(s.strategy.get_deployed_total(), 7_000);
}

#[test]
fn test_deploy_to_protocol_emits_event() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &4_000);

    let events = s.e.events().all();
    let last = events.events().last().unwrap();
    let debug = std::format!("{last:?}");
    assert!(
        debug.contains("deployed_to_protocol"),
        "Expected DeployedToProtocol event, got: {debug}"
    );
}

#[test]
fn test_deploy_to_protocol_full_balance() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);

    assert_eq!(s.strategy_balance(), 0);
    assert_eq!(s.asset.balance(&protocol), 5_000);
    assert_eq!(s.strategy.get_deployed_total(), 5_000);
}

#[test]
fn test_deploy_to_protocol_multiple_protocols() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol_a = Address::generate(&s.e);
    let protocol_b = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol_a, &3_000);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol_b, &2_000);

    assert_eq!(s.strategy_balance(), 5_000);
    assert_eq!(s.asset.balance(&protocol_a), 3_000);
    assert_eq!(s.asset.balance(&protocol_b), 2_000);
    assert_eq!(s.strategy.get_deployed_total(), 5_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_deploy_to_protocol_non_controller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let rando = Address::generate(&s.e);
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&rando, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_deploy_to_protocol_vault_as_caller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&s.vault, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deploy_to_protocol_zero_amount_fails() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&s.controller, &protocol, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_deploy_to_protocol_negative_amount_fails() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &-100);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_deploy_to_protocol_insufficient_balance_fails() {
    let s = Setup::new();
    s.send_to_strategy(100);
    let protocol = Address::generate(&s.e);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &200);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_deploy_to_protocol_empty_strategy_fails() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&s.controller, &protocol, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_deploy_to_protocol_self_target_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.strategy
        .deploy_to_protocol(&s.controller, &s.strategy.address, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_deploy_to_protocol_vault_target_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.strategy.deploy_to_protocol(&s.controller, &s.vault, &500);
}

// ══════════════════════════════════════════════════════════════════════════
// recall_from_protocol tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_recall_from_protocol_succeeds() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &6_000);
    assert_eq!(s.strategy_balance(), 4_000);

    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &3_000);
    assert_eq!(s.strategy_balance(), 7_000);
    assert_eq!(s.asset.balance(&protocol), 3_000);
}

#[test]
fn test_recall_from_protocol_updates_deployed_total() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &6_000);
    assert_eq!(s.strategy.get_deployed_total(), 6_000);

    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &4_000);
    assert_eq!(s.strategy.get_deployed_total(), 2_000);
}

#[test]
fn test_recall_from_protocol_deployed_total_saturates_at_zero() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &3_000);

    // Fund protocol externally so it has more than was deployed.
    s.asset.transfer(&s.vault, &protocol, &2_000);

    // Recall more than was deployed — deployed_total should saturate at 0.
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &5_000);

    // Check the event BEFORE any other calls (events reset on next invocation).
    // The event should report the actual transfer amount (5000), not the
    // capped deployed_total delta (3000).
    let all_events = s.e.events().all();
    let debug = std::format!("{all_events:?}");
    assert!(
        debug.contains("recalled_from_protocol"),
        "Expected RecalledFromProtocol event in: {debug}"
    );
    assert!(
        debug.contains("5000"),
        "Event should contain amount=5000 (actual transfer), got: {debug}"
    );

    assert_eq!(s.strategy.get_deployed_total(), 0);
}

#[test]
fn test_recall_from_protocol_emits_event() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &6_000);
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &3_000);

    let events = s.e.events().all();
    let last = events.events().last().unwrap();
    let debug = std::format!("{last:?}");
    assert!(
        debug.contains("recalled_from_protocol"),
        "Expected RecalledFromProtocol event, got: {debug}"
    );
}

#[test]
fn test_recall_from_protocol_full_amount() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &5_000);

    assert_eq!(s.strategy_balance(), 5_000);
    assert_eq!(s.asset.balance(&protocol), 0);
    assert_eq!(s.strategy.get_deployed_total(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_recall_from_protocol_non_controller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let protocol = Address::generate(&s.e);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &500);

    let rando = Address::generate(&s.e);
    s.strategy.recall_from_protocol(&rando, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_recall_from_protocol_vault_as_caller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let protocol = Address::generate(&s.e);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &500);

    s.strategy.recall_from_protocol(&s.vault, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_recall_from_protocol_zero_amount_fails() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_recall_from_protocol_negative_amount_fails() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &-50);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_recall_from_protocol_self_target_fails() {
    let s = Setup::new();
    s.strategy
        .recall_from_protocol(&s.controller, &s.strategy.address, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_recall_from_protocol_vault_target_fails() {
    let s = Setup::new();
    s.strategy
        .recall_from_protocol(&s.controller, &s.vault, &100);
}

// ══════════════════════════════════════════════════════════════════════════
// set_controller tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_controller_succeeds() {
    let s = Setup::new();
    let new_controller = Address::generate(&s.e);

    s.strategy.set_controller(&s.controller, &new_controller);
    assert_eq!(s.strategy.get_controller(), new_controller);
}

#[test]
fn test_set_controller_emits_event() {
    let s = Setup::new();
    let new_controller = Address::generate(&s.e);

    s.strategy.set_controller(&s.controller, &new_controller);

    let events = s.e.events().all();
    let last = events.events().last().unwrap();
    let debug = std::format!("{last:?}");
    assert!(
        debug.contains("controller_changed"),
        "Expected ControllerChanged event, got: {debug}"
    );
}

#[test]
fn test_set_controller_to_same_address() {
    let s = Setup::new();
    // Setting controller to the same address is a valid no-op.
    s.strategy.set_controller(&s.controller, &s.controller);
    assert_eq!(s.strategy.get_controller(), s.controller);
}

#[test]
fn test_new_controller_can_deploy() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let new_controller = Address::generate(&s.e);
    let protocol = Address::generate(&s.e);

    s.strategy.set_controller(&s.controller, &new_controller);

    s.strategy
        .deploy_to_protocol(&new_controller, &protocol, &1_000);
    assert_eq!(s.strategy_balance(), 4_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_old_controller_rejected_after_rotation() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let new_controller = Address::generate(&s.e);
    let protocol = Address::generate(&s.e);

    s.strategy.set_controller(&s.controller, &new_controller);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_set_controller_by_non_controller_fails() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    let new_controller = Address::generate(&s.e);
    s.strategy.set_controller(&rando, &new_controller);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_set_controller_by_vault_fails() {
    let s = Setup::new();
    let new_controller = Address::generate(&s.e);
    s.strategy.set_controller(&s.vault, &new_controller);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_set_controller_to_vault_fails() {
    let s = Setup::new();
    s.strategy.set_controller(&s.controller, &s.vault);
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")]
fn test_set_controller_to_strategy_self_fails() {
    let s = Setup::new();
    s.strategy
        .set_controller(&s.controller, &s.strategy.address);
}

#[test]
fn test_double_rotation() {
    let s = Setup::new();
    let second = Address::generate(&s.e);
    let third = Address::generate(&s.e);

    s.strategy.set_controller(&s.controller, &second);
    assert_eq!(s.strategy.get_controller(), second);

    s.strategy.set_controller(&second, &third);
    assert_eq!(s.strategy.get_controller(), third);
}

// ══════════════════════════════════════════════════════════════════════════
// TTL tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_extend_ttl_permissionless() {
    let s = Setup::new();
    // extend_ttl has no auth requirement — anyone can call it.
    s.strategy.extend_ttl();
}

#[test]
fn test_deposit_bumps_ttl() {
    let s = Setup::new();
    s.send_to_strategy(100);
    // If bump_instance failed this would panic; passing is sufficient.
}

// ══════════════════════════════════════════════════════════════════════════
// Deployed capital tracking tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_deployed_total_tracks_deploy_and_recall() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &3_000);
    assert_eq!(s.strategy.get_deployed_total(), 3_000);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &2_000);
    assert_eq!(s.strategy.get_deployed_total(), 5_000);

    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &1_000);
    assert_eq!(s.strategy.get_deployed_total(), 4_000);

    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &4_000);
    assert_eq!(s.strategy.get_deployed_total(), 0);
}

#[test]
fn test_deployed_total_across_multiple_protocols() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let proto_a = Address::generate(&s.e);
    let proto_b = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &proto_a, &3_000);
    s.strategy
        .deploy_to_protocol(&s.controller, &proto_b, &2_000);
    assert_eq!(s.strategy.get_deployed_total(), 5_000);

    s.strategy
        .recall_from_protocol(&s.controller, &proto_a, &1_000);
    assert_eq!(s.strategy.get_deployed_total(), 4_000);
}

// ══════════════════════════════════════════════════════════════════════════
// End-to-end flow tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_cycle_deposit_deploy_recall_withdraw() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);

    // 1. Vault sends tokens to strategy (simulates deposit_to_subaccount).
    s.send_to_strategy(10_000);
    assert_eq!(s.strategy_balance(), 10_000);

    // 2. Controller deploys to protocol.
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &8_000);
    assert_eq!(s.strategy_balance(), 2_000);
    assert_eq!(s.asset.balance(&protocol), 8_000);
    assert_eq!(s.strategy.get_deployed_total(), 8_000);

    // 3. Controller recalls from protocol.
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &8_000);
    assert_eq!(s.strategy_balance(), 10_000);
    assert_eq!(s.strategy.get_deployed_total(), 0);

    // 4. Vault withdraws all.
    let vault_before = s.vault_balance();
    let actual = s.strategy.withdraw(&s.vault, &10_000);
    assert_eq!(actual, 10_000);
    assert_eq!(s.vault_balance(), vault_before + 10_000);
    assert_eq!(s.strategy_balance(), 0);
}

#[test]
fn test_partial_withdraw_when_funds_deployed() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);

    s.send_to_strategy(10_000);

    // Deploy 7k to protocol, leaving 3k in strategy.
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &7_000);

    // Vault can only withdraw what's in the strategy (3k), not the full 10k.
    let actual = s.strategy.withdraw(&s.vault, &10_000);
    assert_eq!(actual, 3_000);
    assert_eq!(s.strategy_balance(), 0);
    // deployed_total still reflects the 7k out in protocol.
    assert_eq!(s.strategy.get_deployed_total(), 7_000);
}

#[test]
fn test_multiple_deposits_and_withdrawals() {
    let s = Setup::new();

    // Multiple deposits from vault.
    s.send_to_strategy(1_000);
    s.send_to_strategy(2_000);
    assert_eq!(s.strategy_balance(), 3_000);

    // Partial withdrawal.
    let actual = s.strategy.withdraw(&s.vault, &1_500);
    assert_eq!(actual, 1_500);
    assert_eq!(s.strategy_balance(), 1_500);

    // Another deposit.
    s.send_to_strategy(500);
    assert_eq!(s.strategy_balance(), 2_000);
}

#[test]
fn test_controller_rotation_mid_flow() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    let new_controller = Address::generate(&s.e);

    s.send_to_strategy(10_000);

    // Original controller deploys.
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);

    // Rotate controller.
    s.strategy.set_controller(&s.controller, &new_controller);

    // New controller recalls.
    s.strategy
        .recall_from_protocol(&new_controller, &protocol, &5_000);
    assert_eq!(s.strategy_balance(), 10_000);
    assert_eq!(s.strategy.get_deployed_total(), 0);

    // Vault can still withdraw.
    let actual = s.strategy.withdraw(&s.vault, &10_000);
    assert_eq!(actual, 10_000);
}

// ══════════════════════════════════════════════════════════════════════════
// Role separation tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_controller_cannot_call_withdraw() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.strategy.withdraw(&s.controller, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_vault_cannot_call_deploy_to_protocol() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&s.vault, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_vault_cannot_call_recall_from_protocol() {
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    s.strategy.recall_from_protocol(&s.vault, &protocol, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_vault_cannot_call_set_controller() {
    let s = Setup::new();
    let new_ctrl = Address::generate(&s.e);
    s.strategy.set_controller(&s.vault, &new_ctrl);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_random_address_cannot_call_deposit() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    s.strategy.deposit(&rando, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_random_address_cannot_call_withdraw() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let rando = Address::generate(&s.e);
    s.strategy.withdraw(&rando, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_random_address_cannot_call_deploy() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let rando = Address::generate(&s.e);
    let protocol = Address::generate(&s.e);
    s.strategy.deploy_to_protocol(&rando, &protocol, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_random_address_cannot_call_recall() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    let protocol = Address::generate(&s.e);
    s.strategy.recall_from_protocol(&rando, &protocol, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_random_address_cannot_call_set_controller() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    let new_ctrl = Address::generate(&s.e);
    s.strategy.set_controller(&rando, &new_ctrl);
}

// ══════════════════════════════════════════════════════════════════════════
// settle_protocol_returns tests
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_settle_protocol_returns_succeeds() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &6_000);
    assert_eq!(s.strategy.get_deployed_total(), 6_000);

    // Settle to a lower value within the 5% decrease limit (6000 × 0.95 = 5700).
    s.strategy.settle_protocol_returns(&s.controller, &5_700);
    assert_eq!(s.strategy.get_deployed_total(), 5_700);

    // get_balance = idle(4_000) + deployed(5_700) = 9_700
    assert_eq!(s.strategy.get_balance(), 9_700);
}

#[test]
fn test_settle_protocol_returns_increase_allowed() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);

    // Increase within the 10% increase limit (5000 × 1.10 = 5500).
    s.strategy.settle_protocol_returns(&s.controller, &5_500);
    assert_eq!(s.strategy.get_deployed_total(), 5_500);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_settle_protocol_returns_to_zero_one_shot_blocked() {
    // F3: writing deployed_total to 0 in a single call exceeds the 5%
    // decrease limit and must be rejected. Legitimate wind-downs either
    // step the value down across multiple calls or recover funds via
    // `recall_from_protocol`, which brings the counter down through the
    // actual-received path (not rate-limited by this mechanism).
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);

    s.strategy.settle_protocol_returns(&s.controller, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_settle_protocol_returns_negative_fails() {
    let s = Setup::new();
    s.strategy.settle_protocol_returns(&s.controller, &-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_settle_protocol_returns_non_controller_fails() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    s.strategy.settle_protocol_returns(&rando, &0);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_settle_protocol_returns_vault_caller_fails() {
    let s = Setup::new();
    s.strategy.settle_protocol_returns(&s.vault, &0);
}

#[test]
fn test_settle_protocol_returns_emits_event() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);
    // Decrease within the 5% limit (5000 × 0.95 = 4_750).
    s.strategy.settle_protocol_returns(&s.controller, &4_750);

    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        debug.contains("deployed_total_settled"),
        "Expected DeployedTotalSettled event in: {debug}"
    );
}

// ── F3: settle_protocol_returns rate-limit tests ────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_settle_protocol_returns_exceeds_increase_limit_fails() {
    let s = Setup::new();
    s.send_to_strategy(20_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &10_000);
    // 10_000 × 1.10 = 11_000 — 11_001 is one unit over the 10% increase cap.
    s.strategy.settle_protocol_returns(&s.controller, &11_001);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_settle_protocol_returns_exceeds_decrease_limit_fails() {
    let s = Setup::new();
    s.send_to_strategy(20_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &10_000);
    // 10_000 × 0.95 = 9_500 — 9_499 is one unit past the 5% decrease cap.
    s.strategy.settle_protocol_returns(&s.controller, &9_499);
}

#[test]
fn test_settle_protocol_returns_at_increase_boundary_allowed() {
    let s = Setup::new();
    s.send_to_strategy(20_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &10_000);
    // Exactly at the 10% boundary.
    s.strategy.settle_protocol_returns(&s.controller, &11_000);
    assert_eq!(s.strategy.get_deployed_total(), 11_000);
}

#[test]
fn test_settle_protocol_returns_at_decrease_boundary_allowed() {
    let s = Setup::new();
    s.send_to_strategy(20_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &10_000);
    // Exactly at the 5% boundary.
    s.strategy.settle_protocol_returns(&s.controller, &9_500);
    assert_eq!(s.strategy.get_deployed_total(), 9_500);
}

#[test]
fn test_settle_protocol_returns_from_zero_unrestricted() {
    // When the baseline is zero the bps-based limit is undefined; the first
    // settle after a clean start bootstraps the tracker to any non-negative
    // value. Matches the vault's `old_deployed > 0` exemption.
    let s = Setup::new();
    s.strategy
        .settle_protocol_returns(&s.controller, &1_000_000i128);
    assert_eq!(s.strategy.get_deployed_total(), 1_000_000i128);
}

#[test]
fn test_settle_protocol_returns_noop_allowed() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);

    // Same value — zero delta — always allowed; still emits the audit event.
    s.strategy.settle_protocol_returns(&s.controller, &5_000);
    assert_eq!(s.strategy.get_deployed_total(), 5_000);
}

/// Recall path is not gated by the settle rate limit: a single large
/// `recall_from_protocol` can bring `deployed_total` down to 0 in one
/// transaction because the decrement is driven by actual balance-differenced
/// receipts, not operator-reported values. F3 specifically protects the
/// settle path, which is the only way to move `deployed_total` without a
/// corresponding on-chain token movement.
#[test]
fn test_recall_path_not_gated_by_settle_rate_limit() {
    let s = Setup::new();
    s.send_to_strategy(20_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &10_000);
    // Fund protocol so the recall transfer can succeed.
    s.asset.transfer(&s.vault, &protocol, &0);

    // Full recall: deployed_total goes 10_000 → 0, larger than the 5%
    // settle decrease cap, but unrestricted on the recall path.
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &10_000);
    assert_eq!(s.strategy.get_deployed_total(), 0);
}

// ══════════════════════════════════════════════════════════════════════════
// DeployedTotalUnderflow event test
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_recall_from_protocol_underflow_event_fields() {
    let s = Setup::new();
    s.send_to_strategy(5_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &3_000);

    // Fund protocol externally so it has more than was deployed.
    s.asset.transfer(&s.vault, &protocol, &2_000);

    // Recall more than was deployed — should emit DeployedTotalUnderflow.
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &5_000);

    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        debug.contains("deployed_total_underflow"),
        "Expected DeployedTotalUnderflow event in: {debug}"
    );
    // Event should contain tracked=3000 and actual_recall=5000
    assert!(
        debug.contains("3000"),
        "Event should contain tracked=3000, got: {debug}"
    );
    assert!(
        debug.contains("5000"),
        "Event should contain actual_recall=5000, got: {debug}"
    );
    assert_eq!(s.strategy.get_deployed_total(), 0);
}

// ══════════════════════════════════════════════════════════════════════════
// I-2 / R2-7: DepositShortfall — deposit must not silently inflate tracker
// ══════════════════════════════════════════════════════════════════════════

/// Simulates a fee-on-transfer token, partial delivery, or adverse rebase:
/// fewer tokens actually arrive at the strategy than `deposit` declares.
/// The post-deposit `actual_idle >= local_balance` check must panic with
/// `DepositShortfall`, and the tracker increment must revert (Soroban
/// transactional semantics).
///
/// R2-7 motivation: without this test, a future "simplification" that
/// removes the I-2 check (lib.rs `DepositShortfall` panic) would not
/// be caught by the suite. This is the class of regression flagged by
/// the second-round test-coverage review.
#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_deposit_rejects_when_token_delivers_less_than_declared() {
    let s = Setup::new();

    // Only 500 tokens actually at the strategy...
    s.asset.transfer(&s.vault, &s.strategy.address, &500);

    // ...but deposit is called declaring 1_000 (as a fee-on-transfer
    // token would misreport, or a buggy vault balance-diff would).
    // Post-increment: local_balance = 1_000 but token.balance = 500 <
    // 1_000 → DepositShortfall.
    s.strategy.deposit(&s.vault, &1_000);
}

#[test]
fn test_deposit_shortfall_reverts_tracker_increment() {
    // Complement to the panic test: after the rejected deposit, the
    // tracker must be unchanged. Verifies the Soroban rollback
    // assumption the I-2 fix depends on (panic unwinds all writes).
    let s = Setup::new();
    assert_eq!(s.strategy.get_local_balance(), 0);

    s.asset.transfer(&s.vault, &s.strategy.address, &500);
    let r = s.strategy.try_deposit(&s.vault, &1_000);
    assert!(r.is_err(), "declared > delivered deposit must fail");

    // Tracker rolled back to the pre-call value.
    assert_eq!(s.strategy.get_local_balance(), 0);
    // Tokens sent directly are now a "donation" from the tracker's
    // perspective — picked up on the next get_balance via the
    // DonationDetected event.
    assert_eq!(s.strategy_balance(), 500);
}

// ══════════════════════════════════════════════════════════════════════════
// F1: LocalBalance tracking + donation attack defense
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_local_balance_tracks_deposit_and_withdraw() {
    let s = Setup::new();
    assert_eq!(s.strategy.get_local_balance(), 0);

    s.send_to_strategy(1_000);
    assert_eq!(s.strategy.get_local_balance(), 1_000);

    s.strategy.withdraw(&s.vault, &400);
    assert_eq!(s.strategy.get_local_balance(), 600);
}

#[test]
fn test_local_balance_moves_with_deploy_and_recall() {
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &4_000);
    assert_eq!(s.strategy.get_local_balance(), 6_000);
    assert_eq!(s.strategy.get_deployed_total(), 4_000);

    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &3_000);
    assert_eq!(s.strategy.get_local_balance(), 9_000);
    assert_eq!(s.strategy.get_deployed_total(), 1_000);
}

#[test]
fn test_get_balance_uses_local_balance_not_raw_token_balance() {
    // The core F1 invariant: donated tokens sitting at the strategy do
    // not inflate `get_balance()`. Before the fix, a direct SEP-41
    // transfer of X would make `get_balance()` return `idle + X`,
    // enabling a donation-attack inflation of the vault's NAV.
    let s = Setup::new();
    s.send_to_strategy(1_000);
    assert_eq!(s.strategy.get_balance(), 1_000);

    // Adversary donates 9_000 directly — bypasses `deposit` notification.
    s.donate_to_strategy(9_000);

    // Raw token.balance rose, but get_balance is pinned to the tracker.
    assert_eq!(s.strategy_balance(), 10_000);
    assert_eq!(s.strategy.get_balance(), 1_000);
}

#[test]
fn test_get_balance_emits_donation_detected_on_excess() {
    let s = Setup::new();
    s.send_to_strategy(1_000);

    s.donate_to_strategy(500);
    let _ = s.strategy.get_balance();

    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        debug.contains("donation_detected"),
        "DonationDetected should fire when token.balance > local_balance: {debug}"
    );
}

#[test]
fn test_get_balance_does_not_emit_donation_in_normal_flow() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let _ = s.strategy.get_balance();

    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        !debug.contains("donation_detected"),
        "DonationDetected must NOT fire on clean flows: {debug}"
    );
}

/// R2-11: withdraw at exactly `local_balance` with a donation present
/// transfers exactly the tracked amount — not the tracked amount plus
/// the donation. Boundary check distinct from the over-request case.
#[test]
fn test_withdraw_exactly_local_balance_with_donation() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(9_000);

    let actual = s.strategy.withdraw(&s.vault, &1_000);
    assert_eq!(actual, 1_000);
    assert_eq!(s.strategy.get_local_balance(), 0);
    assert_eq!(s.strategy_balance(), 9_000);
}

/// R2-11: partial withdraw below `local_balance` transfers exactly the
/// requested amount, leaves the remainder tracked, preserves the donation.
#[test]
fn test_withdraw_partial_under_local_balance_with_donation() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(9_000);

    let actual = s.strategy.withdraw(&s.vault, &500);
    assert_eq!(actual, 500);
    assert_eq!(s.strategy.get_local_balance(), 500);
    // 1_000 tracked + 9_000 donated - 500 withdrawn.
    assert_eq!(s.strategy_balance(), 9_500);
}

#[test]
fn test_withdraw_caps_at_local_balance_not_token_balance() {
    // Related to F1: a donation must not be withdrawable through the
    // vault's `withdraw` path. The vault ignores the strategy's return
    // value and measures its own balance delta, so if `withdraw` ever
    // transferred donated tokens the vault would credit them into
    // `local_balance` from the vault side — creating phantom NAV.
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(9_000);

    // Vault asks for 5_000 — only 1_000 (tracked) should transit.
    let actual = s.strategy.withdraw(&s.vault, &5_000);
    assert_eq!(actual, 1_000);
    assert_eq!(s.strategy.get_local_balance(), 0);
    // Donated tokens remain at the strategy.
    assert_eq!(s.strategy_balance(), 9_000);
}

#[test]
fn test_deploy_to_protocol_rejects_donated_excess() {
    // Controller should not be able to repurpose donated tokens into
    // external protocols through `deploy_to_protocol` — that would move
    // untracked capital under the vault's NAV umbrella without first
    // passing through `recover_donation`.
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(9_000);
    let protocol = Address::generate(&s.e);

    // Attempt to deploy 5_000 — only 1_000 is vault-tracked.
    let r = s
        .strategy
        .try_deploy_to_protocol(&s.controller, &protocol, &5_000);
    assert!(r.is_err(), "deploy exceeding local_balance must fail");
}

#[test]
fn test_recover_donation_transfers_excess_to_recipient() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(400);

    let recipient = Address::generate(&s.e);
    s.strategy.recover_donation(&s.controller, &recipient, &400);

    assert_eq!(s.asset.balance(&recipient), 400);
    assert_eq!(s.strategy_balance(), 1_000);
    assert_eq!(s.strategy.get_local_balance(), 1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_recover_donation_over_excess_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(400);

    let recipient = Address::generate(&s.e);
    s.strategy.recover_donation(&s.controller, &recipient, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_recover_donation_with_no_excess_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    let recipient = Address::generate(&s.e);
    s.strategy.recover_donation(&s.controller, &recipient, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_recover_donation_non_controller_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(100);
    let rando = Address::generate(&s.e);
    s.strategy.recover_donation(&rando, &rando, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_recover_donation_to_vault_fails() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(100);
    s.strategy.recover_donation(&s.controller, &s.vault, &100);
}

#[test]
fn test_recover_donation_emits_event() {
    let s = Setup::new();
    s.send_to_strategy(1_000);
    s.donate_to_strategy(100);
    let recipient = Address::generate(&s.e);
    s.strategy.recover_donation(&s.controller, &recipient, &100);

    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        debug.contains("donation_recovered"),
        "DonationRecovered event missing: {debug}"
    );
}

#[test]
fn test_seed_local_balance_one_time_migration() {
    // Initial: local_balance = 0. Post-upgrade scenario: tokens are
    // already at the strategy from pre-F1 state. Seed the tracker to
    // reflect that legitimate balance.
    let s = Setup::new();
    s.donate_to_strategy(10_000);
    assert_eq!(s.strategy.get_local_balance(), 0);

    s.strategy.seed_local_balance(&s.controller, &10_000);
    assert_eq!(s.strategy.get_local_balance(), 10_000);

    // DonationDetected no longer fires — actual_idle == tracked.
    let _ = s.strategy.get_balance();
    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        !debug.contains("donation_detected"),
        "Post-seed get_balance must not flag the seeded funds as a donation: {debug}"
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_seed_local_balance_rejects_second_call() {
    let s = Setup::new();
    s.donate_to_strategy(10_000);
    s.strategy.seed_local_balance(&s.controller, &10_000);
    // Second call must fail — single-use migration path.
    s.strategy.seed_local_balance(&s.controller, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_seed_local_balance_non_controller_fails() {
    let s = Setup::new();
    let rando = Address::generate(&s.e);
    s.strategy.seed_local_balance(&rando, &100);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_seed_local_balance_negative_fails() {
    let s = Setup::new();
    s.strategy.seed_local_balance(&s.controller, &-1);
}

// ══════════════════════════════════════════════════════════════════════════
// C-3: seed_local_balance must not exceed actual balance
// ══════════════════════════════════════════════════════════════════════════

#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_seed_local_balance_over_actual_balance_fails() {
    // Typo / malicious seed that exceeds what's actually at the strategy
    // must be rejected — otherwise it creates a silent `tracked > actual`
    // NAV overstatement that `DonationDetected` cannot flag.
    let s = Setup::new();
    s.donate_to_strategy(1_000);
    s.strategy.seed_local_balance(&s.controller, &1_001);
}

#[test]
fn test_seed_local_balance_at_actual_balance_succeeds() {
    let s = Setup::new();
    s.donate_to_strategy(1_000);
    // Exactly at the cap — allowed.
    s.strategy.seed_local_balance(&s.controller, &1_000);
    assert_eq!(s.strategy.get_local_balance(), 1_000);
}

// ══════════════════════════════════════════════════════════════════════════
// C-1: seed_deployed_total + bootstrap latch (closes deploy→recall→settle bypass)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_seed_deployed_total_from_pristine_succeeds() {
    let s = Setup::new();
    s.strategy.seed_deployed_total(&s.controller, &500_000i128);

    // Capture events before any further contract read: Soroban's test
    // env returns events from the most recent invocation's frame, and
    // subsequent calls (even view-only `get_deployed_total`) reset
    // that buffer.
    let events = s.e.events().all();
    let debug = std::format!("{events:?}");
    assert!(
        debug.contains("deployed_total_seeded"),
        "DeployedTotalSeeded event missing: {debug}"
    );

    assert_eq!(s.strategy.get_deployed_total(), 500_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_seed_deployed_total_second_call_rejected() {
    let s = Setup::new();
    s.strategy.seed_deployed_total(&s.controller, &100);
    s.strategy.seed_deployed_total(&s.controller, &200);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")]
fn test_seed_deployed_total_rejected_after_deploy() {
    // Any successful `deploy_to_protocol` trips the bootstrap latch
    // even without an explicit seed — `seed_deployed_total` becomes
    // unreachable from that point forward.
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &1_000);
    s.strategy.seed_deployed_total(&s.controller, &1_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_settle_rate_limit_bypass_via_cycle_blocked() {
    // The C-1 attack: a compromised controller does
    // `deploy_to_protocol(self_EOA, local_balance)` to trip the bootstrap
    // latch, `recall_from_protocol(self_EOA, X)` to zero `deployed_total`
    // without losing the latch, then `settle_protocol_returns(HUGE)` to
    // inflate. The post-C-1 check_settle_rate_limit rejects step 3 —
    // with the latch set and `previous == 0`, max_change = 0, so any
    // positive delta fails.
    let s = Setup::new();
    s.send_to_strategy(10_000);
    let protocol = Address::generate(&s.e);

    // Step 1: deploy — trips the latch, deployed_total = 5000.
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &5_000);
    assert!(
        s.strategy.is_deployed_total_initialized(),
        "latch must trip on first deploy"
    );

    // Step 2: recall — deployed_total goes to 0; R2-11 invariant: the
    // latch MUST stay set even though the tracker is back to zero.
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &5_000);
    assert_eq!(s.strategy.get_deployed_total(), 0);
    assert!(
        s.strategy.is_deployed_total_initialized(),
        "latch must survive recall-to-zero"
    );

    // Step 3: settle from 0 to HUGE. Pre-C-1 this was exempt
    // (`previous == 0` bootstrap). Post-C-1: rejected.
    s.strategy
        .settle_protocol_returns(&s.controller, &1_000_000);
}

/// R2-1: a settle that writes a non-zero value on a pristine strategy
/// trips the latch as a side effect. Closes the first-call-on-pristine
/// bypass the initial C-1 fix left open.
///
/// The assertion is on the `is_deployed_total_initialized()` view
/// directly, not on a follow-up settle — because a follow-up settle
/// from `previous > 0` would fall into normal rate-limit math
/// regardless of latch state, which would let a regression
/// (latch not set) pass. See `test_settle_rate_limit_bypass_via_cycle_blocked`
/// for the latch-behaviour-only isolation via the deploy→recall→settle
/// path (previous=0 + latch=true is the only way that test's step 3
/// can panic).
#[test]
fn test_first_settle_trips_bootstrap_latch() {
    let s = Setup::new();
    assert!(!s.strategy.is_deployed_total_initialized());

    s.strategy
        .settle_protocol_returns(&s.controller, &1_000_000);

    assert!(
        s.strategy.is_deployed_total_initialized(),
        "first positive settle must trip the latch"
    );
    assert_eq!(s.strategy.get_deployed_total(), 1_000_000);
}

/// R2-1 negative edge: a first settle that writes ZERO does NOT trip
/// the latch — no real bootstrap happened. Allows a pristine strategy
/// to remain pristine through no-op settles.
#[test]
fn test_first_zero_settle_does_not_trip_latch() {
    let s = Setup::new();
    s.strategy.settle_protocol_returns(&s.controller, &0i128);
    assert!(
        !s.strategy.is_deployed_total_initialized(),
        "zero-valued settle must not trip the latch"
    );
}

#[test]
fn test_settle_rate_limit_rounding_dust_blocks_any_decrease() {
    // pr-test-analyzer gap: for small `previous`, the bps-based max_change
    // rounds to 0 under integer division. Documents the behavior as a
    // known operational constraint — dust-level `deployed_total` cannot
    // be settled downward at all (operators must use `recall` or accept
    // the dust remains). Raises an error if this behaviour changes
    // silently.
    let s = Setup::new();
    // Seed to 10 — below the 20-token threshold where 5% decrease rounds to 0.
    s.strategy.seed_deployed_total(&s.controller, &10i128);

    let r = s.strategy.try_settle_protocol_returns(&s.controller, &9);
    assert!(
        r.is_err(),
        "settle from 10 to 9 should fail due to rate-limit rounding (max_change = 10*500/10000 = 0)"
    );
}

// ══════════════════════════════════════════════════════════════════════════
// I-5: end-to-end multi-step invariant
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_lifecycle_with_donation_and_recovery() {
    // End-to-end: deposit → deploy → recall (with yield) → donation →
    // recover_donation → withdraw. The single test most likely to catch
    // a future contributor forgetting to update `local_balance` in one
    // of the paths.
    let s = Setup::new();
    let protocol = Address::generate(&s.e);
    let recovery = Address::generate(&s.e);

    // 1. Vault deposits 10_000.
    s.send_to_strategy(10_000);
    assert_eq!(s.strategy.get_local_balance(), 10_000);
    assert_eq!(s.strategy.get_balance(), 10_000);

    // 2. Deploy 6_000 to protocol.
    s.strategy
        .deploy_to_protocol(&s.controller, &protocol, &6_000);
    assert_eq!(s.strategy.get_local_balance(), 4_000);
    assert_eq!(s.strategy.get_deployed_total(), 6_000);
    assert_eq!(s.strategy.get_balance(), 10_000);

    // 3. Protocol returns 6_500 (original + 500 yield). Recall moves
    //    local_balance up by full actual_received; deployed_total
    //    saturates at 0 with an underflow event.
    s.asset.transfer(&s.vault, &protocol, &500);
    s.strategy
        .recall_from_protocol(&s.controller, &protocol, &6_500);
    assert_eq!(s.strategy.get_local_balance(), 10_500);
    assert_eq!(s.strategy.get_deployed_total(), 0);
    assert_eq!(s.strategy.get_balance(), 10_500);

    // 4. Donation of 2_000 directly to the strategy.
    s.donate_to_strategy(2_000);
    assert_eq!(s.strategy.get_local_balance(), 10_500);
    assert_eq!(s.strategy_balance(), 12_500);
    // get_balance unchanged — donation is isolated.
    assert_eq!(s.strategy.get_balance(), 10_500);

    // 5. Recover the donation to a separate recovery wallet.
    s.strategy
        .recover_donation(&s.controller, &recovery, &2_000);
    assert_eq!(s.asset.balance(&recovery), 2_000);
    assert_eq!(s.strategy_balance(), 10_500);
    assert_eq!(s.strategy.get_local_balance(), 10_500);

    // 6. Full withdraw back to vault.
    let withdrawn = s.strategy.withdraw(&s.vault, &10_500);
    assert_eq!(withdrawn, 10_500);
    assert_eq!(s.strategy.get_local_balance(), 0);
    assert_eq!(s.strategy_balance(), 0);
    assert_eq!(s.strategy.get_balance(), 0);
}
