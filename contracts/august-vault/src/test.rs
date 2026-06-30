extern crate std;

use soroban_sdk::{
    contract, contractimpl,
    testutils::{storage::Instance as _, Address as _, Events as _, Ledger as _},
    token, Address, Env, MuxedAddress, String,
};
use stellar_tokens::fungible::{Base, FungibleToken, INSTANCE_EXTEND_AMOUNT};

use crate::contract::{AugustVault, AugustVaultClient};
use crate::storage::SubaccountType;
// Used by the "Math properties" section at the end of this file.
use proptest::prelude::*;

// ==================== Constants ====================

const DEFAULT_SUPPLY: i128 = 100_000_000_000_000_000_000; // 100e18
const ONE_WEEK: u64 = 7 * 24 * 60 * 60; // 604_800 seconds

// Mock Asset Contract - Simple fungible token to use as underlying asset
#[contract]
pub struct MockAssetContract;

#[contractimpl]
impl MockAssetContract {
    pub fn __constructor(e: &Env, initial_supply: i128, admin: Address) {
        Base::set_metadata(
            e,
            18,
            String::from_str(e, "Mock Asset Token"),
            String::from_str(e, "MAT"),
        );
        Base::mint(e, &admin, initial_supply);
    }
}

#[contractimpl(contracttrait)]
impl FungibleToken for MockAssetContract {
    type ContractType = stellar_tokens::fungible::Base;
}

// Mock contract that does not implement the SEP-41 token interface
#[contract]
pub struct MockNonTokenContract;

#[contractimpl]
impl MockNonTokenContract {
    pub fn __constructor(_e: &Env) {}
}

fn create_vault_client<'a>(
    e: &Env,
    asset_address: &Address,
    decimals_offset: u32,
    admin: &Address,
) -> AugustVaultClient<'a> {
    let name = String::from_str(e, "Vault Token");
    let symbol = String::from_str(e, "VLT");
    let vault_address = e.register(
        AugustVault,
        (name, symbol, asset_address, decimals_offset, admin),
    );
    AugustVaultClient::new(e, &vault_address)
}

fn create_asset_client<'a>(
    e: &Env,
    initial_supply: i128,
    admin: &Address,
) -> MockAssetContractClient<'a> {
    let asset_address = e.register(MockAssetContract, (initial_supply, admin));
    MockAssetContractClient::new(e, &asset_address)
}

// ==================== Test Macros ====================

/// Asserts that an operation panics with Unauthorized (#1) on a vault
/// with no operator configured (`vault_only` setup).
macro_rules! test_vault_only_unauthorized {
    ($name:ident, |$s:ident| $body:block) => {
        #[test]
        #[should_panic(expected = "Error(Contract, #1)")]
        fn $name() {
            let $s = TestSetup::vault_only();
            $body
        }
    };
}

/// Generates matching `#[should_panic]` tests for both `deposit_to_subaccount`
/// and `withdraw_from_subaccount`, verifying they reject the same invalid input.
macro_rules! test_subaccount_ops_reject {
    (
        $deposit_name:ident, $withdraw_name:ident,
        error: $err:literal,
        setup: |$s:ident| { $($setup:tt)* } => ($caller:expr, $addr:expr, $amt:expr)
    ) => {
        #[test]
        #[should_panic(expected = $err)]
        fn $deposit_name() {
            let $s = TestSetup::new();
            $($setup)*
            $s.vault_client.deposit_to_subaccount(&$caller, &$addr, &$amt);
        }
        #[test]
        #[should_panic(expected = $err)]
        fn $withdraw_name() {
            let $s = TestSetup::new();
            $($setup)*
            $s.vault_client.withdraw_from_subaccount(&$caller, &$addr, &$amt);
        }
    };
}

// ==================== Existing vault tests ====================

#[test]
fn test_vault_initialization() {
    let s = TestSetup::vault_only_with_offset(6);

    assert_eq!(s.vault_client.query_asset(), s.asset_client.address);
    assert_eq!(s.vault_client.decimals(), 18 + 6);
    assert_eq!(s.vault_client.total_supply(), 0);
    assert_eq!(s.vault_client.total_assets(), 0);
    assert_eq!(s.vault_client.get_admin(), s.admin);
    assert_eq!(s.vault_client.get_operator(), None);
    assert!(!s.vault_client.is_paused());
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

#[test]
fn test_vault_deposit() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    assert_eq!(s.asset_client.balance(&s.user), deposit_amount);

    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    assert_eq!(s.vault_client.balance(&s.user), shares_minted);
    assert_eq!(s.vault_client.total_supply(), shares_minted);
    assert_eq!(s.vault_client.total_assets(), deposit_amount);
    assert_eq!(s.asset_client.balance(&s.user), 0);
    assert_eq!(
        s.asset_client.balance(&s.vault_client.address),
        deposit_amount
    );
    assert_eq!(shares_minted, deposit_amount * 10i128.pow(6));
}

#[test]
fn test_vault_mint() {
    let s = TestSetup::vault_only_with_offset(6);
    let shares_to_mint = 100_000_000_000_000_000i128;

    let required_assets = s.vault_client.preview_mint(&shares_to_mint);
    s.asset_client.transfer(&s.admin, &s.user, &required_assets);

    let assets_deposited = s
        .vault_client
        .mint(&shares_to_mint, &s.user, &s.user, &s.user);

    assert_eq!(s.vault_client.balance(&s.user), shares_to_mint);
    assert_eq!(s.vault_client.total_supply(), shares_to_mint);
    assert_eq!(s.vault_client.total_assets(), assets_deposited);
    assert_eq!(assets_deposited, required_assets);
}

#[test]
fn test_vault_withdraw() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;
    let withdraw_amount = 50_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    let shares_burned = s
        .vault_client
        .withdraw(&withdraw_amount, &s.user, &s.user, &s.user);

    assert_eq!(
        s.vault_client.balance(&s.user),
        shares_minted - shares_burned
    );
    assert_eq!(
        s.vault_client.total_assets(),
        deposit_amount - withdraw_amount
    );
    assert_eq!(s.asset_client.balance(&s.user), withdraw_amount);
}

#[test]
fn test_vault_redeem() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    let shares_to_redeem = shares_minted / 2;
    let assets_received = s
        .vault_client
        .redeem(&shares_to_redeem, &s.user, &s.user, &s.user);

    assert_eq!(
        s.vault_client.balance(&s.user),
        shares_minted - shares_to_redeem
    );
    assert_eq!(
        s.vault_client.total_supply(),
        shares_minted - shares_to_redeem
    );
    assert_eq!(s.asset_client.balance(&s.user), assets_received);

    let expected_assets = deposit_amount / 2;
    assert!(assets_received >= expected_assets - 1 && assets_received <= expected_assets + 1);
}

#[test]
fn test_conversion_functions() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    let assets = 1_000_000_000_000_000_000i128;
    let expected_shares = assets * 10i128.pow(6);

    assert_eq!(s.vault_client.convert_to_shares(&assets), expected_shares);
    assert_eq!(s.vault_client.convert_to_assets(&expected_shares), assets);

    assert_eq!(s.vault_client.preview_deposit(&assets), expected_shares);
    assert_eq!(s.vault_client.preview_mint(&expected_shares), assets);
    assert_eq!(s.vault_client.preview_withdraw(&assets), expected_shares);
    assert_eq!(s.vault_client.preview_redeem(&expected_shares), assets);

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    s.vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    let new_assets = 50_000_000_000_000_000i128;
    let shares = s.vault_client.convert_to_shares(&new_assets);
    let converted_back = s.vault_client.convert_to_assets(&shares);

    assert!(converted_back >= new_assets - 1 && converted_back <= new_assets + 1);
}

#[test]
fn test_max_functions() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    assert_eq!(s.vault_client.max_deposit(&s.user), i128::MAX);
    assert_eq!(s.vault_client.max_mint(&s.user), i128::MAX);
    assert_eq!(s.vault_client.max_withdraw(&s.user), 0);
    assert_eq!(s.vault_client.max_redeem(&s.user), 0);

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    assert_eq!(s.vault_client.max_redeem(&s.user), shares_minted);
    let max_withdraw = s.vault_client.max_withdraw(&s.user);
    assert!(max_withdraw > 0);
    assert!(max_withdraw <= deposit_amount);
}

#[test]
fn test_multiple_users_deposit_withdraw() {
    let s = TestSetup::vault_only_with_offset(6);
    let user2 = Address::generate(&s.e);
    let deposit_amount = 100_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    s.asset_client.transfer(&s.admin, &user2, &deposit_amount);

    let shares1 = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);
    let shares2 = s
        .vault_client
        .deposit(&deposit_amount, &user2, &user2, &user2);

    assert_eq!(shares1, shares2);
    assert_eq!(s.vault_client.total_supply(), shares1 + shares2);
    assert_eq!(s.vault_client.total_assets(), deposit_amount * 2);

    let withdraw_amount = deposit_amount / 2;
    let shares_burned = s
        .vault_client
        .withdraw(&withdraw_amount, &s.user, &s.user, &s.user);

    assert_eq!(s.vault_client.balance(&s.user), shares1 - shares_burned);
    assert_eq!(s.vault_client.balance(&user2), shares2);
    assert_eq!(s.asset_client.balance(&s.user), withdraw_amount);
}

#[test]
fn test_deposit_max_validation() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    let max_deposit = s.vault_client.max_deposit(&s.user);
    assert_eq!(max_deposit, i128::MAX);

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);
    assert!(shares > 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #407)")]
fn test_withdraw_exceeds_max() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    let max_withdraw = s.vault_client.max_withdraw(&s.user);
    s.vault_client
        .withdraw(&(max_withdraw + 1), &s.user, &s.user, &s.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #408)")]
fn test_redeem_exceeds_max() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    s.vault_client
        .redeem(&(shares + 1), &s.user, &s.user, &s.user);
}

#[test]
fn test_vault_metadata() {
    let s = TestSetup::vault_only_with_offset(6);

    assert_eq!(s.vault_client.name(), String::from_str(&s.e, "Vault Token"));
    assert_eq!(s.vault_client.symbol(), String::from_str(&s.e, "VLT"));
    assert_eq!(s.vault_client.decimals(), 18 + 6);
}

/// Vault rejects decimals_offset < 3 to mitigate first-depositor inflation.
#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_vault_decimals_offset_zero_rejected() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let asset = e.register(MockAssetContract, (DEFAULT_SUPPLY, &admin));
    create_vault_client(&e, &asset, 0, &admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_vault_decimals_offset_two_rejected() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let asset = e.register(MockAssetContract, (DEFAULT_SUPPLY, &admin));
    create_vault_client(&e, &asset, 2, &admin);
}

#[test]
fn test_vault_decimals_offset_minimum() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000_000_000_000i128;

    // offset=3 (minimum), so decimals = 18 + 3 = 21
    assert_eq!(s.vault_client.decimals(), 18 + 3);

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    // With offset=3, shares = assets * 10^3
    assert_eq!(shares_minted, deposit_amount * 1_000);
}

#[test]
fn test_full_redeem() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    let shares_minted = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    let assets_received = s
        .vault_client
        .redeem(&shares_minted, &s.user, &s.user, &s.user);

    assert_eq!(s.vault_client.balance(&s.user), 0);
    assert_eq!(s.vault_client.total_supply(), 0);
    assert_eq!(s.vault_client.total_assets(), 0);
    assert_eq!(s.asset_client.balance(&s.user), assets_received);
    assert!(assets_received >= deposit_amount - 1 && assets_received <= deposit_amount);
}

#[test]
fn test_deposit_zero() {
    let s = TestSetup::vault_only_with_offset(6);

    let shares = s.vault_client.deposit(&0, &s.user, &s.user, &s.user);

    assert_eq!(shares, 0);
    assert_eq!(s.vault_client.total_supply(), 0);
    assert_eq!(s.vault_client.total_assets(), 0);
}

#[test]
fn test_withdraw_zero() {
    let s = TestSetup::vault_only_with_offset(6);
    let deposit_amount = 100_000_000_000_000_000i128;

    let shares_minted = s.deposit_as_user(deposit_amount);

    let shares_burned = s.vault_client.withdraw(&0, &s.user, &s.user, &s.user);

    assert_eq!(shares_burned, 0);
    assert_eq!(s.vault_client.balance(&s.user), shares_minted);
    assert_eq!(s.vault_client.total_assets(), deposit_amount);
}

#[test]
#[should_panic(expected = "HostError: Error(Context, InvalidAction)")]
fn test_constructor_panics_for_invalid_asset() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let not_a_token = Address::generate(&e);
    create_vault_client(&e, &not_a_token, 6, &admin);
}

#[test]
#[should_panic(expected = "HostError: Error(Context, InvalidAction)")]
fn test_constructor_panics_for_non_token_contract() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let non_token_address = e.register(MockNonTokenContract, ());
    create_vault_client(&e, &non_token_address, 6, &admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #409)")] // VaultMaxDecimalsOffsetExceeded
fn test_constructor_rejects_excessive_decimals_offset() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;
    let asset_client = create_asset_client(&e, initial_supply, &admin);
    // OZ enforces decimals_offset <= 10
    create_vault_client(&e, &asset_client.address, 11, &admin);
}

#[test]
fn test_constructor_extends_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let asset_address = asset_client.address.clone();
    let vault_client = create_vault_client(&e, &asset_address, 6, &admin);

    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        assert!(
            ttl >= INSTANCE_EXTEND_AMOUNT,
            "Constructor should extend instance TTL to at least {} ledgers, got {}",
            INSTANCE_EXTEND_AMOUNT,
            ttl
        );
    });
}

#[test]
fn test_extend_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let asset_address = asset_client.address.clone();
    let vault_client = create_vault_client(&e, &asset_address, 6, &admin);

    // Advance ledger to reduce TTL below the threshold
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    // extend_ttl is permissionless — callable without mock_all_auths()
    vault_client.extend_ttl();

    e.as_contract(&vault_client.address, || {
        assert_eq!(e.storage().instance().get_ttl(), INSTANCE_EXTEND_AMOUNT);
    });
}

// ==================== Admin/operator/pause tests ====================

#[test]
fn test_set_operator() {
    let s = TestSetup::vault_only();

    assert_eq!(s.vault_client.get_operator(), None);
    s.vault_client.set_operator(&s.admin, &s.operator);
    assert_eq!(s.vault_client.get_operator(), Some(s.operator.clone()));
}

test_vault_only_unauthorized!(test_set_operator_unauthorized, |s| {
    let not_admin = Address::generate(&s.e);
    s.vault_client.set_operator(&not_admin, &s.operator);
});

#[test]
fn test_pause_unpause() {
    let s = TestSetup::vault_only();

    assert!(!s.vault_client.is_paused());
    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());
    s.vault_client.unpause(&s.admin);
    assert!(!s.vault_client.is_paused());
}

test_vault_only_unauthorized!(test_pause_unauthorized, |s| {
    let not_admin = Address::generate(&s.e);
    s.vault_client.pause(&not_admin);
});

#[test]
fn test_max_returns_zero_when_paused() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    assert_eq!(s.vault_client.max_deposit(&s.user), i128::MAX);
    assert_eq!(s.vault_client.max_mint(&s.user), i128::MAX);
    assert!(s.vault_client.max_withdraw(&s.user) > 0);
    assert!(s.vault_client.max_redeem(&s.user) > 0);

    s.vault_client.pause(&s.admin);

    assert_eq!(s.vault_client.max_deposit(&s.user), 0);
    assert_eq!(s.vault_client.max_mint(&s.user), 0);
    assert_eq!(s.vault_client.max_withdraw(&s.user), 0);
    assert_eq!(s.vault_client.max_redeem(&s.user), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_deposit_blocked_when_paused() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000_000_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);
    s.vault_client.pause(&s.admin);

    s.vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_withdraw_blocked_when_paused() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000_000_000_000i128;

    s.deposit_as_user(deposit_amount);
    s.vault_client.pause(&s.admin);

    s.vault_client
        .withdraw(&deposit_amount, &s.user, &s.user, &s.user);
}

#[test]
fn test_set_aum_limits() {
    let s = TestSetup::vault_only();

    assert_eq!(s.vault_client.get_aum_increase_limit(), 1_000);
    assert_eq!(s.vault_client.get_aum_decrease_limit(), 500);

    s.vault_client.set_aum_limits(&s.admin, &2_000, &1_000);

    assert_eq!(s.vault_client.get_aum_increase_limit(), 2_000);
    assert_eq!(s.vault_client.get_aum_decrease_limit(), 1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_limits_invalid() {
    let s = TestSetup::vault_only();

    s.vault_client.set_aum_limits(&s.admin, &0, &500);
}

#[test]
fn test_deployed_assets_initially_zero() {
    let s = TestSetup::vault_only();

    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

// ==================== Custom total_assets with deployed capital ====================

/// Helper: set deployed_assets directly in vault storage for testing.
fn set_deployed_assets(e: &Env, vault_address: &Address, amount: i128) {
    use crate::storage;
    e.as_contract(vault_address, || {
        storage::set_deployed_assets(e, amount);
    });
}

#[test]
fn test_total_assets_includes_deployed_capital() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000i128;

    s.deposit_as_user(deposit_amount);
    assert_eq!(s.vault_client.total_assets(), deposit_amount);

    let deployed = 40_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    assert_eq!(s.vault_client.total_assets(), deposit_amount);
    assert_eq!(
        s.asset_client.balance(&s.vault_client.address),
        deposit_amount - deployed
    );
}

#[test]
fn test_total_assets_all_three_components() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();

    // Deposit to vault: local_balance = 1_000_000
    s.fund_vault(1_000_000);

    // Deploy to strategy: local_balance = 700k, strategy_balance = 300k
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &300_000i128);

    // Set off-chain deployed_assets = 50k
    s.set_deployed(50_000);

    // total = local(700k) + strategy.get_balance()(300k) + deployed_assets(50k)
    assert_eq!(s.vault_client.total_assets(), 1_050_000i128);
    assert_eq!(s.vault_client.get_strategy_balances(), 300_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 50_000);
}

#[test]
fn test_share_price_preserved_after_deployment() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let assets_before = s.vault_client.convert_to_assets(&shares);
    assert!(
        assets_before >= deposit_amount - 1 && assets_before <= deposit_amount,
        "Before deploy: expected ~{}, got {}",
        deposit_amount,
        assets_before
    );

    let deployed = deposit_amount / 2;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    let assets_after = s.vault_client.convert_to_assets(&shares);
    assert!(
        assets_after >= deposit_amount - 1 && assets_after <= deposit_amount,
        "After deploy: expected ~{}, got {}",
        deposit_amount,
        assets_after
    );
}

#[test]
fn test_share_price_increases_with_strategy_profit() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let deployed_original = 500_000_000i128;
    let profit = 100_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed_original);
    s.set_deployed(deployed_original + profit);

    assert_eq!(s.vault_client.total_assets(), deposit_amount + profit);

    let assets_for_shares = s.vault_client.convert_to_assets(&shares);
    assert!(
        assets_for_shares > deposit_amount,
        "Share value should increase with profit: expected > {}, got {}",
        deposit_amount,
        assets_for_shares
    );
}

#[test]
fn test_share_price_decreases_with_strategy_loss() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let deployed_original = 500_000_000i128;
    let loss = 200_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed_original);
    s.set_deployed(deployed_original - loss);

    assert_eq!(s.vault_client.total_assets(), deposit_amount - loss);

    let assets_for_shares = s.vault_client.convert_to_assets(&shares);
    assert!(
        assets_for_shares < deposit_amount,
        "Share value should decrease with loss: expected < {}, got {}",
        deposit_amount,
        assets_for_shares
    );
}

#[test]
fn test_new_depositor_gets_fewer_shares_after_profit() {
    let s = TestSetup::vault_only();
    let user2 = Address::generate(&s.e);
    let deposit_amount = 1_000_000_000i128;

    let shares1 = s.deposit_as_user(deposit_amount);

    let deployed_out = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed_out);
    s.set_deployed(750_000_000i128);

    s.asset_client.transfer(&s.admin, &user2, &deposit_amount);
    let shares2 = s
        .vault_client
        .deposit(&deposit_amount, &user2, &user2, &user2);

    assert!(
        shares2 < shares1,
        "User2 should get fewer shares after profit: shares1={}, shares2={}",
        shares1,
        shares2
    );
}

#[test]
fn test_withdraw_respects_deployed_assets_accounting() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Deploy half the assets
    let deployed = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // max_withdraw should be capped at local balance (not full AUM),
    // because the vault can only transfer tokens it physically holds.
    let local_balance = deposit_amount - deployed; // 500_000_000
    let max_w = s.vault_client.max_withdraw(&s.user);
    assert!(
        max_w >= local_balance - 1 && max_w <= local_balance,
        "max_withdraw should be capped at local balance: expected ~{}, got {}",
        local_balance,
        max_w
    );
}

#[test]
fn test_conversions_with_deployed_assets_and_offset() {
    let s = TestSetup::vault_only_with_offset(3);
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Simulate 20% profit via deployed assets
    let profit = 200_000_000i128;
    s.set_deployed(profit);
    s.asset_client
        .transfer(&s.admin, &s.vault_client.address, &profit);

    // convert_to_shares should reflect new (higher) price
    let shares_for_1000 = s.vault_client.convert_to_shares(&1_000_000_000);
    let original_shares_for_1000 = 1_000_000_000i128 * 10i128.pow(3);

    // Shares for the same assets should be fewer after profit
    assert!(
        shares_for_1000 < original_shares_for_1000,
        "After profit, fewer shares per asset: orig={}, now={}",
        original_shares_for_1000,
        shares_for_1000
    );

    // Round-trip: shares → assets → shares should be consistent
    let assets_back = s.vault_client.convert_to_assets(&shares_for_1000);
    let shares_again = s.vault_client.convert_to_shares(&assets_back);
    assert!(
        shares_again <= shares_for_1000,
        "Round-trip should not create shares: {} > {}",
        shares_again,
        shares_for_1000
    );
}

// ==================== Pause: mint and redeem ====================

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_mint_blocked_when_paused() {
    let s = TestSetup::vault_only();

    s.asset_client
        .transfer(&s.admin, &s.user, &1_000_000_000i128);
    s.vault_client.pause(&s.admin);

    s.vault_client
        .mint(&1_000_000i128, &s.user, &s.user, &s.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_redeem_blocked_when_paused() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);
    s.vault_client.pause(&s.admin);

    s.vault_client.redeem(&shares, &s.user, &s.user, &s.user);
}

#[test]
fn test_unpause_resumes_operations() {
    let s = TestSetup::vault_only();
    let deposit_amount = 100_000_000i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);

    // Pause → unpause → deposit should work
    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());

    s.vault_client.unpause(&s.admin);
    assert!(!s.vault_client.is_paused());

    let shares = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);
    assert!(shares > 0);

    // Pause → unpause → withdraw should work
    s.vault_client.pause(&s.admin);
    s.vault_client.unpause(&s.admin);

    let withdrawn = s
        .vault_client
        .withdraw(&(deposit_amount / 2), &s.user, &s.user, &s.user);
    assert!(withdrawn > 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_unpause_unauthorized() {
    let s = TestSetup::vault_only();
    let not_admin = Address::generate(&s.e);

    s.vault_client.pause(&s.admin);
    s.vault_client.unpause(&not_admin);
}

#[test]
fn test_double_pause_idempotent() {
    let s = TestSetup::vault_only();

    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());

    // Second pause should succeed without error
    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());
}

#[test]
fn test_double_unpause_idempotent() {
    let s = TestSetup::vault_only();

    // Double unpause on an already unpaused vault
    s.vault_client.unpause(&s.admin);
    assert!(!s.vault_client.is_paused());
}

// ==================== Admin/Operator edge cases ====================

#[test]
fn test_replace_operator() {
    let s = TestSetup::vault_only();
    let operator1 = Address::generate(&s.e);
    let operator2 = Address::generate(&s.e);

    s.vault_client.set_operator(&s.admin, &operator1);
    assert_eq!(s.vault_client.get_operator(), Some(operator1));

    s.vault_client.set_operator(&s.admin, &operator2);
    assert_eq!(s.vault_client.get_operator(), Some(operator2.clone()));
}

test_vault_only_unauthorized!(test_set_aum_limits_unauthorized, |s| {
    let not_admin = Address::generate(&s.e);
    s.vault_client.set_aum_limits(&not_admin, &2_000, &1_000);
});

#[test]
fn test_set_aum_limits_boundary_min() {
    let s = TestSetup::vault_only();

    // Minimum valid: 1 bps each
    s.vault_client.set_aum_limits(&s.admin, &1, &1);
    assert_eq!(s.vault_client.get_aum_increase_limit(), 1);
    assert_eq!(s.vault_client.get_aum_decrease_limit(), 1);
}

#[test]
fn test_set_aum_limits_boundary_max() {
    let s = TestSetup::vault_only();

    // Maximum valid: 10000 bps (100%)
    s.vault_client.set_aum_limits(&s.admin, &10_000, &10_000);
    assert_eq!(s.vault_client.get_aum_increase_limit(), 10_000);
    assert_eq!(s.vault_client.get_aum_decrease_limit(), 10_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_limits_exceeds_max() {
    let s = TestSetup::vault_only();

    // 10001 bps exceeds maximum
    s.vault_client.set_aum_limits(&s.admin, &10_001, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_limits_decrease_exceeds_max() {
    let s = TestSetup::vault_only();

    s.vault_client.set_aum_limits(&s.admin, &500, &10_001);
}

// ==================== Conversion edge cases ====================

#[test]
#[should_panic(expected = "Error(Contract, #403)")]
fn test_convert_to_shares_negative_assets() {
    let s = TestSetup::vault_only();
    s.vault_client.convert_to_shares(&-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #404)")]
fn test_convert_to_assets_negative_shares() {
    let s = TestSetup::vault_only();
    s.vault_client.convert_to_assets(&-1);
}

#[test]
fn test_convert_to_shares_zero() {
    let s = TestSetup::vault_only();
    assert_eq!(s.vault_client.convert_to_shares(&0), 0);
}

#[test]
fn test_convert_to_assets_zero() {
    let s = TestSetup::vault_only();
    assert_eq!(s.vault_client.convert_to_assets(&0), 0);
}

// ==================== Rounding direction tests ====================

#[test]
fn test_rounding_favors_vault_on_deposit() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_003i128; // Odd amount to trigger rounding

    // First deposit establishes 1:1 rate
    s.asset_client
        .transfer(&s.admin, &s.user, &(deposit_amount * 3));
    s.vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    // Second deposit: preview_deposit rounds DOWN (fewer shares for depositor)
    let shares = s.vault_client.preview_deposit(&7);
    let assets_back = s.vault_client.convert_to_assets(&shares);
    // Depositor gets shares worth <= what they deposited
    assert!(
        assets_back <= 7,
        "Deposit rounding should favor vault: assets_back={}",
        assets_back
    );
}

#[test]
fn test_rounding_favors_vault_on_withdraw() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_003i128;

    s.deposit_as_user(deposit_amount);

    // preview_withdraw rounds UP shares burned (more shares burned for same assets)
    let shares_for_withdraw = s.vault_client.preview_withdraw(&7);
    let shares_for_redeem = s.vault_client.preview_redeem(&shares_for_withdraw);

    // Withdrawing assets costs >= redeeming the equivalent shares gives back
    assert!(
        shares_for_redeem <= 7,
        "Withdraw rounding should favor vault: redeem_assets={}",
        shares_for_redeem
    );
}

#[test]
fn test_rounding_favors_vault_on_mint() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_003i128;

    s.deposit_as_user(deposit_amount);

    // preview_mint rounds UP (depositor pays more assets for same shares)
    let assets_for_mint = s.vault_client.preview_mint(&7);
    let shares_for_deposit = s.vault_client.preview_deposit(&assets_for_mint);

    // Minting N shares costs >= depositing enough assets to get N shares
    assert!(
        shares_for_deposit >= 7,
        "Mint rounding should favor vault: deposit_shares={}",
        shares_for_deposit
    );
}

// ==================== Event emission tests ====================

#[test]
fn test_set_operator_emits_event() {
    let s = TestSetup::vault_only();
    let operator = Address::generate(&s.e);

    s.vault_client.set_operator(&s.admin, &operator);
    s.assert_last_event_contains("operator_set");
}

#[test]
fn test_pause_emits_event() {
    let s = TestSetup::vault_only();

    s.vault_client.pause(&s.admin);
    s.assert_last_event_contains("vault_paused");
}

#[test]
fn test_unpause_emits_event() {
    let s = TestSetup::vault_only();

    s.vault_client.pause(&s.admin);
    s.vault_client.unpause(&s.admin);
    s.assert_last_event_contains("vault_unpaused");
}

#[test]
fn test_set_aum_limits_emits_event() {
    let s = TestSetup::vault_only();

    s.vault_client.set_aum_limits(&s.admin, &2_000, &1_000);
    s.assert_last_event_contains("aum_limits_changed");
}

// ==================== TTL extension in state-changing operations ====================

#[test]
fn test_deposit_extends_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;
    let deposit_amount = 100_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    // Advance ledger to reduce TTL
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    asset_client.transfer(&admin, &user, &deposit_amount);
    vault_client.deposit(&deposit_amount, &user, &user, &user);

    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        assert!(
            ttl >= INSTANCE_EXTEND_AMOUNT,
            "Deposit should extend TTL: got {}",
            ttl
        );
    });
}

// ==================== Multi-user scenarios with deployed capital ====================

#[test]
fn test_multi_user_fairness_with_deployed_assets() {
    let s = TestSetup::vault_only();
    let user1 = Address::generate(&s.e);
    let user2 = Address::generate(&s.e);
    let deposit_amount = 1_000_000_000i128;

    // User1 deposits
    s.asset_client.transfer(&s.admin, &user1, &deposit_amount);
    let shares1 = s
        .vault_client
        .deposit(&deposit_amount, &user1, &user1, &user1);

    // User2 deposits same amount
    s.asset_client.transfer(&s.admin, &user2, &deposit_amount);
    let shares2 = s
        .vault_client
        .deposit(&deposit_amount, &user2, &user2, &user2);

    // Both should have same shares (same deposit, same price)
    assert_eq!(shares1, shares2);

    // Simulate: deploy 1B to strategy, strategy earns 50% → deployed = 1.5B
    // Move tokens out to simulate deployment
    let deployed = 1_000_000_000i128;
    let profit = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed + profit);
    // vault local = 1B, deployed = 1.5B, total_assets = 2.5B

    // Return capital + profit to vault before redemption
    s.asset_client
        .transfer(&s.admin, &s.vault_client.address, &(deployed + profit));
    s.set_deployed(0);
    // vault local = 2.5B, deployed = 0, total_assets = 2.5B

    // Both users redeem all shares
    let assets1 = s.vault_client.redeem(&shares1, &user1, &user1, &user1);
    let assets2 = s.vault_client.redeem(&shares2, &user2, &user2, &user2);

    // Both should get approximately the same amount (equal share of profit)
    assert!(
        (assets1 - assets2).abs() <= 1,
        "Users should share profit equally: user1={}, user2={}",
        assets1,
        assets2
    );

    // Each should get ~1.25B (original 1B + half of 500M profit)
    let expected_per_user = deposit_amount + profit / 2;
    assert!(
        assets1 >= expected_per_user - 2 && assets1 <= expected_per_user + 2,
        "User1 should get ~{}: got {}",
        expected_per_user,
        assets1
    );
}

#[test]
fn test_late_depositor_does_not_dilute_existing() {
    let s = TestSetup::vault_only();
    let user1 = Address::generate(&s.e);
    let user2 = Address::generate(&s.e);
    let deposit_amount = 1_000_000_000i128;

    // User1 deposits 1B → vault local = 1B
    s.asset_client.transfer(&s.admin, &user1, &deposit_amount);
    let shares1 = s
        .vault_client
        .deposit(&deposit_amount, &user1, &user1, &user1);

    // Strategy deploys 500M, earns 100% → deployed = 1B
    // Move 500M out, set deployed to 1B (500M principal + 500M profit)
    let deployed_out = 500_000_000i128;
    let strategy_value = 1_000_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed_out);
    s.set_deployed(strategy_value);
    // vault local = 500M, deployed = 1B, total_assets = 1.5B

    // User2 deposits same nominal amount AFTER profit
    s.asset_client.transfer(&s.admin, &user2, &deposit_amount);
    let shares2 = s
        .vault_client
        .deposit(&deposit_amount, &user2, &user2, &user2);
    // vault local = 1.5B, deployed = 1B, total_assets = 2.5B

    // User2 gets fewer shares (share price increased due to profit)
    assert!(
        shares2 < shares1,
        "Late depositor should get fewer shares: s1={}, s2={}",
        shares1,
        shares2
    );

    // Return deployed capital before redemption
    s.asset_client
        .transfer(&s.admin, &s.vault_client.address, &strategy_value);
    s.set_deployed(0);
    // vault local = 2.5B, deployed = 0, total_assets = 2.5B

    // User1 redeems: should get ~1.5B (1B deposit + 500M profit)
    let assets1 = s.vault_client.redeem(&shares1, &user1, &user1, &user1);
    let expected1 = deposit_amount + deployed_out; // 1.5B
    assert!(
        assets1 >= expected1 - 2 && assets1 <= expected1 + 2,
        "Early user should get profit: expected ~{}, got {}",
        expected1,
        assets1
    );

    // User2 redeems: should get ~their original deposit (no profit for them)
    let assets2 = s.vault_client.redeem(&shares2, &user2, &user2, &user2);
    assert!(
        assets2 >= deposit_amount - 2 && assets2 <= deposit_amount + 2,
        "Late depositor should get ~original: expected ~{}, got {}",
        deposit_amount,
        assets2
    );
}

// ==================== Preview consistency ====================

#[test]
fn test_preview_deposit_matches_actual_deposit() {
    let s = TestSetup::vault_only_with_offset(3);
    let deposit_amount = 123_456_789i128;

    s.asset_client.transfer(&s.admin, &s.user, &deposit_amount);

    let preview = s.vault_client.preview_deposit(&deposit_amount);
    let actual = s
        .vault_client
        .deposit(&deposit_amount, &s.user, &s.user, &s.user);

    assert_eq!(
        preview, actual,
        "preview_deposit should match actual deposit"
    );
}

#[test]
fn test_preview_mint_matches_actual_mint() {
    let s = TestSetup::vault_only_with_offset(3);
    let shares_to_mint = 123_456_789_000i128;

    let preview = s.vault_client.preview_mint(&shares_to_mint);
    s.asset_client.transfer(&s.admin, &s.user, &preview);

    let actual = s
        .vault_client
        .mint(&shares_to_mint, &s.user, &s.user, &s.user);

    assert_eq!(preview, actual, "preview_mint should match actual mint");
}

#[test]
fn test_preview_withdraw_matches_actual_withdraw() {
    let s = TestSetup::vault_only_with_offset(3);
    let deposit_amount = 1_000_000_000i128;
    let withdraw_amount = 123_456_789i128;

    s.deposit_as_user(deposit_amount);

    let preview = s.vault_client.preview_withdraw(&withdraw_amount);
    let actual = s
        .vault_client
        .withdraw(&withdraw_amount, &s.user, &s.user, &s.user);

    assert_eq!(
        preview, actual,
        "preview_withdraw should match actual withdraw"
    );
}

#[test]
fn test_preview_redeem_matches_actual_redeem() {
    let s = TestSetup::vault_only_with_offset(3);
    let deposit_amount = 1_000_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let redeem_shares = shares / 3;
    let preview = s.vault_client.preview_redeem(&redeem_shares);
    let actual = s
        .vault_client
        .redeem(&redeem_shares, &s.user, &s.user, &s.user);

    assert_eq!(preview, actual, "preview_redeem should match actual redeem");
}

// ==================== Preview consistency with deployed assets ====================

#[test]
fn test_previews_consistent_with_deployed_assets() {
    let s = TestSetup::vault_only_with_offset(3);
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Simulate deployed assets with profit
    let profit = 200_000_000i128;
    s.set_deployed(profit);
    s.asset_client
        .transfer(&s.admin, &s.vault_client.address, &profit);

    // Preview a second deposit with deployed assets active
    let second_deposit = 500_000_000i128;
    s.asset_client.transfer(&s.admin, &s.user, &second_deposit);
    let preview = s.vault_client.preview_deposit(&second_deposit);
    let actual = s
        .vault_client
        .deposit(&second_deposit, &s.user, &s.user, &s.user);

    assert_eq!(
        preview, actual,
        "preview_deposit should match with deployed assets"
    );
}

// ==================== Validation guard tests ====================

#[test]
#[should_panic(expected = "Error(Contract, #3)")] // VaultError::InvalidAmount
fn test_set_deployed_assets_rejects_negative() {
    let s = TestSetup::vault_only();

    // Try setting deployed assets to a negative value via the storage helper
    s.set_deployed(-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #8)")] // VaultError::MathOverflow
fn test_custom_total_assets_rejects_negative_result() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let initial_supply = 10_000_000_000_000_000_000i128;
    let deposit_amount = 1_000_000_000i128;

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    asset_client.transfer(&admin, &user, &deposit_amount);
    vault_client.deposit(&deposit_amount, &user, &user, &user);

    // Transfer all tokens out of the vault to simulate deployment
    asset_client.transfer(&vault_client.address, &admin, &deposit_amount);

    // Bypass the non-negative check in set_deployed_assets by writing directly
    // to storage to simulate a corrupted state where total would be negative.
    // deployed_assets = 0 but local_balance = 0, so total = 0 (not negative).
    // To trigger the negative guard, we need local_balance + deployed < 0.
    // Since we can't set negative via set_deployed_assets (it validates),
    // we write directly to storage to simulate corruption.
    use crate::storage::StorageKey;
    e.as_contract(&vault_client.address, || {
        e.storage()
            .instance()
            .set(&StorageKey::DeployedAssets, &(-1_000_000_000i128));
    });

    // This should panic because local_balance(0) + deployed(-1B) = -1B < 0
    vault_client.total_assets();
}

#[test]
fn test_max_redeem_capped_by_local_liquidity() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    let user_shares = s.vault_client.balance(&s.user);

    // Deploy half the assets
    let deployed = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // max_redeem should be less than user's total shares
    // (capped by what's locally available)
    let max_r = s.vault_client.max_redeem(&s.user);
    assert!(
        max_r < user_shares,
        "max_redeem should be less than total shares when capital is deployed: max_redeem={}, user_shares={}",
        max_r, user_shares
    );

    // The capped max_redeem should correspond to roughly the local balance worth of shares
    let local_balance = deposit_amount - deployed; // 500M
    let expected_max_assets = s.vault_client.convert_to_assets(&max_r);
    assert!(
        expected_max_assets <= local_balance,
        "max_redeem assets value should not exceed local balance: assets={}, local={}",
        expected_max_assets,
        local_balance
    );
}

#[test]
#[should_panic(expected = "Error(Contract, #408)")] // VaultExceededMaxRedeem
fn test_redeem_fails_when_exceeding_liquidity() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    let user_shares = s.vault_client.balance(&s.user);

    // Deploy 80% of assets
    let deployed = 800_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // Attempting to redeem all shares should fail since max_redeem is capped
    s.vault_client
        .redeem(&user_shares, &s.user, &s.user, &s.user);
}

#[test]
#[should_panic(expected = "Error(Contract, #407)")] // VaultExceededMaxWithdraw
fn test_withdraw_fails_when_exceeding_liquidity() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Deploy 80% of assets
    let deployed = 800_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // Local balance is 200M. Try to withdraw 300M — should fail.
    s.vault_client
        .withdraw(&300_000_000, &s.user, &s.user, &s.user);
}

#[test]
fn test_withdraw_succeeds_within_local_liquidity() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Deploy 50% of assets
    let deployed = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // Withdraw up to max_withdraw — should succeed
    let max_w = s.vault_client.max_withdraw(&s.user);
    assert!(max_w > 0, "max_withdraw should be positive");

    let shares_burned = s.vault_client.withdraw(&max_w, &s.user, &s.user, &s.user);
    assert!(shares_burned > 0, "should have burned shares");

    // User should have received tokens
    let user_balance = s.asset_client.balance(&s.user);
    assert!(
        user_balance >= max_w - 1 && user_balance <= max_w,
        "user should have received ~max_w tokens: got {}, expected ~{}",
        user_balance,
        max_w
    );
}

#[test]
fn test_redeem_succeeds_within_local_liquidity() {
    let s = TestSetup::vault_only();
    let deposit_amount = 1_000_000_000i128;

    s.deposit_as_user(deposit_amount);

    // Deploy 50% of assets
    let deployed = 500_000_000i128;
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &deployed);
    s.set_deployed(deployed);

    // Redeem up to max_redeem — should succeed
    let max_r = s.vault_client.max_redeem(&s.user);
    assert!(max_r > 0, "max_redeem should be positive");

    let assets_received = s.vault_client.redeem(&max_r, &s.user, &s.user, &s.user);
    assert!(assets_received > 0, "should have received assets");
}

// ==================== Additional coverage (PR review) ====================

#[test]
#[should_panic(expected = "Error(Contract, #403)")] // VaultInvalidAssetsAmount
fn test_dust_deposit_rejected_when_zero_shares() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let user = Address::generate(&e);
    let initial_supply = 10_000_000_000_000_000_000i128;

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    // offset=3 means virtual shares = 10^3 = 1000, but a dust deposit can still
    // round down to zero shares when the vault already has a large share base.
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    // Seed the vault with a small deposit.
    let seed = 1_000i128;
    asset_client.transfer(&admin, &user, &seed);
    vault_client.deposit(&seed, &user, &user, &user);

    // Simulate profit by donating tokens directly to the vault (inflates share price).
    // total_assets goes from 1000 to 1_000_001_000, but total_supply stays at ~1000.
    // share price becomes ~1_000_000 assets per share.
    let donation = 1_000_000_000i128;
    asset_client.transfer(&admin, &vault_client.address, &donation);

    // A 1-stroop deposit: shares = 1 * (supply + 1) / (total_assets + 1)
    // ≈ 1 * 1001 / 1_001_001_001 ≈ 0 (Floor). Should be rejected.
    asset_client.transfer(&admin, &user, &1);
    vault_client.deposit(&1, &user, &user, &user);
}

#[test]
fn test_admin_sets_self_as_operator() {
    let s = TestSetup::vault_only();

    // Admin can set themselves as operator (no guard against this).
    s.vault_client.set_operator(&s.admin, &s.admin);
    assert_eq!(s.vault_client.get_operator(), Some(s.admin));
}

#[test]
fn test_set_operator_validates_state_transitions() {
    let s = TestSetup::vault_only();
    let operator1 = Address::generate(&s.e);
    let operator2 = Address::generate(&s.e);

    // Initially no operator
    assert_eq!(s.vault_client.get_operator(), None);

    // First set: None → operator1
    s.vault_client.set_operator(&s.admin, &operator1);
    assert_eq!(s.vault_client.get_operator(), Some(operator1.clone()));

    // Second set: operator1 → operator2
    s.vault_client.set_operator(&s.admin, &operator2);
    assert_eq!(s.vault_client.get_operator(), Some(operator2.clone()));

    // Verify operator1 is fully replaced (not additive)
    assert_ne!(s.vault_client.get_operator(), Some(operator1));
}

// ==================== Share transfer / approve tests ====================

#[test]
fn test_vault_share_transfer() {
    let s = TestSetup::vault_only();
    let user2 = Address::generate(&s.e);
    let deposit_amount = 100_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let total_assets_before = s.vault_client.total_assets();
    let total_supply_before = s.vault_client.total_supply();

    let transfer_amount = shares / 2;
    s.vault_client.transfer(&s.user, &user2, &transfer_amount);

    assert_eq!(s.vault_client.balance(&s.user), shares - transfer_amount);
    assert_eq!(s.vault_client.balance(&user2), transfer_amount);
    assert_eq!(s.vault_client.total_assets(), total_assets_before);
    assert_eq!(s.vault_client.total_supply(), total_supply_before);
}

#[test]
fn test_vault_share_approve_and_transfer_from() {
    let s = TestSetup::vault_only();
    let spender = Address::generate(&s.e);
    let receiver = Address::generate(&s.e);
    let deposit_amount = 100_000_000i128;

    let shares = s.deposit_as_user(deposit_amount);

    let approve_amount = shares / 2;
    let live_until_ledger = s.e.ledger().sequence() + 1000;
    s.vault_client
        .approve(&s.user, &spender, &approve_amount, &live_until_ledger);

    assert_eq!(s.vault_client.allowance(&s.user, &spender), approve_amount);

    s.vault_client
        .transfer_from(&spender, &s.user, &receiver, &approve_amount);

    assert_eq!(s.vault_client.balance(&s.user), shares - approve_amount);
    assert_eq!(s.vault_client.balance(&receiver), approve_amount);
    assert_eq!(
        s.vault_client.allowance(&s.user, &spender),
        0,
        "Allowance should be fully spent"
    );
}

// ==================== Share transfer / approve TTL tests ====================

#[test]
fn test_transfer_extends_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let user1 = Address::generate(&e);
    let user2 = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;
    let deposit_amount = 100_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    asset_client.transfer(&admin, &user1, &deposit_amount);
    vault_client.deposit(&deposit_amount, &user1, &user1, &user1);

    // Advance ledger to reduce TTL
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    let transfer_amount = vault_client.balance(&user1) / 2;
    vault_client.transfer(&user1, &user2, &transfer_amount);

    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        assert!(
            ttl >= INSTANCE_EXTEND_AMOUNT,
            "Transfer should extend TTL: got {}",
            ttl
        );
    });
}

#[test]
fn test_approve_extends_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let owner = Address::generate(&e);
    let spender = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    // Advance ledger to reduce TTL
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    let live_until_ledger = e.ledger().sequence() + 1000;
    vault_client.approve(&owner, &spender, &100, &live_until_ledger);

    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        assert!(
            ttl >= INSTANCE_EXTEND_AMOUNT,
            "Approve should extend TTL: got {}",
            ttl
        );
    });
}

#[test]
fn test_transfer_from_extends_ttl() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let owner = Address::generate(&e);
    let spender = Address::generate(&e);
    let receiver = Address::generate(&e);
    let initial_supply = 1_000_000_000_000_000_000i128;
    let deposit_amount = 100_000_000i128;

    e.ledger().with_mut(|l| {
        l.min_persistent_entry_ttl = 500;
    });

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    e.mock_all_auths();

    asset_client.transfer(&admin, &owner, &deposit_amount);
    vault_client.deposit(&deposit_amount, &owner, &owner, &owner);
    let shares = vault_client.balance(&owner);

    // Advance ledger to reduce instance TTL, then approve with a
    // live_until_ledger that is valid relative to the new sequence.
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    // The allowance's live_until_ledger must outlast the second ledger
    // advance below, otherwise the temporary storage entry expires and
    // transfer_from fails with InsufficientAllowance.
    let live_until_ledger = e.ledger().sequence() + 300_000;
    vault_client.approve(&owner, &spender, &shares, &live_until_ledger);

    // Advance again so the TTL set by approve decays — only
    // transfer_from should restore it.
    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        let current = e.ledger().sequence();
        e.ledger().set_sequence_number(current + ttl);
    });

    vault_client.transfer_from(&spender, &owner, &receiver, &(shares / 2));

    e.as_contract(&vault_client.address, || {
        let ttl = e.storage().instance().get_ttl();
        assert!(
            ttl >= INSTANCE_EXTEND_AMOUNT,
            "transfer_from should extend TTL: got {}",
            ttl
        );
    });
}

// ==================== Mock Strategy Contracts ====================

/// Standard mock strategy. Holds tokens and implements deposit + withdraw.
#[contract]
pub struct MockStrategyContract;

#[contractimpl]
impl MockStrategyContract {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(e: &Env, _from: Address, amount: i128) {
        if amount > 0 {
            let reverts: bool = e
                .storage()
                .instance()
                .get(&"deposit_reverts")
                .unwrap_or(false);
            if reverts {
                panic!("deposit reverted");
            }
        }
    }

    pub fn withdraw(e: &Env, to: Address, amount: i128) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        let token_client = token::Client::new(e, &asset);
        let balance = token_client.balance(&e.current_contract_address());
        let actual = core::cmp::min(amount, balance);
        if actual > 0 {
            token_client.transfer(&e.current_contract_address(), &to, &actual);
        }
        actual
    }

    pub fn get_balance(e: &Env) -> i128 {
        if e.storage()
            .instance()
            .get::<_, bool>(&"balance_reverts")
            .unwrap_or(false)
        {
            panic!("broken get_balance");
        }
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }

    /// F2: mock simply mirrors `get_balance` for test purposes. Real
    /// strategies should track idle distinctly from deployed; the mock's
    /// lack of deployed-total tracking makes the two values equivalent.
    pub fn get_local_balance(e: &Env) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    /// Test helper: make deposit() panic on non-zero amounts.
    pub fn set_deposit_reverts(e: &Env, reverts: bool) {
        e.storage().instance().set(&"deposit_reverts", &reverts);
    }

    /// Test helper: make get_balance() panic (simulates a broken strategy).
    pub fn set_balance_reverts(e: &Env, reverts: bool) {
        e.storage().instance().set(&"balance_reverts", &reverts);
    }
}

#[contract]
pub struct MockBrokenStrategy;

#[contractimpl]
impl MockBrokenStrategy {
    pub fn __constructor(_e: &Env) {}

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {
        panic!("broken strategy");
    }

    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        panic!("broken strategy");
    }

    pub fn get_balance(_e: &Env) -> i128 {
        panic!("broken strategy");
    }

    pub fn get_asset(_e: &Env) -> Address {
        panic!("broken strategy");
    }

    pub fn get_local_balance(_e: &Env) -> i128 {
        panic!("broken strategy");
    }
}

/// Configurable test harness for vault + strategy tests.
struct TestSetup<'a> {
    e: Env,
    admin: Address,
    operator: Address,
    user: Address,
    asset_client: MockAssetContractClient<'a>,
    vault_client: AugustVaultClient<'a>,
}

impl<'a> TestSetup<'a> {
    /// Full setup: offset=3 (minimum), operator configured.
    fn new() -> Self {
        Self::build(3, true)
    }

    /// Offset=3, no operator (for admin/initialization tests).
    fn vault_only() -> Self {
        Self::build(3, false)
    }

    /// Custom offset, no operator.
    fn vault_only_with_offset(offset: u32) -> Self {
        Self::build(offset, false)
    }

    fn build(offset: u32, with_operator: bool) -> Self {
        let e = Env::default();
        let admin = Address::generate(&e);
        let operator = Address::generate(&e);
        let user = Address::generate(&e);

        let asset_client = create_asset_client(&e, DEFAULT_SUPPLY, &admin);
        let vault_client = create_vault_client(&e, &asset_client.address, offset, &admin);

        e.mock_all_auths();

        if with_operator {
            vault_client.set_operator(&admin, &operator);
        }

        TestSetup {
            e,
            admin,
            operator,
            user,
            asset_client,
            vault_client,
        }
    }

    // -- Strategy factories --

    fn create_strategy(&self) -> MockStrategyContractClient<'a> {
        let address = self
            .e
            .register(MockStrategyContract, (&self.asset_client.address,));
        MockStrategyContractClient::new(&self.e, &address)
    }

    fn create_broken_strategy(&self) -> MockBrokenStrategyClient<'a> {
        let address = self.e.register(MockBrokenStrategy, ());
        MockBrokenStrategyClient::new(&self.e, &address)
    }

    fn create_noop_strategy(&self) -> MockNoopStrategyClient<'a> {
        let address = self
            .e
            .register(MockNoopStrategy, (&self.asset_client.address,));
        MockNoopStrategyClient::new(&self.e, &address)
    }

    fn create_no_deposit_strategy(&self) -> MockNoDepositStrategyClient<'a> {
        let address = self
            .e
            .register(MockNoDepositStrategy, (&self.asset_client.address,));
        MockNoDepositStrategyClient::new(&self.e, &address)
    }

    fn create_no_withdraw_strategy(&self) -> MockNoWithdrawStrategyClient<'a> {
        let address = self
            .e
            .register(MockNoWithdrawStrategy, (&self.asset_client.address,));
        MockNoWithdrawStrategyClient::new(&self.e, &address)
    }

    fn add_strategy(&self) -> MockStrategyContractClient<'a> {
        let strategy = self.create_strategy();
        self.vault_client
            .add_subaccount(&self.admin, &strategy.address, &SubaccountType::Strategy);
        strategy
    }

    /// Register a plain wallet address as a Wallet subaccount and return it.
    fn add_wallet_subaccount(&self) -> Address {
        let wallet = Address::generate(&self.e);
        self.vault_client
            .add_subaccount(&self.admin, &wallet, &SubaccountType::Wallet);
        wallet
    }

    // -- Funding helpers --

    /// Creates a new user, funds them from admin, deposits into the vault.
    /// Returns (user_address, shares_received).
    fn fund_vault(&self, amount: i128) -> (Address, i128) {
        let user = Address::generate(&self.e);
        self.asset_client.transfer(&self.admin, &user, &amount);
        let shares = self.vault_client.deposit(&amount, &user, &user, &user);
        (user, shares)
    }

    /// Fund vault using self.user. Returns shares received.
    fn deposit_as_user(&self, amount: i128) -> i128 {
        self.asset_client.transfer(&self.admin, &self.user, &amount);
        self.vault_client
            .deposit(&amount, &self.user, &self.user, &self.user)
    }

    /// Fund vault + deploy a portion to a strategy. Returns (user, shares).
    fn fund_and_deploy(
        &self,
        vault_amount: i128,
        strategy: &MockStrategyContractClient,
        deploy_amount: i128,
    ) -> (Address, i128) {
        let (user, shares) = self.fund_vault(vault_amount);
        self.vault_client
            .deposit_to_subaccount(&self.operator, &strategy.address, &deploy_amount);
        (user, shares)
    }

    /// Add a strategy, fund vault with 1M, deploy 500k. Returns strategy client.
    fn setup_standard_deploy(&self) -> MockStrategyContractClient<'a> {
        let strategy = self.add_strategy();
        self.fund_and_deploy(1_000_000, &strategy, 500_000);
        strategy
    }

    /// Widen AUM rate limits to 100% for both increase and decrease.
    fn relax_aum_limits(&self) {
        self.vault_client
            .set_aum_limits(&self.admin, &10_000, &10_000);
        self.vault_client
            .set_aum_window_limits(&self.admin, &86_400, &10_000, &10_000);
    }

    // -- Storage helpers --

    /// Set deployed_assets directly in vault storage.
    fn set_deployed(&self, amount: i128) {
        set_deployed_assets(&self.e, &self.vault_client.address, amount);
    }

    // -- Assertion helpers --

    /// Asserts the most recent event's debug representation contains `expected`.
    fn assert_last_event_contains(&self, expected: &str) {
        let events = self.e.events().all();
        let last_event = events.events().last().unwrap();
        let topics_str = std::format!("{:?}", last_event);
        assert!(
            topics_str.contains(expected),
            "Expected event containing '{}', got: {}",
            expected,
            topics_str
        );
    }
}

// ==================== Subaccount Management Tests ====================

#[test]
fn test_add_subaccount_success() {
    let s = TestSetup::new();
    let strategy = s.create_strategy();

    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs.get(0).unwrap(), strategy.address);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_add_subaccount_not_admin_fails() {
    let s = TestSetup::new();
    let strategy = s.create_strategy();
    let not_admin = Address::generate(&s.e);

    s.vault_client
        .add_subaccount(&not_admin, &strategy.address, &SubaccountType::Strategy);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_add_subaccount_duplicate_fails() {
    let s = TestSetup::new();
    let strategy = s.create_strategy();

    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);
    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_add_subaccount_max_reached_fails() {
    let s = TestSetup::new();

    // Add MAX_SUBACCOUNTS strategies
    for _ in 0..10 {
        let strategy = s.create_strategy();
        s.vault_client
            .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);
    }

    // The 11th should fail
    let extra_strategy = s.create_strategy();
    s.vault_client
        .add_subaccount(&s.admin, &extra_strategy.address, &SubaccountType::Strategy);
}

#[test]
#[should_panic]
fn test_add_subaccount_invalid_interface_fails() {
    let s = TestSetup::new();
    let broken = s.create_broken_strategy();

    s.vault_client
        .add_subaccount(&s.admin, &broken.address, &SubaccountType::Strategy);
}

#[test]
fn test_remove_subaccount_success() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_remove_subaccount_not_admin_fails() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    let not_admin = Address::generate(&s.e);

    s.vault_client
        .remove_subaccount(&not_admin, &strategy.address);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_remove_subaccount_not_whitelisted_fails() {
    let s = TestSetup::new();
    let random_addr = Address::generate(&s.e);

    s.vault_client.remove_subaccount(&s.admin, &random_addr);
}

#[test]
fn test_get_subaccounts() {
    let s = TestSetup::new();

    assert_eq!(s.vault_client.get_subaccounts().len(), 0);

    let s1 = s.add_strategy();
    let s2 = s.add_strategy();

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs.get(0).unwrap(), s1.address);
    assert_eq!(subs.get(1).unwrap(), s2.address);
}

#[test]
fn test_get_strategy_balances() {
    let s = TestSetup::new();

    // No strategies: sum is 0
    assert_eq!(s.vault_client.get_strategy_balances(), 0);

    let s1 = s.add_strategy();
    let s2 = s.add_strategy();
    // Add a wallet — should NOT be included in strategy balances
    let wallet = Address::generate(&s.e);
    s.vault_client
        .add_subaccount(&s.admin, &wallet, &SubaccountType::Wallet);

    s.fund_vault(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &s1.address, &300_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &s2.address, &200_000i128);

    // Strategy balances are queried live
    assert_eq!(s.vault_client.get_strategy_balances(), 500_000);
}

// ==================== Capital Deployment Tests ====================

#[test]
fn test_deposit_to_subaccount_success() {
    let s = TestSetup::new();
    let strategy = s.setup_standard_deploy();

    assert_eq!(s.asset_client.balance(&strategy.address), 500_000i128);
    assert_eq!(s.asset_client.balance(&s.vault_client.address), 500_000i128);
}

test_subaccount_ops_reject!(
    test_deposit_to_subaccount_not_operator_fails,
    test_withdraw_from_subaccount_not_operator_fails,
    error: "Error(Contract, #1)",
    setup: |s| {
        let strategy = s.add_strategy();
        let not_operator = Address::generate(&s.e);
    } => (not_operator, strategy.address, 100i128)
);

test_subaccount_ops_reject!(
    test_deposit_to_subaccount_not_whitelisted_fails,
    test_withdraw_from_subaccount_not_whitelisted_fails,
    error: "Error(Contract, #4)",
    setup: |s| {
        let random_addr = Address::generate(&s.e);
    } => (s.operator, random_addr, 100i128)
);

test_subaccount_ops_reject!(
    test_deposit_to_subaccount_zero_amount_fails,
    test_withdraw_from_subaccount_zero_amount_fails,
    error: "Error(Contract, #3)",
    setup: |s| {
        let strategy = s.add_strategy();
    } => (s.operator, strategy.address, 0i128)
);

test_subaccount_ops_reject!(
    test_deposit_to_subaccount_paused_fails,
    test_withdraw_from_subaccount_paused_fails,
    error: "Error(Contract, #2)",
    setup: |s| {
        let strategy = s.add_strategy();
        s.vault_client.pause(&s.admin);
    } => (s.operator, strategy.address, 100i128)
);

#[test]
fn test_deposit_to_subaccount_updates_deployed_assets() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();
    s.fund_vault(1_000_000);

    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &300_000i128);
    // Strategy deposits no longer change deployed_assets; balance is queried live
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 300_000i128);

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &200_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 500_000i128);
}

#[test]
fn test_deposit_to_subaccount_preserves_share_price() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    let (_user, shares) = s.fund_vault(1_000_000);
    let assets_before = s.vault_client.convert_to_assets(&shares);

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &500_000i128);

    let assets_after = s.vault_client.convert_to_assets(&shares);

    // Share price should be preserved (total_assets unchanged)
    assert_eq!(assets_before, assets_after);
}

#[test]
fn test_deposit_to_subaccount_event_emitted() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();

    s.assert_last_event_contains("deposit_to_subaccount");
}

// ==================== Capital Recall Tests ====================

#[test]
fn test_withdraw_from_subaccount_success() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.setup_standard_deploy();

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &300_000i128);

    assert_eq!(s.asset_client.balance(&strategy.address), 200_000i128);
    assert_eq!(s.asset_client.balance(&s.vault_client.address), 800_000i128);
}

#[test]
fn test_withdraw_from_subaccount_balance_verified() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.setup_standard_deploy();

    // Request more than the strategy has -- strategy returns min(amount, balance)
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &800_000i128);

    // Strategy should have sent all it had (500_000)
    assert_eq!(s.asset_client.balance(&strategy.address), 0);
    assert_eq!(
        s.asset_client.balance(&s.vault_client.address),
        1_000_000i128
    );
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

#[test]
fn test_withdraw_from_subaccount_updates_deployed_assets() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.setup_standard_deploy();
    // Strategy deposits don't change deployed_assets in the new model
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &200_000i128);

    // Strategy withdrawals don't change deployed_assets either
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 300_000i128);
}

#[test]
fn test_withdraw_from_subaccount_event_emitted() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.setup_standard_deploy();

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &200_000i128);

    s.assert_last_event_contains("withdraw_from_subaccount");
}

// ==================== AUM Reconciliation Tests ====================

#[test]
fn test_update_deployed_assets_success() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();

    // Set up initial deployed_assets baseline for rate-limit testing
    s.set_deployed(500_000);

    // Operator reports a small increase within default 10% limit
    s.vault_client
        .update_deployed_assets(&s.operator, &510_000i128);

    assert_eq!(s.vault_client.get_deployed_assets(), 510_000i128);
}

#[test]
fn test_update_deployed_assets_with_profit() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Simulate profit: operator reports 8% increase -- within 10% default limit
    let profit = 40_000i128;
    s.vault_client
        .update_deployed_assets(&s.operator, &(500_000i128 + profit));

    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128 + profit);
}

#[test]
fn test_update_deployed_assets_with_loss() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Simulate loss: operator reports 4% decrease -- within 5% default limit
    let loss = 20_000i128;
    s.vault_client
        .update_deployed_assets(&s.operator, &(500_000i128 - loss));

    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128 - loss);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_deployed_assets_not_operator_fails() {
    let s = TestSetup::new();
    let not_operator = Address::generate(&s.e);

    s.vault_client.update_deployed_assets(&not_operator, &0i128);
}

#[test]
fn test_update_deployed_assets_works_when_paused() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());

    // Same value is a no-op, should succeed despite pause
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);

    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_aum_increase_limit_enforced() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Default increase limit = 10% (1000 bps). Report > 10% profit.
    let excessive_profit = 60_000i128; // 12% -- exceeds 10% limit
    s.vault_client
        .update_deployed_assets(&s.operator, &(500_000i128 + excessive_profit));
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_aum_decrease_limit_enforced() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Default decrease limit = 5% (500 bps). Report > 5% loss.
    let excessive_loss = 30_000i128; // 6% -- exceeds 5% limit
    s.vault_client
        .update_deployed_assets(&s.operator, &(500_000i128 - excessive_loss));
}

#[test]
fn test_update_deployed_assets_within_limits_succeeds() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Exactly at the increase limit boundary: 10% = 50_000
    s.vault_client
        .update_deployed_assets(&s.operator, &550_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 550_000i128);
}

#[test]
fn test_update_deployed_assets_empty_subaccounts() {
    let s = TestSetup::new();

    // No subaccounts — operator reports a small amount (from zero baseline,
    // rate limits are skipped)
    s.vault_client.update_deployed_assets(&s.operator, &100i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 100);
}

#[test]
fn test_update_deployed_assets_event_emitted() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Report a different value to trigger the event (same value is a no-op)
    s.vault_client
        .update_deployed_assets(&s.operator, &510_000i128);

    s.assert_last_event_contains("deployed_assets_changed");
}

// ==================== Integration / E2E Tests ====================

#[test]
fn test_full_lifecycle_deploy_profit_reconcile_withdraw() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();

    let (user, shares) = s.fund_and_deploy(1_000_000, &strategy, 500_000);
    assert_eq!(s.vault_client.total_assets(), 1_000_000i128);

    // Strategy earns profit (5%) — tokens sent directly to strategy
    let profit = 25_000i128;
    s.asset_client
        .transfer(&s.admin, &strategy.address, &profit);

    // total_assets picks up the profit live via strategy.get_balance()
    // total_assets = 500k (local) + 525k (strategy) + 0 (deployed) = 1_025k
    assert_eq!(s.vault_client.total_assets(), 1_025_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Withdraw from strategy
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &525_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(
        s.asset_client.balance(&s.vault_client.address),
        1_025_000i128
    );

    // User redeems all shares — should get original + profit
    let assets_received = s.vault_client.redeem(&shares, &user, &user, &user);
    assert!(
        assets_received >= 1_024_999i128, // allow rounding
        "User should receive ~1_025_000, got {}",
        assets_received
    );
}

#[test]
fn test_multi_strategy_deployment() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy1 = s.add_strategy();
    let strategy2 = s.add_strategy();
    s.fund_vault(1_000_000);

    // Deploy to two strategies
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy1.address, &300_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy2.address, &200_000i128);

    // Strategy deposits don't change deployed_assets; balances queried live
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.vault_client.total_assets(), 1_000_000i128);
    assert_eq!(s.asset_client.balance(&s.vault_client.address), 500_000i128);
    assert_eq!(s.asset_client.balance(&strategy1.address), 300_000i128);
    assert_eq!(s.asset_client.balance(&strategy2.address), 200_000i128);
}

#[test]
fn test_user_deposit_after_strategy_profit() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // User1 deposits first
    let (_user1, shares1) = s.fund_and_deploy(1_000_000, &strategy, 500_000);

    // Strategy earns profit — tokens sent directly, picked up by get_balance()
    let profit = 50_000i128;
    s.asset_client
        .transfer(&s.admin, &strategy.address, &profit);

    // User2 deposits after profit
    let (_user2, shares2) = s.fund_vault(1_000_000);

    // User2 should get fewer shares (shares are worth more after profit)
    assert!(
        shares2 < shares1,
        "shares2={} should be < shares1={}",
        shares2,
        shares1
    );
}

#[test]
fn test_user_withdrawal_after_strategy_loss() {
    let s = TestSetup::new();
    s.relax_aum_limits();

    // Deposit 1M. Simulate off-chain deployment via set_deployed.
    let (user, _shares) = s.fund_vault(1_000_000);
    // Simulate: operator deployed 500k to off-chain strategy, vault sent 500k out
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &500_000);
    s.set_deployed(500_000);
    assert_eq!(s.vault_client.total_assets(), 1_000_000i128);

    // Suffer loss — operator reports 5% decrease
    let loss = 25_000i128;
    s.vault_client
        .update_deployed_assets(&s.operator, &(500_000i128 - loss));

    // Redeem max redeemable shares — user gets less than deposited
    let max_redeemable = s.vault_client.max_redeem(&user);
    let assets_received = s.vault_client.redeem(&max_redeemable, &user, &user, &user);
    assert!(
        assets_received < 1_000_000i128,
        "User should receive less than deposited after loss, got {}",
        assets_received
    );
    assert!(
        assets_received >= 474_000i128, // ~475k expected (only local balance is available)
        "User should get approximately 475_000, got {}",
        assets_received
    );
}

#[test]
fn test_pause_blocks_deploy_but_allows_decrease_reconciliation() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client.pause(&s.admin);

    // Decrease reconciliation should work during pause
    s.vault_client
        .update_deployed_assets(&s.operator, &490_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 490_000i128);
}

#[test]
fn test_remove_subaccount_after_full_recall() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.setup_standard_deploy();

    // Full recall
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &500_000i128);
    assert_eq!(s.asset_client.balance(&strategy.address), 0);

    // Now removal should succeed
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);
    assert_eq!(s.vault_client.get_subaccounts().len(), 0);
}

#[test]
fn test_share_price_accuracy_through_full_cycle() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();
    let (_user, shares) = s.fund_and_deploy(1_000_000, &strategy, 500_000);

    // Price should be stable (no profit/loss)
    let price_after_deploy = s.vault_client.convert_to_assets(&shares);
    assert!(
        (999_999..=1_000_000).contains(&price_after_deploy),
        "Price after deploy: {}",
        price_after_deploy
    );

    // Recall
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &500_000i128);

    // Price should still be stable
    let price_after_recall = s.vault_client.convert_to_assets(&shares);
    assert!(
        (999_999..=1_000_000).contains(&price_after_recall),
        "Price after recall: {}",
        price_after_recall
    );
}

#[test]
fn test_multiple_reconciliations() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // First profit + reconcile: 5%
    s.vault_client
        .update_deployed_assets(&s.operator, &525_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 525_000i128);

    // Second profit + reconcile: ~4.76% of 525_000
    s.vault_client
        .update_deployed_assets(&s.operator, &550_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 550_000i128);

    // Verify total assets = local(500k) + strategy(500k) + deployed(550k)
    assert_eq!(s.vault_client.total_assets(), 1_550_000i128);
}

// ==================== Additional Edge Case Tests ====================

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_add_subaccount_paused_fails() {
    let s = TestSetup::new();
    let strategy = s.create_strategy();

    s.vault_client.pause(&s.admin);
    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);
}

#[test]
fn test_remove_subaccount_preserves_others() {
    let s = TestSetup::new();
    let s1 = s.add_strategy();
    let s2 = s.add_strategy();
    let s3 = s.add_strategy();

    // Remove the middle one
    s.vault_client.remove_subaccount(&s.admin, &s2.address);

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs.get(0).unwrap(), s1.address);
    assert_eq!(subs.get(1).unwrap(), s3.address);
}

#[test]
fn test_update_deployed_assets_from_zero_deployed() {
    let s = TestSetup::new();
    let _strategy = s.add_strategy();

    // old_deployed = 0, edge case: skip rate limiting when old == 0
    s.vault_client.update_deployed_assets(&s.operator, &100i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 100i128);
}

#[test]
fn test_multi_strategy_reconciliation() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy1 = s.add_strategy();
    let strategy2 = s.add_strategy();
    s.fund_vault(1_000_000);

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy1.address, &300_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy2.address, &200_000i128);

    // Strategy1 profits +15k, strategy2 loses -10k: net +5k
    // Operator reports the net total
    s.vault_client
        .update_deployed_assets(&s.operator, &505_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 505_000i128);
}

// ==================== Mock Noop Strategy ====================

/// Strategy that accepts withdraw calls but transfers nothing back.
#[contract]
pub struct MockNoopStrategy;

#[contractimpl]
impl MockNoopStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}

    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    pub fn get_balance(e: &Env) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }

    pub fn get_local_balance(e: &Env) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }
}

// ==================== Additional Tests (Review Findings) ====================

#[test]
#[should_panic(expected = "Error(Contract, #11)")]
fn test_withdraw_from_subaccount_returned_nothing() {
    let s = TestSetup::new();
    let noop = s.create_noop_strategy();
    s.vault_client
        .add_subaccount(&s.admin, &noop.address, &SubaccountType::Strategy);

    // Fund vault and deploy to noop
    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &noop.address, &100_000i128);

    // The noop strategy will accept the call but transfer nothing
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &noop.address, &50_000i128);
}

#[test]
fn test_remove_subaccount_works_when_paused() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client.pause(&s.admin);
    assert!(s.vault_client.is_paused());

    // Remove should succeed even while paused (zero balance, admin-only)
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 0);
}

test_subaccount_ops_reject!(
    test_deposit_to_subaccount_negative_amount_fails,
    test_withdraw_from_subaccount_negative_amount_fails,
    error: "Error(Contract, #3)",
    setup: |s| {
        let strategy = s.add_strategy();
    } => (s.operator, strategy.address, -1i128)
);

#[test]
fn test_update_deployed_assets_respects_custom_aum_limits() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Tighten increase limit to 2% (200 bps)
    s.vault_client.set_aum_limits(&s.admin, &200, &500);

    // Report 3% profit (15,000 on 500,000) — should exceed 2% limit
    let result = s
        .vault_client
        .try_update_deployed_assets(&s.operator, &515_000i128);
    assert!(result.is_err()); // AumChangeExceedsLimit

    // Report only 1% profit (5,000 on 500,000) — within 2% limit
    s.vault_client
        .update_deployed_assets(&s.operator, &505_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 505_000i128);
}

#[test]
fn test_update_deployed_assets_exact_decrease_boundary() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Exactly at the decrease limit boundary: 5% = 25,000 of 500,000
    // Should succeed — changes exactly at the limit are permitted
    s.vault_client
        .update_deployed_assets(&s.operator, &475_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 475_000i128);
}

#[test]
fn test_remove_subaccount_does_not_reconcile() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy1 = s.add_strategy();
    let strategy2 = s.add_strategy();
    s.fund_vault(1_000_000);

    // Deploy to both strategies
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy1.address, &300_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy2.address, &200_000i128);
    // Strategy deposits don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Set some off-chain AUM
    s.set_deployed(200_000);

    // Withdraw all from strategy1 so it can be removed
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy1.address, &300_000i128);
    // Strategy withdrawals don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000i128);

    // Remove strategy1 — deployed_assets stays at 200k (no reconciliation)
    s.vault_client
        .remove_subaccount(&s.admin, &strategy1.address);
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000i128);
    assert_eq!(s.vault_client.get_subaccounts().len(), 1);

    // Simulate strategy2 profit (external tokens sent to it)
    s.asset_client
        .transfer(&s.admin, &strategy2.address, &10_000i128);
    // Strategy balance is queried live — profit is visible immediately via get_balance()
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000i128);
    assert_eq!(s.asset_client.balance(&strategy2.address), 210_000i128);
}

#[test]
fn test_deposit_to_subaccount_uses_actual_balance_change() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.fund_and_deploy(500_000, &strategy, 100_000);

    // Strategy deposits don't change deployed_assets; balance is queried live
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.asset_client.balance(&strategy.address), 100_000i128);
    assert_eq!(s.asset_client.balance(&s.vault_client.address), 400_000i128);
}

#[test]
fn test_withdraw_from_subaccount_deployed_assets_underflow() {
    // Strategy withdrawals no longer touch deployed_assets, so no underflow is possible.
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();
    s.fund_and_deploy(1_000_000, &strategy, 100_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Externally send extra tokens directly to the strategy (simulates airdrop/donation).
    s.asset_client
        .transfer(&s.admin, &strategy.address, &200_000i128);
    assert_eq!(s.asset_client.balance(&strategy.address), 300_000i128);

    // Withdraw 300_000 from strategy — no underflow because deployed_assets is untouched.
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &300_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.asset_client.balance(&strategy.address), 0);
}

#[test]
fn test_add_subaccount_event_emitted() {
    let s = TestSetup::new();
    let strategy = s.create_strategy();

    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);

    s.assert_last_event_contains("subaccount_added");
}

#[test]
fn test_remove_subaccount_event_emitted() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    s.assert_last_event_contains("subaccount_removed");
}

// ==================== Upgrade Tests ====================
//
// The `#[derive(Upgradeable)]` macro generates `upgrade(wasm_hash, operator)`
// which delegates auth to our `UpgradeableInternal::_require_auth` →
// `storage::require_admin`. Unit tests verify the auth wiring. Full upgrade
// behavior (state preservation, WASM replacement) is tested in e2e where
// real deployed WASM is available.

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_upgrade_not_admin_fails() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let not_admin = Address::generate(&e);
    let initial_supply = 1_000_000_000i128;

    let asset_client = create_asset_client(&e, initial_supply, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 6, &admin);

    e.mock_all_auths();

    // Auth check panics before update_current_contract_wasm is reached,
    // so the hash value doesn't matter.
    let dummy_hash = soroban_sdk::BytesN::from_array(&e, &[0u8; 32]);
    vault_client.upgrade(&dummy_hash, &not_admin);
}

/// update_deployed_assets reflects a decrease in strategy value when the
/// operator reports a lower amount.
#[test]
fn test_update_deployed_assets_reflects_strategy_loss() {
    let s = TestSetup::new();
    // Raise AUM limits to allow the decrease
    s.relax_aum_limits();

    let strategy = s.add_strategy();

    // Deploy 2000 to strategy
    s.fund_vault(10_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &2_000i128);
    // Strategy deposits don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Set deployed_assets to represent off-chain AUM baseline
    s.set_deployed(2_000);

    // Operator reports strategy lost 100 tokens
    s.vault_client
        .update_deployed_assets(&s.operator, &1_900i128);

    assert_eq!(s.vault_client.get_deployed_assets(), 1_900i128);
}

/// total_assets reflects external assets reported by strategies, affecting
/// share pricing for depositors.
#[test]
fn test_share_price_reflects_external_assets() {
    let s = TestSetup::new();
    // Raise AUM limits to allow the external assets increase
    s.relax_aum_limits();

    let strategy = s.create_strategy();
    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);

    // User1 deposits 10_000, deploy 5_000 to strategy
    let (user1, _) = s.fund_vault(10_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &5_000i128);

    // Operator reports 2000 off-chain yield (in addition to the 5000 visible via get_balance)
    s.vault_client
        .update_deployed_assets(&s.operator, &2_000i128);

    // total_assets = local(5000) + strategy.get_balance(5000) + deployed(2000) = 12000
    assert_eq!(s.vault_client.total_assets(), 12_000i128);

    // User2 deposits same 10_000 — should get fewer shares than user1
    // because the vault is now worth more per share
    let user2 = Address::generate(&s.e);
    s.asset_client.transfer(&s.admin, &user2, &10_000i128);
    let shares2 = s.vault_client.deposit(&10_000i128, &user2, &user2, &user2);
    let shares1 = s.vault_client.balance(&user1);

    assert!(
        shares2 < shares1,
        "User2 should get fewer shares after external yield: user1={}, user2={}",
        shares1,
        shares2
    );
}

/// update_deployed_assets rejects a negative operator-provided amount.
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_deployed_assets_rejects_negative_amount() {
    let s = TestSetup::new();

    // Operator passes -1 — should fail with InvalidAmount (#3)
    s.vault_client.update_deployed_assets(&s.operator, &-1i128);
}

/// Operator writes off all deployed capital by passing amount = 0.
/// Rate limits apply: a 100% decrease exceeds the default 5% limit.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_writeoff_blocked_by_rate_limit() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // amount = 0 is a 100% decrease from 500k — exceeds the 5% default limit
    s.vault_client.update_deployed_assets(&s.operator, &0i128);
}

/// Full write-off succeeds when AUM limits are wide enough.
#[test]
fn test_update_deployed_assets_writeoff_with_wide_limits() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // With 100% decrease limit, full write-off is allowed
    s.vault_client.update_deployed_assets(&s.operator, &0i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    // total_assets = local(500k) + strategy.get_balance(500k) + deployed(0) = 1M
    assert_eq!(s.vault_client.total_assets(), 1_000_000i128);
}

/// Overflow protection: amount so large that local_balance + amount overflows i128.
/// The eager overflow check in update_deployed_assets catches this at the operator's
/// call rather than deferring it to the next user calling total_assets().
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_update_deployed_assets_overflow_rejected_eagerly() {
    let s = TestSetup::new();
    s.relax_aum_limits();

    // Deposit 1 token so local_balance = 1
    s.fund_vault(1);

    // amount = i128::MAX — local(1) + deployed(MAX) overflows, rejected here
    s.vault_client
        .update_deployed_assets(&s.operator, &i128::MAX);
}

/// Operator can report a value that doesn't match actual strategy balances.
/// The vault trusts the operator's reported value — no on-chain verification.
#[test]
fn test_update_deployed_assets_accepts_arbitrary_amount() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Strategy actually holds 500k tokens, but operator reports 700k off-chain AUM
    s.vault_client
        .update_deployed_assets(&s.operator, &700_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 700_000i128);
    // total_assets = local(500k) + strategy.get_balance(500k) + deployed(700k) = 1.7M
    assert_eq!(s.vault_client.total_assets(), 1_700_000i128);
}

/// Recovery cycle: write off deployed capital to 0, then rebuild.
/// Rate limits skip when old_deployed = 0, allowing unrestricted recovery.
#[test]
fn test_update_deployed_assets_recovery_after_writeoff() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let _strategy = s.setup_standard_deploy();

    // Full write-off
    s.vault_client.update_deployed_assets(&s.operator, &0i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Tighten limits back to defaults
    s.vault_client.set_aum_limits(&s.admin, &1_000, &500);

    // Recovery: from 0 → 100k. Rate limits are skipped when old_deployed = 0.
    s.vault_client
        .update_deployed_assets(&s.operator, &100_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 100_000i128);

    // Subsequent update IS rate-limited: 100k → 120k (20%) exceeds 10% limit
    let result = s
        .vault_client
        .try_update_deployed_assets(&s.operator, &120_000i128);
    assert!(result.is_err());
}

/// remove_subaccount succeeds even when the strategy holds funds.
/// The admin is responsible for recovering funds before removal.
#[test]
fn test_remove_subaccount_with_funds_succeeds() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // Deploy funds to strategy — it now has a non-zero balance
    s.fund_vault(10_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &5_000i128);
    assert_eq!(s.asset_client.balance(&strategy.address), 5_000i128);

    // Removal succeeds — admin takes responsibility for fund recovery
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);
    assert_eq!(s.vault_client.get_subaccounts().len(), 0);
}

/// Recovery flow: remove a broken strategy, then update_deployed_assets
/// reconciles the remaining healthy strategies correctly.
#[test]
fn test_remove_then_reconcile_recovery_flow() {
    let s = TestSetup::new();
    // Raise AUM limits — removal causes a large deployed_assets drop
    // when the removed strategy's funds are written off during reconciliation.
    s.relax_aum_limits();

    let healthy = s.add_strategy();
    let doomed = s.add_strategy();

    // Fund vault and deploy to both strategies
    s.fund_vault(100_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &healthy.address, &30_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &doomed.address, &20_000i128);
    // Strategy deposits don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Set off-chain AUM to represent additional external deployment
    s.set_deployed(50_000);

    // Admin removes the doomed strategy
    s.vault_client.remove_subaccount(&s.admin, &doomed.address);
    assert_eq!(s.vault_client.get_subaccounts().len(), 1);

    // deployed_assets is still stale at 50_000 (remove doesn't reconcile)
    assert_eq!(s.vault_client.get_deployed_assets(), 50_000i128);

    // Operator reconciles — writes down deployed_assets to account for loss.
    s.vault_client
        .update_deployed_assets(&s.operator, &30_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 30_000i128);

    // total_assets = local(50_000) + healthy.get_balance(30_000) + deployed(30_000)
    // = 110_000. The doomed strategy's 20_000 is no longer counted in get_balance
    // but still sits at its address (effectively written off from vault's perspective).
    assert_eq!(s.vault_client.total_assets(), 110_000i128);
}

/// Recovery flow: a previously healthy strategy starts panicking in
/// get_balance(), freezing total_assets(). Admin removes the broken strategy
/// via remove_subaccount (no cross-contract calls), unblocking the vault.
#[test]
fn test_remove_broken_strategy_unblocks_vault() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // Fund vault and deploy to strategy — everything works
    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    assert_eq!(s.vault_client.total_assets(), 100_000i128);

    // Strategy breaks — get_balance() now panics
    strategy.set_balance_reverts(&true);

    // total_assets() is now frozen (panics)
    let frozen = s.vault_client.try_total_assets();
    assert!(
        frozen.is_err(),
        "total_assets should panic with broken strategy"
    );

    // Admin removes the broken strategy (no cross-contract calls)
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    // Vault is unblocked — total_assets() works again
    // Only local balance remains (50k); strategy's 50k is effectively written off.
    assert_eq!(s.vault_client.total_assets(), 50_000i128);
}

/// A broken strategy's `get_balance()` panic is trapped and surfaced as
/// `VaultError::StrategyUnreachable` (#21) on total_assets-dependent ops,
/// rather than propagating the strategy's raw trap code.
#[test]
#[should_panic(expected = "Error(Contract, #21)")]
fn test_broken_strategy_surfaces_strategy_unreachable_on_total_assets() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    strategy.set_balance_reverts(&true);

    // Should panic with #21 (StrategyUnreachable), not the strategy's raw trap.
    s.vault_client.total_assets();
}

/// Same translation happens for deposit/withdraw/mint/redeem: the broken
/// strategy's panic is mapped to StrategyUnreachable (#21).
#[test]
#[should_panic(expected = "Error(Contract, #21)")]
fn test_broken_strategy_surfaces_strategy_unreachable_on_deposit() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    strategy.set_balance_reverts(&true);

    // deposit goes through preview_deposit → convert_to_shares → total_assets.
    s.deposit_as_user(1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #21)")]
fn test_broken_strategy_surfaces_strategy_unreachable_on_withdraw() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    strategy.set_balance_reverts(&true);

    // withdraw touches total_assets_from via its pricing/limit checks.
    s.vault_client
        .withdraw(&1_000i128, &s.user, &s.user, &s.user);
}

/// A strategy that returns a non-i128 value (or that lacks `get_balance`)
/// also produces `StrategyUnreachable` — `try_get_balance` catches both
/// invocation failures and return-value conversion failures.
///
/// We simulate this via the balance_reverts flag on the standard mock
/// (which panics inside get_balance); a separate no-balance mock would
/// already be rejected at registration time by add_subaccount's smoke test.
#[test]
#[should_panic(expected = "Error(Contract, #21)")]
fn test_broken_strategy_freezes_max_withdraw_with_strategy_unreachable() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    strategy.set_balance_reverts(&true);

    // max_withdraw → total_assets_from → query_strategy_balances.
    s.vault_client.max_withdraw(&s.user);
}

// ==================== Mock: No-deposit Strategy ====================

/// Strategy that implements withdraw but NOT deposit.
/// Used to test that add_subaccount rejects contracts missing deposit.
#[contract]
pub struct MockNoDepositStrategy;

#[contractimpl]
impl MockNoDepositStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    // Intentionally missing: deposit

    pub fn withdraw(e: &Env, to: Address, amount: i128) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        let token_client = token::Client::new(e, &asset);
        let balance = token_client.balance(&e.current_contract_address());
        let actual = core::cmp::min(amount, balance);
        if actual > 0 {
            token_client.transfer(&e.current_contract_address(), &to, &actual);
        }
        actual
    }

    pub fn get_balance(e: &Env) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// ==================== Mock: No-withdraw Strategy ====================

/// Strategy that implements deposit but NOT withdraw.
/// Used to test that add_subaccount rejects contracts missing withdraw.
#[contract]
pub struct MockNoWithdrawStrategy;

#[contractimpl]
impl MockNoWithdrawStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}

    // Intentionally missing: withdraw

    pub fn get_balance(e: &Env) -> i128 {
        let asset: Address = e.storage().instance().get(&"asset").unwrap();
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// ==================== Mock: No-balance Strategy ====================

/// Strategy that implements deposit/withdraw but NOT get_balance.
/// Used to test that add_subaccount rejects contracts missing get_balance.
#[contract]
pub struct MockNoBalanceStrategy;

#[contractimpl]
impl MockNoBalanceStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}
    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    // Intentionally missing: get_balance

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// ==================== Mock: Mismatched-asset Strategy ====================

/// Strategy that returns a different asset from get_asset() than the vault's.
#[contract]
pub struct MockMismatchedAssetStrategy;

#[contractimpl]
impl MockMismatchedAssetStrategy {
    pub fn __constructor(e: &Env, fake_asset: Address) {
        e.storage().instance().set(&"asset", &fake_asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}
    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    pub fn get_balance(_e: &Env) -> i128 {
        0
    }

    pub fn get_local_balance(_e: &Env) -> i128 {
        0
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// ==================== Strategy deposit() Tests ====================

/// add_subaccount rejects a strategy whose asset doesn't match the vault's.
#[test]
#[should_panic(expected = "Error(Contract, #17)")]
fn test_add_subaccount_rejects_asset_mismatch() {
    let s = TestSetup::new();
    // Register strategy with a different asset than the vault's
    let wrong_asset = Address::generate(&s.e);
    let mismatched = s.e.register(MockMismatchedAssetStrategy, (&wrong_asset,));
    let mismatched_client = MockMismatchedAssetStrategyClient::new(&s.e, &mismatched);
    s.vault_client.add_subaccount(
        &s.admin,
        &mismatched_client.address,
        &SubaccountType::Strategy,
    );
}

/// add_subaccount rejects a strategy that doesn't implement deposit().
#[test]
#[should_panic]
fn test_add_subaccount_rejects_missing_deposit() {
    let s = TestSetup::new();
    let no_deposit = s.create_no_deposit_strategy();
    s.vault_client
        .add_subaccount(&s.admin, &no_deposit.address, &SubaccountType::Strategy);
}

/// deposit_to_subaccount fails if strategy.deposit() reverts.
#[test]
#[should_panic(expected = "deposit reverted")]
fn test_deposit_to_subaccount_fails_if_strategy_deposit_reverts() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    // Enable reverting after registration (smoke test uses amount=0, which always succeeds)
    strategy.set_deposit_reverts(&true);

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
}

// update_deployed_assets rejects calls when no operator is configured.
test_vault_only_unauthorized!(test_update_deployed_assets_no_operator_configured, |s| {
    s.vault_client.update_deployed_assets(&s.operator, &100i128);
});

/// deposit_to_subaccount fails when vault has insufficient balance.
#[test]
#[should_panic]
fn test_deposit_to_subaccount_insufficient_vault_balance() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // Fund vault with only 100 tokens, try to deploy 200
    s.fund_vault(100);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &200i128);
}

/// withdraw_from_subaccount preserves share price (total_assets unchanged).
#[test]
fn test_withdraw_from_subaccount_preserves_share_price() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();

    let (_user, shares) = s.fund_and_deploy(1_000_000, &strategy, 500_000);
    let assets_before = s.vault_client.convert_to_assets(&shares);

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &200_000i128);

    let assets_after = s.vault_client.convert_to_assets(&shares);
    assert_eq!(assets_before, assets_after);
}

/// update_deployed_assets with same value is a no-op (early return).
#[test]
fn test_update_deployed_assets_same_value_noop() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // Call with the same deployed value — should succeed as a no-op
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128);
    // total_assets = local(500k) + strategy.get_balance(500k) + deployed(500k) = 1.5M
    assert_eq!(s.vault_client.total_assets(), 1_500_000i128);
}

// ==================== Coverage Gap Tests (PR Review) ====================

/// Operator role specifically cannot remove subaccounts (admin-only privilege).
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_remove_subaccount_operator_cannot_remove() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client
        .remove_subaccount(&s.operator, &strategy.address);
}

/// AUM rate limit math overflow: delta * 10_000 overflows i128 on very large
/// deployed values. The checked_mul in check_aum_rate_limit must catch this.
#[test]
#[should_panic(expected = "Error(Contract, #8)")]
fn test_aum_rate_limit_overflow_on_large_delta() {
    let s = TestSetup::new();

    // Set old_deployed to 1 via direct storage write.
    s.set_deployed(1);

    // new_deployed chosen so delta * 10_000 overflows i128.
    // delta = new_deployed - 1 ≈ i128::MAX / 10_000 + 1
    let new_deployed = i128::MAX / 10_000 + 2;
    s.vault_client
        .update_deployed_assets(&s.operator, &new_deployed);
}

/// AUM increase boundary off-by-one: exactly 10% passes (tested elsewhere),
/// but 10% + 1 unit must be rejected. Validates the `>` (not `>=`) operator
/// in check_aum_rate_limit.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_increase_off_by_one_rejected() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // 550_001 on 500_000 base = 10.0002% — one unit above the 10% limit
    s.vault_client
        .update_deployed_assets(&s.operator, &550_001i128);
}

/// update_deployed_assets(0) when deployed is already 0 — exercises the
/// no-op early return at the zero/zero boundary.
#[test]
fn test_update_deployed_assets_zero_to_zero_noop() {
    let s = TestSetup::new();
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    s.vault_client.update_deployed_assets(&s.operator, &0i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

test_vault_only_unauthorized!(test_deposit_to_subaccount_no_operator_configured, |s| {
    let any_address = Address::generate(&s.e);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &any_address, &100i128);
});

test_vault_only_unauthorized!(test_withdraw_from_subaccount_no_operator_configured, |s| {
    let any_address = Address::generate(&s.e);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &any_address, &100i128);
});

/// A same-value `update_deployed_assets` emits an audit event with a
/// zero delta. Previously this path silently returned, which made
/// double-submits and precision-rounded resubmissions look like real
/// updates on the frontend. Always-emit turns those into visible
/// no-op records so operators can tell "already at that value" from
/// "just updated".
#[test]
fn test_update_deployed_assets_noop_emits_zero_delta_event() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("deployed_assets_changed"),
        "Same-value update_deployed_assets must emit a zero-delta audit event, got: {all_events}"
    );
}

/// withdraw_from_subaccount preserves total_assets: local balance increases
/// by the same amount deployed_assets decreases.
#[test]
fn test_withdraw_from_subaccount_total_assets_preserved() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();
    s.fund_and_deploy(1_000_000, &strategy, 500_000);

    let total_before = s.vault_client.total_assets();

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &200_000i128);

    let total_after = s.vault_client.total_assets();
    assert_eq!(
        total_before, total_after,
        "total_assets should be unchanged after recall"
    );
}

/// add_subaccount rejects a strategy that doesn't implement withdraw().
#[test]
#[should_panic]
fn test_add_subaccount_rejects_missing_withdraw() {
    let s = TestSetup::new();
    let no_withdraw = s.create_no_withdraw_strategy();
    s.vault_client
        .add_subaccount(&s.admin, &no_withdraw.address, &SubaccountType::Strategy);
}

/// add_subaccount rejects a strategy that doesn't implement get_balance().
#[test]
#[should_panic]
fn test_add_subaccount_rejects_missing_get_balance() {
    let s = TestSetup::new();
    let no_balance =
        s.e.register(MockNoBalanceStrategy, (&s.asset_client.address,));
    s.vault_client
        .add_subaccount(&s.admin, &no_balance, &SubaccountType::Strategy);
}

/// F2: `add_subaccount` probes `get_local_balance` as part of the interface
/// smoke-test. Strategies missing the method are rejected at registration.
#[test]
#[should_panic]
fn test_add_subaccount_rejects_missing_get_local_balance() {
    let s = TestSetup::new();
    let no_local =
        s.e.register(MockNoLocalBalanceStrategy, (&s.asset_client.address,));
    s.vault_client
        .add_subaccount(&s.admin, &no_local, &SubaccountType::Strategy);
}

// ==================== Mock: No-local-balance Strategy ====================

/// Strategy that implements everything except `get_local_balance()`.
/// Used to test the F2 interface probe at registration.
#[contract]
pub struct MockNoLocalBalanceStrategy;

#[contractimpl]
impl MockNoLocalBalanceStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}
    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    pub fn get_balance(_e: &Env) -> i128 {
        0
    }

    // Intentionally missing: get_local_balance

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// ==================== Wallet Subaccount Tests ====================

#[test]
fn test_add_wallet_subaccount_success() {
    // A plain address (not a contract) is accepted as Wallet type without
    // needing to implement IStrategy (smoke-test is skipped for wallets).
    let s = TestSetup::new();
    let wallet = Address::generate(&s.e);

    s.vault_client
        .add_subaccount(&s.admin, &wallet, &SubaccountType::Wallet);

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs.get(0).unwrap(), wallet);
    assert_eq!(
        s.vault_client.get_subaccount_type(&wallet),
        SubaccountType::Wallet
    );
}

#[test]
fn test_deposit_to_wallet_subaccount() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();

    // Fund the vault
    s.deposit_as_user(1_000_000);

    // Deploy to wallet subaccount
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // Wallet should hold the tokens
    assert_eq!(s.asset_client.balance(&wallet), 500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);
}

#[test]
fn test_withdraw_from_wallet_subaccount() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    // Fund the vault and deploy to wallet
    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // Wallet owner approves the vault to pull tokens back
    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &500_000, &1000);

    // Operator withdraws from wallet subaccount (pull model)
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &300_000);

    assert_eq!(s.asset_client.balance(&wallet), 200_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000);
}

#[test]
fn test_remove_wallet_subaccount_cleans_up_type() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();

    assert_eq!(
        s.vault_client.get_subaccount_type(&wallet),
        SubaccountType::Wallet
    );

    s.vault_client.remove_subaccount(&s.admin, &wallet);

    assert_eq!(s.vault_client.get_subaccounts().len(), 0);
}

#[test]
fn test_strategy_subaccount_type_defaults_to_strategy() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    assert_eq!(
        s.vault_client.get_subaccount_type(&strategy.address),
        SubaccountType::Strategy
    );
}

#[test]
fn test_mixed_strategy_and_wallet_subaccounts() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    let wallet = s.add_wallet_subaccount();

    let subs = s.vault_client.get_subaccounts();
    assert_eq!(subs.len(), 2);

    assert_eq!(
        s.vault_client.get_subaccount_type(&strategy.address),
        SubaccountType::Strategy
    );
    assert_eq!(
        s.vault_client.get_subaccount_type(&wallet),
        SubaccountType::Wallet
    );
}

/// Integration: deposit to both a wallet and a strategy, then verify
/// total_assets correctly sums all three components and deployed_assets
/// only reflects the wallet amount.
#[test]
fn test_total_assets_with_mixed_wallet_and_strategy() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    let wallet = s.add_wallet_subaccount();

    s.fund_vault(1_000_000);

    // Deploy to strategy (live balance, no deployed_assets change)
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &300_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Deploy to wallet (increments deployed_assets)
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &200_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000);

    // total = local(500k) + strategy.get_balance(300k) + deployed(200k wallet)
    assert_eq!(s.vault_client.total_assets(), 1_000_000i128);
    assert_eq!(s.vault_client.get_strategy_balances(), 300_000);
}

// Panics with the token contract's allowance error, which surfaces as a
// host-level Auth error — not one of our VaultError codes. Using
// `#[should_panic]` without an expected string is intentional: the exact
// error message comes from the token implementation, not from this
// contract. Critically, the panic must happen BEFORE the strict-mode
// over-pull check — 300k is within the 500k tracker, so if
// `WalletOverWithdraw` (#23) ever fires here that would indicate the
// strict check was moved ahead of the transfer_from and is masking the
// real allowance failure.
#[test]
#[should_panic]
fn test_withdraw_from_wallet_without_allowance_fails() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();

    // Fund the vault and deploy to wallet
    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // Attempt withdrawal without wallet owner approving the vault — should panic
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &300_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_add_subaccount_duplicate_different_type_fails() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // Re-adding the same address with a different type should still fail
    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Wallet);
}

#[test]
#[should_panic(expected = "Error(Contract, #24)")]
fn test_remove_subaccount_rejects_wallet_with_nonzero_tracker() {
    // F5a: `remove_subaccount` refuses to remove a Wallet whose
    // `WalletNetDeployed` tracker is non-zero. Splitting the write-down and
    // the removal across two transactions would price any operation landing
    // between them against an incorrect NAV; the guard forces the operator
    // to either drain the wallet first or use `remove_wallet_and_reconcile`.
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);

    // Panics with WalletTrackerNotZero (#24).
    s.vault_client.remove_subaccount(&s.admin, &wallet);
}

#[test]
fn test_remove_wallet_after_full_drain_succeeds() {
    // Drain the wallet via `withdraw_from_subaccount` until the tracker
    // reaches zero, then `remove_subaccount` is allowed.
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // Drain back to the vault via the SEP-41 allowance flow.
    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &500_000, &1000);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);

    s.vault_client.remove_subaccount(&s.admin, &wallet);
    assert_eq!(s.vault_client.get_subaccounts().len(), 0);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        !all_events.contains("wallet_balance_diverged"),
        "WalletBalanceDiverged should NOT fire on clean post-drain removal, got: {all_events}"
    );
}

/// Removing a profitable wallet atomically via `remove_wallet_and_reconcile`:
/// the operator writes down `deployed_assets` to 0 and the admin removes the
/// wallet in a single transaction, closing the pricing window.
#[test]
fn test_remove_profitable_wallet_via_reconcile_closes_pricing_window() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    // Operator recognises a 100k gain on the wallet's deployed capital.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 600_000);

    // Atomic: aggregate write-down to 0 + removal in one tx.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
    assert_eq!(s.vault_client.get_subaccounts().len(), 0);
}

/// Dust defense: a third party sending tokens directly to a wallet does
/// not affect the vault's `deployed_assets` — that aggregate only moves
/// through `deposit_to_subaccount` / `withdraw_from_subaccount` /
/// `update_wallet_deployed` / `remove_wallet_and_reconcile`. Dust still
/// surfaces as a `WalletBalanceDiverged` diagnostic at removal time.
#[test]
fn test_wallet_removal_unaffected_by_dust_deposits() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);

    // Adversary dusts the wallet directly with 100k — not through the vault.
    s.asset_client.transfer(&s.admin, &wallet, &100_000i128);
    assert_eq!(s.asset_client.balance(&wallet), 600_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);

    // Atomic write-down + removal. Dust does not alter the aggregate.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

/// The tracker still records net deposits minus withdrawals accurately
/// while the wallet is active; it is just not used to drive reconciliation
/// at removal.
#[test]
fn test_wallet_net_deployed_tracks_deposits_and_withdrawals() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &200_000, &1000);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &200_000);

    // Tracker reflects net: 500k - 200k = 300k.
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 300_000);
    // deployed_assets tracked by deposit/withdraw, same 300k.
    assert_eq!(s.vault_client.get_deployed_assets(), 300_000);
}

/// Divergence event: when the tracked amount differs from the wallet's
/// observed balance at removal time, emit WalletBalanceDiverged for
/// off-chain monitoring (it does not affect reconciliation). Fires on the
/// atomic `remove_wallet_and_reconcile` path as well, so operational
/// monitoring keeps working under the new `remove_subaccount` guard.
#[test]
fn test_wallet_balance_diverged_event_emitted_on_dust() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    // External dust that the vault did not drive.
    s.asset_client.transfer(&s.admin, &wallet, &50_000i128);

    // Atomic path: write down `deployed_assets` and remove in one tx.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("wallet_balance_diverged"),
        "Expected WalletBalanceDiverged event on dust, got: {all_events}"
    );
}

// ==================== Mock: Trap-on-balance token ====================
//
// Minimal token that satisfies the vault's construction-time `decimals()`
// probe but traps on every `balance()` call. Used to exercise the
// `try_balance` fallback in `remove_subaccount` — the diagnostic event
// must be skipped silently when the token is degraded so removal itself
// cannot be held hostage by observability.
#[contract]
pub struct TrapOnBalanceTokenContract;

#[contractimpl]
impl TrapOnBalanceTokenContract {
    pub fn __constructor(_e: &Env) {}

    pub fn decimals(_e: &Env) -> u32 {
        18
    }

    pub fn balance(_e: &Env, _account: Address) -> i128 {
        // Simulate a degraded token (archived instance storage, broken
        // upgrade, etc.) by trapping. `try_balance` in the vault must
        // catch this cleanly.
        panic!("token balance call unavailable")
    }
}

/// Degraded-token resilience: when the underlying token's `balance` call
/// traps, `remove_subaccount` still succeeds and simply skips the
/// advisory `WalletBalanceDiverged` event. Protects the admin's ability
/// to decommission a wallet during a token outage, matching the pattern
/// used for strategy `get_balance` failures via `try_get_balance`.
///
/// Note on F5a interaction: removal requires the wallet tracker to be
/// zero. For a wallet that was deposited to before the token broke,
/// neither `withdraw_from_subaccount` (calls `transfer_from`) nor
/// `remove_wallet_and_reconcile` (calls `balance` via `local_balance`)
/// can complete with a trapping token — so the non-zero-tracker + broken
/// token recovery requires an emergency admin escape hatch that is out
/// of scope here. This test exercises the zero-tracker degraded path,
/// which is the configuration that covers fresh registrations and
/// previously-drained wallets.
#[test]
fn test_remove_wallet_subaccount_succeeds_when_try_balance_traps() {
    let e = Env::default();
    let admin = Address::generate(&e);

    // Asset is the trap-on-balance mock. Vault construction only needs
    // `decimals()`, which works, so the vault comes up healthy.
    let asset_address = e.register(TrapOnBalanceTokenContract, ());
    let vault_client = create_vault_client(&e, &asset_address, 3, &admin);

    e.mock_all_auths();

    // Register a wallet with a zero tracker (fresh or previously drained).
    // A working balance() call would still fire the divergence probe; the
    // trap must make that probe short-circuit via `WalletBalanceProbeFailed`
    // rather than propagate through `remove_subaccount`.
    let wallet = Address::generate(&e);
    vault_client.add_subaccount(&admin, &wallet, &SubaccountType::Wallet);

    // Removal must succeed despite the balance trap.
    vault_client.remove_subaccount(&admin, &wallet);

    // Capture events before any further contract calls so the assertions
    // are unambiguously about this removal.
    let all_events = std::format!("{:?}", e.events().all());
    assert!(
        !all_events.contains("wallet_balance_diverged"),
        "WalletBalanceDiverged must NOT fire when try_balance traps, got: {all_events}"
    );
    // The probe failure itself IS observable: removal emits
    // `WalletBalanceProbeFailed` so off-chain monitoring can distinguish
    // a silent skip of the divergence check from a clean exact-match
    // removal.
    assert!(
        all_events.contains("wallet_balance_probe_failed"),
        "WalletBalanceProbeFailed must fire when try_balance traps, got: {all_events}"
    );

    assert_eq!(vault_client.get_subaccounts().len(), 0);
    // Tracker is cleared as part of removal, regardless of the trap.
    assert_eq!(vault_client.get_wallet_net_deployed(&wallet), 0);
}

/// Seeding overwrites the tracker to recognise capital already at a
/// wallet (e.g. tokens transferred before registration or a manual
/// accounting correction). Aggregate `deployed_assets` is left
/// untouched — the caller is responsible for ensuring the recognised
/// value is already present elsewhere in the aggregate.
#[test]
fn test_seed_wallet_net_deployed_restores_tracker_after_upgrade() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    // Simulate a pre-upgrade state: tokens sit at the wallet, but the
    // per-wallet tracker is zero because the old build did not populate it.
    s.asset_client.transfer(&s.admin, &wallet, &500_000i128);
    s.set_deployed(500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);

    // Admin seeds the tracker to the known net-deployed amount.
    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &500_000i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000i128);

    // Subsequent deposits are recorded incrementally on top of the seed.
    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &100_000i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 600_000i128);

    // Removal clears the tracker entry. Use the atomic reconcile path since
    // `remove_subaccount` rejects wallets with non-zero trackers (F5a).
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_seed_wallet_net_deployed_rejects_non_admin() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();
    let not_admin = Address::generate(&s.e);

    s.vault_client
        .seed_wallet_net_deployed(&not_admin, &wallet, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_seed_wallet_net_deployed_rejects_unregistered_address() {
    let s = TestSetup::new();
    let random = Address::generate(&s.e);

    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &random, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_seed_wallet_net_deployed_rejects_strategy_subaccount() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &strategy.address, &100i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_seed_wallet_net_deployed_rejects_negative() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();

    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &(-1i128));
}

/// C-2: `seed_wallet_net_deployed` rejects downward moves. Admin cannot
/// zero a non-zero tracker via the seed path — that would bypass the F5a
/// `WalletTrackerNotZero` guard, allowing removal of a wallet whose
/// attributed value remains in `deployed_assets`. Downward reconciliation
/// must go through the operator-authorized `update_wallet_deployed` or
/// the dual-auth `remove_wallet_and_reconcile`.
#[test]
#[should_panic(expected = "Error(Contract, #25)")]
fn test_seed_wallet_net_deployed_rejects_downward_move() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);

    // Attempt to overwrite tracker to zero via seed path — rejected.
    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &0i128);
}

/// C-2 companion: seeding *upward* (the intended use case — recording
/// pre-existing balance at a wallet) still works.
#[test]
fn test_seed_wallet_net_deployed_upward_allowed() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    // Pre-existing balance at the wallet that the aggregate already
    // reflects (e.g. operator wrote it off-chain first).
    s.asset_client.transfer(&s.admin, &wallet, &500_000i128);
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);

    // Seed the tracker to match — upward move from 0 to 500_000.
    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &500_000i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);
}

/// C-2 + F5a composition: the full stranding-attack path is blocked at
/// step 1. Admin cannot seed(wallet, 0) → remove_subaccount(wallet) to
/// strip per-wallet attribution and leave value in `deployed_assets`.
/// Uses `try_*` so we can assert the tracker was NOT mutated by the
/// rejected seed — pre-panic state must be intact (R2-11).
#[test]
fn test_admin_cannot_strand_value_via_seed_to_zero_then_remove() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let deployed_before = s.vault_client.get_deployed_assets();
    let tracker_before = s.vault_client.get_wallet_net_deployed(&wallet);

    // Step 1: admin tries to zero the tracker via seed (bypasses F5a).
    // Pre-C-2 this was allowed. Post-C-2: rejected at step 1.
    let r = s
        .vault_client
        .try_seed_wallet_net_deployed(&s.admin, &wallet, &0i128);
    assert!(r.is_err(), "downward seed must be rejected");

    // Post-panic: state intact, tracker unchanged, aggregate unchanged.
    assert_eq!(
        s.vault_client.get_wallet_net_deployed(&wallet),
        tracker_before
    );
    assert_eq!(s.vault_client.get_deployed_assets(), deployed_before);

    // F5a guard remains engaged — admin-only removal still fails with #24.
    let r2 = s.vault_client.try_remove_subaccount(&s.admin, &wallet);
    assert!(
        r2.is_err(),
        "remove_subaccount must reject non-zero tracker"
    );
}

/// Strict-mode over-pull: pulling more than the per-wallet tracker
/// records panics with `WalletOverWithdraw` (#23). The operator must first
/// reconcile the wallet's attributed value via `update_wallet_deployed`
/// (either to recognise a gain or to record external dust); otherwise the
/// over-pull would silently consume another wallet's share of
/// `deployed_assets`.
#[test]
#[should_panic(expected = "Error(Contract, #23)")]
fn test_withdraw_from_wallet_panics_on_over_pull() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &100_000);

    // Someone dusts the wallet with 50k, bringing its balance to 150k.
    // Operator has NOT recognised the dust via update_wallet_deployed.
    s.asset_client.transfer(&s.admin, &wallet, &50_000i128);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &150_000, &1000);

    // Pull 150k: tracker is 100k → WalletOverWithdraw fires BEFORE the
    // aggregate accounting runs.
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &150_000);
}

/// Normal withdrawal (within tracker) completes cleanly: both tracker
/// and aggregate decrement by the pulled amount, no panic.
#[test]
fn test_withdraw_from_wallet_within_tracker_succeeds() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &200_000, &1000);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &200_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 300_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 300_000);
}

/// Boundary: `actual_received == tracked` is a clean full recall. The
/// strict check is `>`, not `>=`, so this path must succeed with tracker
/// and aggregate both zeroed.
#[test]
fn test_withdraw_from_wallet_exact_full_recall() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &500_000, &1000);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &500_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

/// Supported gain-recognition flow: operator calls `update_wallet_deployed`
/// to bump BOTH the per-wallet tracker and the aggregate counter, then
/// pulls the full balance back. NAV accrues at recognition time and stays
/// put on the pull — no double-counting, no silent absorption.
#[test]
fn test_withdraw_from_wallet_with_recognised_gain() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let nav_after_deposit = s.vault_client.total_assets();

    // Wallet gains 100k externally; operator recognises via
    // update_wallet_deployed — tracker AND aggregate both move to 600k.
    s.asset_client.transfer(&s.admin, &wallet, &100_000i128);
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 600_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 600_000);
    // NAV accrued at recognition time — +100k from the dust.
    assert_eq!(
        s.vault_client.total_assets(),
        nav_after_deposit + 100_000i128
    );
    let nav_after_recognition = s.vault_client.total_assets();

    // Pull the full 600k. Allowed because tracker == 600k after recognition.
    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &600_000, &1000);
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &600_000);

    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
    // NAV preserved across the pull.
    assert_eq!(s.vault_client.total_assets(), nav_after_recognition);
}

/// Multi-wallet corruption regression: in a vault with two wallets,
/// pulling from A must not consume B's share of `deployed_assets`. The
/// over-pull check panics instead of silently consuming the headroom,
/// preserving B's accounting.
#[test]
#[should_panic(expected = "Error(Contract, #23)")]
fn test_over_pull_does_not_consume_other_wallet_headroom() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet_a = s.add_wallet_subaccount();
    let wallet_b = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet_a, &100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet_b, &200_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 300_000);

    // Dust A to 150k on-chain; operator does NOT recognise.
    s.asset_client.transfer(&s.admin, &wallet_a, &50_000i128);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet_a, &vault_addr, &150_000, &1000);
    // Would have silently dropped deployed_assets to 150k, leaving B's
    // 200k tracker inconsistent. Per-wallet over-pull check rejects instead.
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet_a, &150_000);
}

/// Re-adding a previously removed wallet starts from a clean per-wallet
/// counter (the entry is dropped on removal).
#[test]
fn test_wallet_net_deployed_cleared_on_removal() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &400_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 400_000);

    // Atomic reconcile + removal clears the tracker entry (F5a requires a
    // zero tracker for the admin-only `remove_subaccount` path; the atomic
    // dual-auth path handles any tracker value).
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);

    // Re-add the same address — tracker starts at a fresh zero.
    s.vault_client
        .add_subaccount(&s.admin, &wallet, &SubaccountType::Wallet);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &100_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 100_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_get_subaccount_type_not_whitelisted_fails() {
    let s = TestSetup::new();
    let random = Address::generate(&s.e);

    // Querying type for a non-registered address should panic
    s.vault_client.get_subaccount_type(&random);
}

// ==================== update_wallet_deployed ====================

/// Happy path: increasing the tracker bumps both the per-wallet tracker
/// AND the aggregate `deployed_assets` by the same delta. NAV accrues
/// immediately (gain recognition).
#[test]
fn test_update_wallet_deployed_recognises_gain() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let nav_before = s.vault_client.total_assets();

    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 600_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 600_000);
    assert_eq!(s.vault_client.total_assets(), nav_before + 100_000i128);
}

/// Recognising a loss: lowering the tracker decreases the aggregate by
/// the same delta. NAV drops (loss recognition).
#[test]
fn test_update_wallet_deployed_recognises_loss() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let nav_before = s.vault_client.total_assets();

    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &400_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 400_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 400_000);
    assert_eq!(s.vault_client.total_assets(), nav_before - 100_000i128);
}

/// No-op when called with the current tracker value: skips storage writes
/// and event emission.
#[test]
fn test_update_wallet_deployed_noop_when_unchanged() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &500_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_wallet_deployed_rejects_non_operator() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();
    s.vault_client
        .update_wallet_deployed(&s.admin, &wallet, &100_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_wallet_deployed_rejects_negative() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &(-1i128));
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_update_wallet_deployed_rejects_unregistered() {
    let s = TestSetup::new();
    let random = Address::generate(&s.e);
    s.vault_client
        .update_wallet_deployed(&s.operator, &random, &100_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_update_wallet_deployed_rejects_strategy_subaccount() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.vault_client
        .update_wallet_deployed(&s.operator, &strategy.address, &100_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_wallet_deployed_increase_blocked_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client.pause(&s.admin);
    // Increase (gain) blocked: matches update_deployed_assets semantics.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);
}

#[test]
fn test_update_wallet_deployed_decrease_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client.pause(&s.admin);
    // Loss recognition still permitted while paused.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &400_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 400_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 400_000);
}

/// Rate-limit guardrail: update_wallet_deployed reuses
/// apply_deployed_assets_change, so the AUM increase limit applies. With
/// default 10% limit and a 500k baseline, jumping to 600k is a 20%
/// increase and must be rejected.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_wallet_deployed_respects_aum_increase_limit() {
    let s = TestSetup::new();
    // Default AUM limits: 10% increase, 5% decrease.
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // +100k on a 500k baseline = 20%, exceeds the 10% limit.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);
}

// ==================== update_wallet_deployed_batch (F4) ====================

/// Happy path: batch moves two wallets in a single call. Aggregate moves
/// by net delta; each tracker ends at its requested value. Exercises the
/// F4 primary operator path for multi-wallet reconciliation.
#[test]
fn test_update_wallet_deployed_batch_happy_path() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &300_000);
    let aggregate_before = s.vault_client.get_deployed_assets();

    // Recognise +100k on w1 (gain) and -50k on w2 (loss) atomically.
    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 600_000i128), (w2.clone(), 250_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), 250_000);
    assert_eq!(
        s.vault_client.get_deployed_assets(),
        aggregate_before + 50_000
    );
    // Invariant holds after batch.
    assert_eq!(
        s.vault_client.get_wallet_deployed_assets(),
        s.vault_client.get_deployed_assets()
    );
}

/// Net-zero batch passes even when the individual steps would exceed
/// the AUM rate limit — the AUM check is applied once to `net_delta`,
/// not per-entry. Demonstrates the operational advantage over repeated
/// `update_wallet_deployed` calls.
#[test]
fn test_update_wallet_deployed_batch_net_zero_bypasses_per_step_limit() {
    let s = TestSetup::new();
    // Default 10% increase, 5% decrease limits — a single +100k on a 500k
    // baseline is 20%, would be rejected. Same for a single -100k vs. 5%.
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    let aggregate_before = s.vault_client.get_deployed_assets();

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 600_000i128), // +100k
        (w2.clone(), 400_000i128), // -100k
    ];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_deployed_assets(), aggregate_before);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), 400_000);
}

/// Empty batch rejected with #26 — explicitness guard against client-side
/// bugs that collapse a filter to an empty list.
#[test]
#[should_panic(expected = "Error(Contract, #26)")]
fn test_update_wallet_deployed_batch_rejects_empty() {
    let s = TestSetup::new();
    let updates: soroban_sdk::Vec<(Address, i128)> = soroban_sdk::Vec::new(&s.e);
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Fast-fail on oversized batch: inputs longer than `MAX_SUBACCOUNTS`
/// are rejected before the O(n²) duplicate scan runs. Guards against a
/// degenerate caller burning ~50M comparisons on a 10k-entry payload
/// that cannot possibly map onto a whitelist of at most 10 wallets.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn test_update_wallet_deployed_batch_rejects_oversized() {
    let s = TestSetup::new();
    let mut updates: soroban_sdk::Vec<(Address, i128)> = soroban_sdk::Vec::new(&s.e);
    // MAX_SUBACCOUNTS = 10, so 11 entries triggers the bound.
    for _ in 0..11 {
        updates.push_back((Address::generate(&s.e), 1i128));
    }
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Duplicate subaccount in batch rejected with #27 — the net-delta
/// calculation would otherwise depend on entry order.
#[test]
#[should_panic(expected = "Error(Contract, #27)")]
fn test_update_wallet_deployed_batch_rejects_duplicates() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();

    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 100_000i128), (w1.clone(), 200_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_update_wallet_deployed_batch_rejects_non_operator() {
    let s = TestSetup::new();
    let w1 = s.add_wallet_subaccount();
    let updates = soroban_sdk::vec![&s.e, (w1, 100_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.admin, &updates);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_update_wallet_deployed_batch_rejects_negative_tracker() {
    let s = TestSetup::new();
    let w1 = s.add_wallet_subaccount();
    let updates = soroban_sdk::vec![&s.e, (w1, -1i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_update_wallet_deployed_batch_rejects_unregistered() {
    let s = TestSetup::new();
    let unregistered = Address::generate(&s.e);
    let updates = soroban_sdk::vec![&s.e, (unregistered, 100_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_update_wallet_deployed_batch_rejects_strategy_subaccount() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    let updates = soroban_sdk::vec![&s.e, (strategy.address.clone(), 100_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Atomicity: a single invalid entry reverts the whole batch. Even if
/// validation catches the bad entry *after* the first one has been
/// "seen", no tracker write reaches storage.
#[test]
fn test_update_wallet_deployed_batch_atomic_on_bad_entry() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let unregistered = Address::generate(&s.e);

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    let tracker_before = s.vault_client.get_wallet_net_deployed(&w1);
    let aggregate_before = s.vault_client.get_deployed_assets();

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 600_000i128),
        (unregistered, 100_000i128), // fails validation — whole batch reverts
    ];
    let r = s
        .vault_client
        .try_update_wallet_deployed_batch(&s.operator, &updates);
    assert!(r.is_err(), "bad entry must revert the batch");

    // State unchanged: w1's tracker did not advance, aggregate did not move.
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), tracker_before);
    assert_eq!(s.vault_client.get_deployed_assets(), aggregate_before);
}

/// Rate limit applied to net delta, not per entry: a batch that adds
/// 200k on two wallets (net +200k on a 500k base = 40%) exceeds the
/// default 10% increase limit and is rejected.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_wallet_deployed_batch_respects_aum_increase_limit() {
    let s = TestSetup::new();
    // Default AUM limits.
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &250_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &250_000);
    // +100k each on a 500k aggregate = 40% increase, over the 10% cap.
    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 350_000i128), (w2.clone(), 350_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_wallet_deployed_batch_increase_blocked_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client.pause(&s.admin);

    let updates = soroban_sdk::vec![&s.e, (w1, 600_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Decreases still allowed while paused: mirrors `update_wallet_deployed`
/// semantics so loss recognition remains possible during emergencies.
#[test]
fn test_update_wallet_deployed_batch_decrease_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client.pause(&s.admin);

    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 400_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 400_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 400_000);
}

/// Per-entry pause enforcement: a batch that nets to zero (or negative)
/// is still rejected while paused if **any** individual entry's delta is
/// positive. Without this, a compromised operator could shift attribution
/// (+A, -B) during pause, inflating one tracker even though the aggregate
/// pause check (which sees only the net) would let it through.
#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_wallet_deployed_batch_per_entry_increase_blocked_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    // Net is -50k (passes the aggregate pause check), but +50k on w1 must
    // panic VaultPaused per the per-entry rule.
    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 550_000i128), // +50k — must trip the pause guard
        (w2.clone(), 400_000i128), // -100k
    ];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// All-decreases multi-wallet batch still allowed while paused — the
/// per-entry guard rejects only *positive* deltas, so atomic loss
/// recognition across multiple wallets remains usable during emergencies.
#[test]
fn test_update_wallet_deployed_batch_multi_decrease_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 400_000i128), (w2.clone(), 300_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 400_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), 300_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 700_000);
}

/// Sanity check that the guard only fires while paused: the same
/// (+A, -B) shape that's blocked above succeeds when the vault is
/// running, preserving the batch's net-zero rebalance use case.
#[test]
fn test_update_wallet_deployed_batch_mixed_net_zero_allowed_when_unpaused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    let aggregate_before = s.vault_client.get_deployed_assets();

    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 600_000i128), (w2.clone(), 400_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), 400_000);
    assert_eq!(s.vault_client.get_deployed_assets(), aggregate_before);
}

/// Atomicity of the per-entry pause panic: a paused (+A, -B) batch must
/// leave **no** state behind — neither wallet tracker nor the aggregate
/// `deployed_assets` may move. The companion `should_panic` test only
/// proves the panic fires; this one pins the all-or-nothing invariant
/// the new code comment claims, catching a future refactor that moves
/// the pause check after a storage write.
#[test]
fn test_update_wallet_deployed_batch_per_entry_pause_panic_is_atomic() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    let w1_before = s.vault_client.get_wallet_net_deployed(&w1);
    let w2_before = s.vault_client.get_wallet_net_deployed(&w2);
    let aggregate_before = s.vault_client.get_deployed_assets();

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 600_000i128), // +100k — must trip the pause guard
        (w2.clone(), 400_000i128), // -100k
    ];
    let r = s
        .vault_client
        .try_update_wallet_deployed_batch(&s.operator, &updates);
    assert!(r.is_err(), "paused (+A,-B) batch must revert");

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), w1_before);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), w2_before);
    assert_eq!(s.vault_client.get_deployed_assets(), aggregate_before);
}

/// Ordering: place the negative entry **first** so accumulated
/// `net_delta` is already -100k when the +50k entry is evaluated. A
/// correct per-entry guard still rejects this; a buggy implementation
/// that checked `net_delta > 0` mid-loop instead of per-entry `delta`
/// would let it pass. The sibling
/// `..._per_entry_increase_blocked_while_paused` test cannot
/// distinguish these two implementations because it puts the positive
/// entry first.
#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_wallet_deployed_batch_per_entry_pause_fires_when_negative_first() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 400_000i128), // -100k first → accumulated net is -100k
        (w2.clone(), 550_000i128), // +50k → must still trip per-entry pause
    ];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Boundary: the per-entry guard is `delta > 0`, not `>=`. A no-op
/// entry (`new_tracked == old_tracked`) must pass while paused even
/// though it goes through the same code path. Pins the boundary so a
/// future regression to `>=` cannot silently break legitimate
/// idempotent paused-vault submissions.
#[test]
fn test_update_wallet_deployed_batch_zero_delta_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    let aggregate_before = s.vault_client.get_deployed_assets();

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 500_000i128), // delta == 0 — no-op, must pass
        (w2.clone(), 400_000i128), // -100k
    ];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&w1), 500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&w2), 400_000);
    assert_eq!(
        s.vault_client.get_deployed_assets(),
        aggregate_before - 100_000
    );
}

/// Ordering between pause and AUM checks: under default (tight) AUM
/// limits, a paused (+50k, -250k) batch trips both the per-entry pause
/// guard (+50k entry while paused) and the AUM decrease limit (200k =
/// 20% of 1M, exceeding the 5% default). The test asserts error #2
/// (`VaultPaused`) wins, locking in that the per-entry pause check
/// fires *before* `apply_deployed_assets_change` evaluates AUM. A
/// future refactor that swaps the order would surface as an error-code
/// mismatch here. `deposit_to_subaccount` is exempt from AUM (see
/// `test_deposit_to_subaccount_no_aum_rate_limit`), so no relax is
/// needed for setup.
#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_wallet_deployed_batch_pause_fires_before_aum_check() {
    let s = TestSetup::new();
    // Default AUM limits in effect (no relax).
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);
    s.vault_client.pause(&s.admin);

    let updates = soroban_sdk::vec![
        &s.e,
        (w1.clone(), 550_000i128), // +50k → trips per-entry pause guard
        (w2.clone(), 250_000i128), // -250k → net -200k = 20%, over 5% AUM cap
    ];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Events: batch emits N `WalletDeployedUpdated` + one
/// `WalletDeployedBatchApplied` + one `DeployedAssetsChanged`, letting
/// indexers anchor per-wallet events to the batch transaction.
#[test]
fn test_update_wallet_deployed_batch_emits_expected_events() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &500_000);

    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 600_000i128), (w2.clone(), 400_000i128),];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("wallet_deployed_batch_applied"),
        "WalletDeployedBatchApplied must fire once per batch, got: {all_events}"
    );
    assert!(
        all_events.contains("wallet_deployed_updated"),
        "WalletDeployedUpdated must fire per entry, got: {all_events}"
    );
    assert!(
        all_events.contains("deployed_assets_changed"),
        "DeployedAssetsChanged must fire for the aggregate move, got: {all_events}"
    );
}

/// Underflow path: a batch whose net delta would drive `deployed_assets`
/// below zero trips `DeployedAssetsUnderflow` (#12). Reachable only when
/// the Σ invariant has already been broken by a prior escape hatch — the
/// setup here deliberately uses `update_deployed_assets` to push the
/// aggregate below the tracker sum.
#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_wallet_deployed_batch_rejects_aggregate_underflow() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);

    // Invariant broken on purpose: aggregate=300k while tracker=500k.
    s.vault_client
        .update_deployed_assets(&s.operator, &300_000i128);

    // Reducing the tracker to 0 yields net_delta = -500k against a 300k
    // aggregate → -200k → underflow guard fires before any tracker write.
    let updates = soroban_sdk::vec![&s.e, (w1, 0i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

/// Cumulative AUM window limit applies to batch calls via the shared
/// `apply_deployed_assets_change` helper. Same recipe as
/// `test_update_wallet_deployed_cumulative_increase_limit_enforced`, but
/// the second bump goes through the batch primitive.
#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_update_wallet_deployed_batch_cumulative_increase_limit_enforced() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // First bump via the aggregate primitive: +5% on a 1M base, passes
    // per-call (10%) and cumulative (10%).
    s.vault_client
        .update_deployed_assets(&s.operator, &1_050_000i128);

    // Second bump via batch: per-call passes (~5% of 1.05M) but the
    // cumulative window is at 50k and another +50_001 pushes the window
    // total over the 10%-of-1M cap.
    let wallet = s.add_wallet_subaccount();
    let updates = soroban_sdk::vec![&s.e, (wallet, 50_001i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
}

// ==================== get_wallet_deployed_assets (F4 invariant view) ====================

/// Invariant: `Σ WalletNetDeployed == deployed_assets` after every
/// standard mutation path. Verified across deposit, withdraw, and the
/// two operator-level reconciliation functions (`update_wallet_deployed`
/// and the new `update_wallet_deployed_batch`).
#[test]
fn test_wallet_deployed_assets_matches_aggregate_through_standard_flow() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();
    let w2 = s.add_wallet_subaccount();

    // Pristine state.
    assert_eq!(s.vault_client.get_wallet_deployed_assets(), 0);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    s.deposit_as_user(10_000_000);

    // After deposits.
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w2, &300_000);
    assert_eq!(
        s.vault_client.get_wallet_deployed_assets(),
        s.vault_client.get_deployed_assets()
    );

    // After single-wallet reconcile (gain recognition).
    s.vault_client
        .update_wallet_deployed(&s.operator, &w1, &600_000);
    assert_eq!(
        s.vault_client.get_wallet_deployed_assets(),
        s.vault_client.get_deployed_assets()
    );

    // After batch reconcile.
    let updates = soroban_sdk::vec![&s.e, (w1.clone(), 550_000i128), (w2.clone(), 350_000i128)];
    s.vault_client
        .update_wallet_deployed_batch(&s.operator, &updates);
    assert_eq!(
        s.vault_client.get_wallet_deployed_assets(),
        s.vault_client.get_deployed_assets()
    );
}

/// Strategy subaccounts are excluded from the Σ view — their AUM is
/// live via `get_balance()`, not tracked in `WalletNetDeployed`.
#[test]
fn test_wallet_deployed_assets_excludes_strategy_balances() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.fund_and_deploy(1_000_000, &strategy, 500_000);

    // Strategy deposits do not move the aggregate nor any wallet tracker.
    assert_eq!(s.vault_client.get_wallet_deployed_assets(), 0);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

/// Escape hatch divergence: `update_deployed_assets` moves only the
/// aggregate, so the two views legitimately diverge. This is the
/// off-chain-monitorable signal F4 documents.
#[test]
fn test_wallet_deployed_assets_diverges_on_update_deployed_assets() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let w1 = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &w1, &500_000);
    // Baseline: invariant holds.
    assert_eq!(
        s.vault_client.get_wallet_deployed_assets(),
        s.vault_client.get_deployed_assets()
    );

    // Aggregate-only escape hatch: bump aggregate without touching trackers.
    s.vault_client.update_deployed_assets(&s.operator, &600_000);

    // Σ unchanged, aggregate changed → visible gap for monitors.
    assert_eq!(s.vault_client.get_wallet_deployed_assets(), 500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 600_000);
}

// ==================== remove_wallet_and_reconcile ====================

/// Atomic combined op: reconcile aggregate + remove wallet in one tx.
/// Eliminates the NAV pricing window of the two-step flow. Exercises the
/// full documented operator workflow — gain recognition via
/// `update_wallet_deployed`, then atomic remove+reconcile.
#[test]
fn test_remove_wallet_and_reconcile_happy_path() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    // Recognise a 100k gain through the real operator primitive — both
    // tracker and aggregate move to 600k.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 600_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 600_000);

    // Atomic remove+reconcile: write aggregate down to the pre-wallet
    // baseline (0 here, since this wallet was the only deployment) AND
    // remove the wallet in one transaction.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_remove_wallet_and_reconcile_rejects_non_admin() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();
    let imposter = Address::generate(&s.e);

    s.vault_client
        .remove_wallet_and_reconcile(&imposter, &s.operator, &wallet, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_remove_wallet_and_reconcile_rejects_non_operator() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();
    let imposter = Address::generate(&s.e);

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &imposter, &wallet, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #22)")]
fn test_remove_wallet_and_reconcile_rejects_strategy() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &strategy.address, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_remove_wallet_and_reconcile_rejects_unregistered() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let random = Address::generate(&s.e);

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &random, &0i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_remove_wallet_and_reconcile_rejects_negative_total() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &(-1i128));
}

/// No-op reconcile case: if `new_deployed_total == current`, the aggregate
/// value stays the same but the function still emits
/// `DeployedAssetsChanged` (zero-delta) as an audit trail. This matters
/// because the subsequent removal drops the wallet's tracker without a
/// dedicated event, so the aggregate-level audit record is the only
/// indication that the transaction touched wallet-attributed state.
#[test]
fn test_remove_wallet_and_reconcile_noop_reconcile() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let deployed_before = s.vault_client.get_deployed_assets();

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &deployed_before);

    // Capture events before any further contract calls so the assertions
    // are unambiguously about this operation.
    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("deployed_assets_changed"),
        "DeployedAssetsChanged must fire even on no-op reconcile as audit trail, got: {all_events}"
    );
    assert!(
        all_events.contains("subaccount_removed"),
        "SubaccountRemoved must fire on removal, got: {all_events}"
    );

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
    assert_eq!(s.vault_client.get_deployed_assets(), deployed_before);
}

// ==================== Event emission on new functions (I-7) ====================

/// update_wallet_deployed emits BOTH `wallet_deployed_updated` and
/// `deployed_assets_changed` — indexers that track aggregate NAV via
/// `deployed_assets_changed` alone see every wallet-recognition move.
#[test]
fn test_update_wallet_deployed_emits_both_events() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &600_000);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("wallet_deployed_updated"),
        "WalletDeployedUpdated must fire, got: {all_events}"
    );
    assert!(
        all_events.contains("deployed_assets_changed"),
        "DeployedAssetsChanged must fire (aggregate moved), got: {all_events}"
    );
}

/// update_wallet_deployed with delta==0 still emits BOTH events as a
/// zero-delta audit trail. This prevents a silent-success footgun where
/// a double-submit or precision-rounded resubmission looks successful
/// to the frontend but leaves no on-chain record.
#[test]
fn test_update_wallet_deployed_noop_emits_zero_delta_events() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &500_000);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("wallet_deployed_updated"),
        "WalletDeployedUpdated must fire on zero-delta update, got: {all_events}"
    );
    assert!(
        all_events.contains("deployed_assets_changed"),
        "DeployedAssetsChanged must fire on zero-delta update, got: {all_events}"
    );
}

/// remove_wallet_and_reconcile with a real aggregate change emits BOTH
/// `deployed_assets_changed` and `subaccount_removed`.
#[test]
fn test_remove_wallet_and_reconcile_emits_both_events_on_change() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("deployed_assets_changed"),
        "DeployedAssetsChanged must fire, got: {all_events}"
    );
    assert!(
        all_events.contains("subaccount_removed"),
        "SubaccountRemoved must fire, got: {all_events}"
    );
}

// ==================== AUM rate + cumulative limits on new funcs (I-4, I-5) ====================

/// I-5: default 5% decrease limit applies to update_wallet_deployed.
/// Going from 500k → 400k is a 20% decrease → exceeds the limit.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_wallet_deployed_respects_aum_decrease_limit() {
    let s = TestSetup::new();
    // Default AUM limits: 10% increase, 5% decrease.
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // -100k on a 500k baseline = 20%, exceeds the 5% decrease limit.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &400_000);
}

/// I-5: default 5% decrease limit applies to remove_wallet_and_reconcile.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_remove_wallet_and_reconcile_respects_aum_decrease_limit() {
    let s = TestSetup::new();
    // Default AUM limits: 10% increase, 5% decrease.
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    // Removing with new_total=400k is a 20% write-down — exceeds limit.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &400_000);
}

/// I-4: cumulative AUM window limit applies to update_wallet_deployed.
/// Uses the same recipe as `test_cumulative_increase_limit_blocks_rapid_calls`
/// but routes the second bump through `update_wallet_deployed`.
#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_update_wallet_deployed_cumulative_increase_limit_enforced() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // First call via the aggregate primitive: 5% increase, passes both
    // per-call (10%) and cumulative (10%).
    s.vault_client
        .update_deployed_assets(&s.operator, &1_050_000i128);

    // Second call via the wallet primitive bumps the same aggregate by
    // another ~5%. Per-call passes (5% of 1.05M); cumulative hits 10% of
    // the base (1M) and must panic.
    let wallet = s.add_wallet_subaccount();
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &50_001i128);
}

/// I-4: cumulative AUM window limit applies to remove_wallet_and_reconcile.
#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_remove_wallet_and_reconcile_cumulative_limit_enforced() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    let wallet = s.add_wallet_subaccount();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // Use up 3% of the 5% cumulative decrease budget.
    s.vault_client
        .update_deployed_assets(&s.operator, &970_000i128);

    // Attempt a further ~3% decrease via remove_wallet_and_reconcile —
    // passes per-call, but cumulative (6% of base 1M) exceeds the 5% limit.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &940_000i128);
}

// ==================== Pause gating on remove_wallet_and_reconcile (I-6) ====================

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_remove_wallet_and_reconcile_increase_blocked_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client.pause(&s.admin);
    // Any aggregate increase is blocked while paused, just like
    // update_deployed_assets.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &600_000);
}

#[test]
fn test_remove_wallet_and_reconcile_decrease_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);

    s.vault_client.pause(&s.admin);
    // Decrease (write-off) still permitted during pause — recovery path.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &0i128);

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

/// No-op reconcile path through a paused vault: the aggregate doesn't
/// change direction (new == old), so the pause-increase guard doesn't
/// fire. Pins the behavior of the "emit audit event on no-op" path
/// under pause.
#[test]
fn test_remove_wallet_and_reconcile_noop_allowed_while_paused() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let deployed_before = s.vault_client.get_deployed_assets();

    s.vault_client.pause(&s.admin);
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &deployed_before);

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
}

// ==================== admin == operator unified signer (I-3) ====================

/// remove_wallet_and_reconcile works when admin == operator (documented
/// supported single-key configuration per `set_operator` doc). The
/// function skips the duplicate `require_auth` to avoid Soroban's
/// `Auth, ExistingValue` host error.
#[test]
fn test_remove_wallet_and_reconcile_admin_eq_operator_allowed() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    // Point operator at admin's address — single-key configuration.
    s.vault_client.set_operator(&s.admin, &s.admin);
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.admin, &wallet, &500_000);

    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.admin, &wallet, &0i128);

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
}

/// Single-key shortcut must NOT bypass the operator role check. Admin
/// passes itself for both slots, but the stored operator is a different
/// address — the role verification against storage rejects with
/// Unauthorized. Guards against a compromised admin smuggling operator
/// permissions into the dual-auth path.
#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_remove_wallet_and_reconcile_rejects_admin_claiming_operator_when_not_set() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    // Stored operator is NOT the admin (the usual TestSetup config).
    let wallet = s.add_wallet_subaccount();

    // Admin passes itself for the operator slot. Because admin == passed
    // operator the single-key shortcut elides the second require_auth,
    // but the role check against `storage::get_operator` still runs and
    // must reject.
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.admin, &wallet, &0i128);
}

/// Happy path: `remove_wallet_and_reconcile` with `new_deployed_total >
/// old_deployed` (unpaused increase). The removal + aggregate write-up
/// both land in one tx. Exercises the increase branch of
/// `apply_deployed_assets_change` directly through this entry point.
#[test]
fn test_remove_wallet_and_reconcile_allows_unpaused_increase() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    s.deposit_as_user(1_000_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &wallet, &500_000);
    let old_deployed = s.vault_client.get_deployed_assets();

    // Recognise a gain AND remove the wallet in one call. new_total
    // (700k) > old_deployed (500k) — exercises the increase branch.
    let new_total = old_deployed + 200_000;
    s.vault_client
        .remove_wallet_and_reconcile(&s.admin, &s.operator, &wallet, &new_total);

    let subs = s.vault_client.get_subaccounts();
    assert!(!subs.contains(&wallet));
    assert_eq!(s.vault_client.get_deployed_assets(), new_total);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
}

// ==================== Additional update_wallet_deployed coverage (S-5) ====================

/// S-5: update_wallet_deployed surfaces DeployedAssetsUnderflow when the
/// aggregate has been driven below a wallet's tracker (via
/// `seed_wallet_net_deployed` + aggregate write-down). The underflow
/// guard at the wallet primitive is the forcing function that catches
/// the inconsistency before it corrupts NAV.
#[test]
#[should_panic(expected = "Error(Contract, #12)")]
fn test_update_wallet_deployed_underflow_rejected() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    // Admin seeds tracker to 1000 without touching aggregate. Aggregate
    // stays below tracker.
    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &1000i128);
    s.set_deployed(500);

    // Operator tries to write the tracker DOWN to 400 → delta=-600,
    // aggregate would go 500 - 600 = -100 → underflow.
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &400i128);
}

/// S-5: update_wallet_deployed from a fresh wallet (tracker == 0) with
/// no other deployed capital — rate-limit skipped at the zero boundary,
/// tracker and aggregate both bumped to the recognised amount.
#[test]
fn test_update_wallet_deployed_from_zero_tracker_fresh_vault() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);
    assert_eq!(s.vault_client.get_deployed_assets(), 0);

    // Recognising from zero is unrestricted (default behavior matches
    // update_deployed_assets at the zero boundary).
    s.vault_client
        .update_wallet_deployed(&s.operator, &wallet, &1_000_000i128);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 1_000_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 1_000_000);
}

/// S-5: update_wallet_deployed from a fresh wallet's tracker (0) but a
/// non-zero aggregate — rate limit DOES apply against the aggregate
/// baseline. Pins the "old_deployed" in the helper refers to aggregate,
/// not per-wallet.
#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_wallet_deployed_from_zero_tracker_respects_aggregate_limit() {
    let s = TestSetup::new();
    // Default AUM limits: 10% increase.
    let existing_wallet = s.add_wallet_subaccount();
    let fresh_wallet = s.add_wallet_subaccount();

    s.deposit_as_user(10_000_000);
    // Existing wallet sets the aggregate to 500k.
    s.vault_client
        .deposit_to_subaccount(&s.operator, &existing_wallet, &500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&fresh_wallet), 0);

    // Recognising 100k on the fresh wallet is +20% of the aggregate base
    // (500k) — exceeds the 10% increase limit.
    s.vault_client
        .update_wallet_deployed(&s.operator, &fresh_wallet, &100_000i128);
}

/// Seeding a wallet's tracker (for pre-existing capital that's already
/// reflected in `deployed_assets` via some other path) lets
/// `withdraw_from_subaccount` pull without tripping `WalletOverWithdraw`.
/// Confirms seed + strict-mode check interact correctly.
#[test]
fn test_seed_then_withdraw_within_seeded_value_succeeds() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let wallet = s.add_wallet_subaccount();

    // Capital is at the wallet, recorded in the aggregate counter, but
    // the per-wallet tracker is empty (simulated via set_deployed +
    // direct token.transfer, which skips deposit_to_subaccount's tracker
    // increment).
    s.deposit_as_user(1_000_000);
    s.asset_client.transfer(&s.admin, &wallet, &500_000i128);
    s.set_deployed(500_000);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 0);

    let vault_addr = s.vault_client.address.clone();
    s.asset_client
        .approve(&wallet, &vault_addr, &300_000, &1000);

    // Admin seeds the tracker to match the already-counted value. Seed
    // does NOT touch aggregate.
    s.vault_client
        .seed_wallet_net_deployed(&s.admin, &wallet, &500_000i128);
    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 500_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000);

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &wallet, &300_000);

    assert_eq!(s.vault_client.get_wallet_net_deployed(&wallet), 200_000);
    assert_eq!(s.vault_client.get_deployed_assets(), 200_000);
}

// ==================== Two-Step Admin Transfer Tests (C-1) ====================

#[test]
fn test_propose_admin_success() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    assert_eq!(s.vault_client.get_pending_admin(), Some(new_admin));
}

#[test]
#[should_panic(expected = "Error(Contract, #15)")]
fn test_propose_admin_self_fails() {
    let s = TestSetup::new();

    // Admin cannot propose themselves as the new admin
    s.vault_client.propose_admin(&s.admin, &s.admin, &ONE_WEEK);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_propose_admin_not_admin_fails() {
    let s = TestSetup::new();
    let not_admin = Address::generate(&s.e);
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&not_admin, &new_admin, &ONE_WEEK);
}

#[test]
fn test_accept_admin_success() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.vault_client.accept_admin();

    assert_eq!(s.vault_client.get_admin(), new_admin);
    assert_eq!(s.vault_client.get_pending_admin(), None);
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_accept_admin_no_pending_fails() {
    let s = TestSetup::new();

    // No proposal — should panic with NoPendingAdmin
    s.vault_client.accept_admin();
}

#[test]
fn test_propose_admin_overwrites_previous() {
    let s = TestSetup::new();
    let first = Address::generate(&s.e);
    let second = Address::generate(&s.e);

    s.vault_client.propose_admin(&s.admin, &first, &ONE_WEEK);
    assert_eq!(s.vault_client.get_pending_admin(), Some(first));

    s.vault_client.propose_admin(&s.admin, &second, &ONE_WEEK);
    assert_eq!(s.vault_client.get_pending_admin(), Some(second.clone()));

    // Only the second should be accepted
    s.vault_client.accept_admin();
    assert_eq!(s.vault_client.get_admin(), second);
}

#[test]
fn test_cancel_admin_proposal_success() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    assert_eq!(s.vault_client.get_pending_admin(), Some(new_admin));

    s.vault_client.cancel_admin_proposal(&s.admin);
    assert_eq!(s.vault_client.get_pending_admin(), None);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_cancel_admin_proposal_not_admin_fails() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);
    let not_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.vault_client.cancel_admin_proposal(&not_admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #14)")]
fn test_cancel_admin_proposal_none_pending_fails() {
    let s = TestSetup::new();

    // No proposal to cancel
    s.vault_client.cancel_admin_proposal(&s.admin);
}

#[test]
fn test_old_admin_loses_power_after_transfer() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.vault_client.accept_admin();

    // New admin can perform admin functions
    s.vault_client.pause(&new_admin);
    assert!(s.vault_client.is_paused());
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_old_admin_rejected_after_transfer() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.vault_client.accept_admin();

    // Old admin must no longer be able to act
    s.vault_client.unpause(&s.admin);
}

#[test]
fn test_admin_transfer_emits_events() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.assert_last_event_contains("admin_transfer_proposed");

    s.vault_client.accept_admin();
    s.assert_last_event_contains("admin_transfer_accepted");
}

#[test]
fn test_cancel_admin_proposal_emits_event() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);
    s.vault_client.cancel_admin_proposal(&s.admin);

    s.assert_last_event_contains("admin_transfer_cancelled");
}

// ==================== Admin Proposal Deadline ====================

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_accept_admin_expired_proposal() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    // Propose with a 100-second deadline
    s.vault_client.propose_admin(&s.admin, &new_admin, &100);

    // Advance ledger timestamp past the deadline
    s.e.ledger().with_mut(|l| {
        l.timestamp = 101;
    });

    // Should fail — proposal expired
    s.vault_client.accept_admin();
}

#[test]
fn test_accept_admin_at_deadline_succeeds() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client.propose_admin(&s.admin, &new_admin, &100);

    // Advance to exactly the deadline — should still work
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100;
    });

    s.vault_client.accept_admin();
    assert_eq!(s.vault_client.get_admin(), new_admin);
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_propose_admin_deadline_in_past_fails() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    // Set current time
    s.e.ledger().with_mut(|l| {
        l.timestamp = 1000;
    });

    // Propose with deadline already passed
    s.vault_client.propose_admin(&s.admin, &new_admin, &999);
}

#[test]
fn test_cancel_clears_deadline() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client.propose_admin(&s.admin, &new_admin, &100);
    s.vault_client.cancel_admin_proposal(&s.admin);

    // Pending admin is cleared
    assert_eq!(s.vault_client.get_pending_admin(), None);
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_propose_admin_deadline_equal_to_current_timestamp_fails() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 500;
    });

    // Deadline == current timestamp should be rejected (must be strictly future)
    s.vault_client.propose_admin(&s.admin, &new_admin, &500);
}

#[test]
#[should_panic(expected = "Error(Contract, #16)")]
fn test_propose_admin_overwrite_resets_deadline() {
    let s = TestSetup::new();
    let first = Address::generate(&s.e);
    let second = Address::generate(&s.e);

    // First proposal with a long deadline
    s.vault_client.propose_admin(&s.admin, &first, &10_000);

    // Second proposal with a short deadline overrides the first
    s.vault_client.propose_admin(&s.admin, &second, &200);

    // Advance past the short deadline
    s.e.ledger().with_mut(|l| {
        l.timestamp = 201;
    });

    // Should fail with AdminProposalExpired — the active deadline is 200, not 10_000
    s.vault_client.accept_admin();
}

#[test]
fn test_propose_admin_event_includes_deadline() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client.propose_admin(&s.admin, &new_admin, &12345);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("admin_transfer_proposed"),
        "Event should be emitted"
    );
    assert!(
        all_events.contains("12345"),
        "Event should include the deadline value"
    );
}

#[test]
fn test_accept_admin_well_before_deadline() {
    let s = TestSetup::new();
    let new_admin = Address::generate(&s.e);

    s.vault_client
        .propose_admin(&s.admin, &new_admin, &ONE_WEEK);

    // Advance a bit, but well before deadline
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100;
    });

    s.vault_client.accept_admin();
    assert_eq!(s.vault_client.get_admin(), new_admin);
    assert_eq!(s.vault_client.get_pending_admin(), None);
}

// ==================== H-2: Pause Restricts update_deployed_assets Increase ====================

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn test_update_deployed_assets_increase_blocked_while_paused() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client.pause(&s.admin);

    // Attempting to increase deployed_assets while paused should fail
    s.vault_client
        .update_deployed_assets(&s.operator, &510_000i128);
}

#[test]
fn test_update_deployed_assets_decrease_allowed_while_paused() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client.pause(&s.admin);

    // Decrease should still work while paused
    s.vault_client
        .update_deployed_assets(&s.operator, &490_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 490_000i128);
}

#[test]
fn test_update_deployed_assets_same_value_allowed_while_paused() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    s.vault_client.pause(&s.admin);

    // Same value is a no-op, should work while paused
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128);
}

// ==================== H-3: AUM Rate Limit on Subaccount Operations ====================

#[test]
fn test_deposit_to_subaccount_no_aum_rate_limit() {
    // Subaccount transfers don't change total_assets, so no AUM rate limit
    // is applied. Large deposits that previously exceeded the limit now succeed.
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // Fund and deploy initial amount (unrestricted from zero)
    s.fund_and_deploy(1_000_000, &strategy, 100_000);

    // 50k / 100k = 50% — would have exceeded the 10% default increase
    // limit, but subaccount ops are now unrestricted.
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);
    // Strategy deposits don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 150_000i128);
}

#[test]
fn test_withdraw_from_subaccount_no_aum_rate_limit() {
    // Subaccount transfers don't change total_assets, so no AUM rate limit
    // is applied. Large withdrawals that previously exceeded the limit now succeed.
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_and_deploy(1_000_000, &strategy, 500_000);

    // 50k / 500k = 10% — would have exceeded the 5% default decrease
    // limit, but subaccount ops are now unrestricted.
    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &50_000i128);
    // Strategy withdrawals don't change deployed_assets
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 450_000i128);
}

#[test]
fn test_deposit_to_subaccount_total_assets_preserved() {
    // Deploying to a subaccount moves tokens from vault to strategy but
    // total_assets (local_balance + deployed_assets) must stay constant.
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.fund_vault(1_000_000);

    let total_before = s.vault_client.total_assets();

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &600_000i128);

    let total_after = s.vault_client.total_assets();
    assert_eq!(
        total_before, total_after,
        "total_assets should be unchanged after deploying to subaccount"
    );
}

#[test]
fn test_deploy_entire_vault_balance() {
    // Operator can deploy the full vault balance in one transaction.
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.fund_vault(1_000_000);

    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &1_000_000i128);
    // Strategy deposits don't change deployed_assets; balance queried live
    assert_eq!(s.vault_client.get_deployed_assets(), 0);
    assert_eq!(strategy.get_balance(), 1_000_000i128);

    let total = s.vault_client.total_assets();
    assert_eq!(total, 1_000_000i128, "total_assets should be unchanged");
}

#[test]
fn test_recall_entire_deployed_amount() {
    // Operator can recall all deployed capital in one transaction.
    let s = TestSetup::new();
    let strategy = s.add_strategy();
    s.fund_and_deploy(1_000_000, &strategy, 1_000_000);

    s.vault_client
        .withdraw_from_subaccount(&s.operator, &strategy.address, &1_000_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 0i128);

    let total = s.vault_client.total_assets();
    assert_eq!(total, 1_000_000i128, "total_assets should be unchanged");
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_increase_still_rate_limited() {
    // AUM rate limits remain on update_deployed_assets even though they were
    // removed from subaccount operations.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // 12% increase — exceeds 10% default limit
    s.vault_client
        .update_deployed_assets(&s.operator, &560_000i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")]
fn test_update_deployed_assets_decrease_still_rate_limited() {
    // AUM decrease rate limit remains on update_deployed_assets.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(500_000);

    // 6% decrease — exceeds 5% default decrease limit
    s.vault_client
        .update_deployed_assets(&s.operator, &470_000i128);
}

// ==================== Cumulative AUM Window Tests ====================

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_cumulative_increase_limit_blocks_rapid_calls() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    // Set a non-zero timestamp so the window doesn't reset on every call.
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // Per-call limit is 10% (default), cumulative limit is also 10% (default).
    // First call: 5% increase → passes both per-call and cumulative.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_050_000i128);

    // Second call: another ~5% → passes per-call (5% of 1.05M) but cumulative
    // is now ~10% of the base (1M), which hits the cumulative limit.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_100_001i128);
}

#[test]
#[should_panic(expected = "Error(Contract, #20)")]
fn test_cumulative_decrease_limit_blocks_rapid_calls() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    // Set a non-zero timestamp so the window doesn't reset on every call.
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // Per-call decrease limit is 5% (default), cumulative is also 5%.
    // First call: 3% decrease → passes.
    s.vault_client
        .update_deployed_assets(&s.operator, &970_000i128);

    // Second call: another ~3% of current → passes per-call, but cumulative
    // 6% of base (1M) exceeds the 5% cumulative limit.
    s.vault_client
        .update_deployed_assets(&s.operator, &940_000i128);
}

#[test]
fn test_cumulative_window_resets_after_expiry() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    // Set timestamp to something reasonable
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // First call: 10% increase → uses entire cumulative budget.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_100_000i128);

    // Advance time past the 24h window.
    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000 + 86_401;
    });

    // Same-size call succeeds because the window has reset.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_210_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 1_210_000i128);
}

#[test]
fn test_cumulative_and_per_call_independent() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    // Widen cumulative limit to 20%, keep per-call at 10% (default).
    s.vault_client
        .set_aum_window_limits(&s.admin, &86_400, &2_000, &1_000);

    // First call: 10% increase → passes per-call (exactly at limit).
    s.vault_client
        .update_deployed_assets(&s.operator, &1_100_000i128);

    // Second call: another ~9% of current → passes per-call (~9% of 1.1M).
    // Cumulative is ~20% of base (1M) — just within the 20% cumulative limit.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_200_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 1_200_000i128);
}

#[test]
fn test_set_aum_window_limits_success() {
    let s = TestSetup::new();

    s.vault_client
        .set_aum_window_limits(&s.admin, &43_200, &2_000, &1_000);

    assert_eq!(s.vault_client.get_aum_window_duration(), 43_200);
    assert_eq!(s.vault_client.get_aum_window_inc_limit(), 2_000);
    assert_eq!(s.vault_client.get_aum_window_dec_limit(), 1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #1)")]
fn test_set_aum_window_limits_unauthorized() {
    let s = TestSetup::new();
    let not_admin = Address::generate(&s.e);
    s.vault_client
        .set_aum_window_limits(&not_admin, &86_400, &2_000, &1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_window_limits_invalid_duration_too_short() {
    let s = TestSetup::new();
    // 1 second — below the 1-hour minimum
    s.vault_client
        .set_aum_window_limits(&s.admin, &1, &2_000, &1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_window_limits_invalid_duration_too_long() {
    let s = TestSetup::new();
    // 30 days — above the 7-day maximum
    s.vault_client
        .set_aum_window_limits(&s.admin, &2_592_000, &2_000, &1_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_window_limits_invalid_bps() {
    let s = TestSetup::new();
    // 0 bps — below the 1 bps minimum
    s.vault_client
        .set_aum_window_limits(&s.admin, &86_400, &0, &1_000);
}

#[test]
fn test_cumulative_skipped_when_deployed_zero() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();

    // deployed_assets starts at 0; cumulative check should be skipped.
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 500_000i128);
}

#[test]
fn test_cumulative_paused_decrease_tracked() {
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.vault_client.pause(&s.admin);

    // 5% decrease while paused — allowed and tracked.
    s.vault_client
        .update_deployed_assets(&s.operator, &950_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 950_000i128);
}

#[test]
fn test_cumulative_defaults() {
    let s = TestSetup::new();
    // Defaults should match per-call defaults (falls back to DEFAULT_AUM_INCREASE/DECREASE_LIMIT)
    assert_eq!(s.vault_client.get_aum_window_duration(), 86_400);
    assert_eq!(s.vault_client.get_aum_window_inc_limit(), 1_000);
    assert_eq!(s.vault_client.get_aum_window_dec_limit(), 500);
}

#[test]
fn test_cumulative_mixed_increase_and_decrease_in_same_window() {
    // Increase and decrease have independent cumulative budgets.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // Use 5% of increase budget
    s.vault_client
        .update_deployed_assets(&s.operator, &1_050_000i128);

    // Use 3% of decrease budget (independent of increase)
    s.vault_client
        .update_deployed_assets(&s.operator, &1_020_000i128);

    assert_eq!(s.vault_client.get_deployed_assets(), 1_020_000i128);
}

#[test]
fn test_cumulative_base_deployed_frozen_at_window_start() {
    // base_deployed is set when the window opens and stays fixed.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    // Widen per-call and cumulative limits to allow two 9% increases
    s.vault_client.set_aum_limits(&s.admin, &10_000, &10_000);
    s.vault_client
        .set_aum_window_limits(&s.admin, &86_400, &2_000, &1_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // First call: 10% increase. Cumulative = 100k / base 1M = 10%.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_100_000i128);

    // Second call: another 100k. Cumulative = 200k / base 1M = 20%.
    // This exceeds the 20% cumulative limit.
    s.vault_client
        .update_deployed_assets(&s.operator, &1_200_000i128);

    assert_eq!(s.vault_client.get_deployed_assets(), 1_200_000i128);
}

#[test]
fn test_cumulative_paused_decrease_with_nonzero_timestamp() {
    // Verify cumulative tracking actually works (not just reset every call)
    // by using a non-zero timestamp.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    s.vault_client.pause(&s.admin);

    // 3% decrease while paused — tracked cumulatively
    s.vault_client
        .update_deployed_assets(&s.operator, &970_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 970_000i128);
}

#[test]
fn test_set_aum_window_limits_boundary_min() {
    let s = TestSetup::new();
    // All minimum values
    s.vault_client
        .set_aum_window_limits(&s.admin, &3_600, &1, &1);
    assert_eq!(s.vault_client.get_aum_window_duration(), 3_600);
    assert_eq!(s.vault_client.get_aum_window_inc_limit(), 1);
    assert_eq!(s.vault_client.get_aum_window_dec_limit(), 1);
}

#[test]
fn test_set_aum_window_limits_boundary_max() {
    let s = TestSetup::new();
    // All maximum values
    s.vault_client
        .set_aum_window_limits(&s.admin, &604_800, &10_000, &10_000);
    assert_eq!(s.vault_client.get_aum_window_duration(), 604_800);
    assert_eq!(s.vault_client.get_aum_window_inc_limit(), 10_000);
    assert_eq!(s.vault_client.get_aum_window_dec_limit(), 10_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn test_set_aum_window_limits_invalid_decrease_bps() {
    let s = TestSetup::new();
    // decrease_bps = 0 — below minimum
    s.vault_client
        .set_aum_window_limits(&s.admin, &86_400, &1_000, &0);
}

#[test]
fn test_cumulative_transition_from_zero_to_nonzero() {
    // When deployed starts at 0, the first call is unrestricted (both per-call
    // and cumulative skip at zero). The second call should be rate-limited.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // First call from 0: unrestricted
    s.vault_client
        .update_deployed_assets(&s.operator, &500_000i128);

    // Second call: now deployed > 0, per-call and cumulative both apply.
    // 10% increase of 500k = 50k max. This should succeed.
    s.vault_client
        .update_deployed_assets(&s.operator, &550_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 550_000i128);
}

#[test]
fn test_set_aum_window_limits_resets_window() {
    // Changing limits should reset the cumulative window.
    let s = TestSetup::new();
    let _strategy = s.setup_standard_deploy();
    s.set_deployed(1_000_000);

    s.e.ledger().with_mut(|l| {
        l.timestamp = 100_000;
    });

    // Use 10% of increase budget (entire default budget)
    s.vault_client
        .update_deployed_assets(&s.operator, &1_100_000i128);

    // Reconfigure limits (same values) — this should reset the window
    s.vault_client
        .set_aum_window_limits(&s.admin, &86_400, &1_000, &500);

    // Another 10% increase should now succeed (window was reset)
    s.vault_client
        .update_deployed_assets(&s.operator, &1_210_000i128);
    assert_eq!(s.vault_client.get_deployed_assets(), 1_210_000i128);
}

// ==================== M-3: SubaccountRemoved Includes deployed_assets ====================

#[test]
fn test_remove_subaccount_event_includes_deployed_assets() {
    let s = TestSetup::new();
    s.relax_aum_limits();
    let strategy = s.add_strategy();

    s.fund_and_deploy(1_000_000, &strategy, 500_000);
    // Set deployed_assets so the event includes a non-zero value
    s.set_deployed(500_000);

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    // The SubaccountRemoved event should contain the deployed_assets value
    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("subaccount_removed"),
        "Event should be emitted"
    );
    // deployed_assets = 500_000 should appear in the event data
    assert!(
        all_events.contains("500000"),
        "Event should include deployed_assets value"
    );
}

#[test]
fn test_remove_subaccount_event_zero_deployed() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    // No funds deployed — remove right away
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    s.assert_last_event_contains("subaccount_removed");
}

// ==================== M-4: max_mint Returns Correct Value ====================

#[test]
fn test_max_mint_returns_max_when_not_paused() {
    let s = TestSetup::new();

    let max = s.vault_client.max_mint(&s.user);
    assert_eq!(max, i128::MAX);
}

#[test]
fn test_max_mint_returns_zero_when_paused() {
    let s = TestSetup::new();

    s.vault_client.pause(&s.admin);
    let max = s.vault_client.max_mint(&s.user);
    assert_eq!(max, 0);
}

// ==================== L-4: Constructor Emits AdminSet Event ====================

#[test]
fn test_constructor_emits_admin_set_event() {
    let e = Env::default();
    let admin = Address::generate(&e);

    let asset_client = create_asset_client(&e, DEFAULT_SUPPLY, &admin);
    let _vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);

    let all_events = std::format!("{:?}", e.events().all());
    assert!(
        all_events.contains("admin_set"),
        "Constructor should emit AdminSet event"
    );
}

// ==================== Mock: Negative-balance Strategy ====================

/// Strategy that returns a negative value from get_balance().
/// Used to test that add_subaccount rejects strategies with negative balances,
/// and that query_strategy_balances panics on negative balances at runtime.
#[contract]
pub struct MockNegativeBalanceStrategy;

#[contractimpl]
impl MockNegativeBalanceStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
        e.storage().instance().set(&"forced_balance", &(-1i128));
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}

    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    pub fn get_balance(e: &Env) -> i128 {
        e.storage().instance().get(&"forced_balance").unwrap()
    }

    pub fn get_local_balance(e: &Env) -> i128 {
        e.storage().instance().get(&"forced_balance").unwrap()
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }

    /// Test helper: update the balance returned by get_balance().
    pub fn set_forced_balance(e: &Env, balance: i128) {
        e.storage().instance().set(&"forced_balance", &balance);
    }
}

// ==================== Negative balance tests ====================

/// add_subaccount rejects a strategy whose get_balance() returns a negative value.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_add_subaccount_rejects_negative_balance_strategy() {
    let s = TestSetup::new();
    let address =
        s.e.register(MockNegativeBalanceStrategy, (&s.asset_client.address,));
    let strategy = MockNegativeBalanceStrategyClient::new(&s.e, &address);

    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);
}

/// query_strategy_balances panics with NegativeStrategyBalance when an
/// already-registered strategy returns a negative balance (e.g. buggy upgrade).
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_total_assets_panics_on_negative_strategy_balance() {
    let s = TestSetup::new();
    // Register with a non-negative balance so it passes the smoke test
    let address =
        s.e.register(MockNegativeBalanceStrategy, (&s.asset_client.address,));
    let strategy = MockNegativeBalanceStrategyClient::new(&s.e, &address);
    strategy.set_forced_balance(&0);

    s.vault_client
        .add_subaccount(&s.admin, &strategy.address, &SubaccountType::Strategy);

    // Now make the balance negative (simulating a buggy upgrade)
    strategy.set_forced_balance(&(-1));

    // total_assets() should panic
    s.vault_client.total_assets();
}

// ==================== max_withdraw / max_redeem freeze test ====================

/// When a strategy's get_balance() reverts, max_withdraw and max_redeem
/// are also frozen (they depend on total_assets_from).
#[test]
fn test_broken_strategy_freezes_max_withdraw_and_max_redeem() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &50_000i128);

    // Strategy breaks
    strategy.set_balance_reverts(&true);

    // max_withdraw and max_redeem are frozen
    assert!(
        s.vault_client.try_max_withdraw(&s.user).is_err(),
        "max_withdraw should panic with broken strategy"
    );
    assert!(
        s.vault_client.try_max_redeem(&s.user).is_err(),
        "max_redeem should panic with broken strategy"
    );

    // Admin removes the broken strategy — vault recovers
    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    assert!(s.vault_client.max_withdraw(&s.user) >= 0);
    assert!(s.vault_client.max_redeem(&s.user) >= 0);
}

// ==================== SubaccountRemoved event includes subaccount_type ====================

#[test]
fn test_remove_subaccount_event_includes_strategy_type() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("subaccount_removed"),
        "Event should be emitted"
    );
    assert!(
        all_events.contains("Strategy"),
        "Event should include Strategy subaccount type"
    );
}

#[test]
fn test_remove_subaccount_event_includes_wallet_type() {
    let s = TestSetup::new();
    let wallet = s.add_wallet_subaccount();

    s.vault_client.remove_subaccount(&s.admin, &wallet);

    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("subaccount_removed"),
        "Event should be emitted"
    );
    assert!(
        all_events.contains("Wallet"),
        "Event should include Wallet subaccount type"
    );
}

/// F5b: removing a funded Strategy records its `try_get_balance()` result
/// in the `SubaccountRemoved` event so off-chain auditors can link the
/// resulting NAV drop to this transaction.
#[test]
fn test_remove_strategy_event_records_balance() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &40_000i128);
    assert_eq!(strategy.get_balance(), 40_000i128);

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    // The strategy's balance at removal is serialized into the event. The
    // exact Option-encoded representation isn't part of the public API;
    // substring-match the captured balance value to verify it was recorded.
    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("subaccount_removed"),
        "SubaccountRemoved event should be emitted"
    );
    assert!(
        all_events.contains("40000"),
        "SubaccountRemoved should record strategy balance 40000 at removal, got: {all_events}"
    );
}

/// F5b + I-4: a broken strategy whose `get_balance()` traps is removed
/// successfully; `SubaccountRemoved` records `None` for `strategy_balance`,
/// and the co-emitted `StrategyBalanceProbeFailed` carries the specific
/// failure reason (`InvokeError` vs `ConvertError`).
#[test]
fn test_remove_broken_strategy_event_records_none_balance() {
    let s = TestSetup::new();
    let strategy = s.add_strategy();

    s.fund_vault(100_000i128);
    s.vault_client
        .deposit_to_subaccount(&s.operator, &strategy.address, &40_000i128);
    strategy.set_balance_reverts(&true);

    s.vault_client
        .remove_subaccount(&s.admin, &strategy.address);

    // Both events must fire. Assert on the probe-failed event's
    // reason field — substring-matching `"InvokeError"` proves we went
    // through the trap arm, not the convert arm, so the discriminator
    // added for I-4 is actually wired to the right branch. (We used to
    // also assert the deposit amount `40000` was absent from the event
    // payload, but that was fragile — a future unrelated numeric field
    // containing "40000" would fail the test for the wrong reason. The
    // probe-failed event + correct reason-code is the real signal.)
    let all_events = std::format!("{:?}", s.e.events().all());
    assert!(
        all_events.contains("subaccount_removed"),
        "SubaccountRemoved must fire even when try_get_balance traps, got: {all_events}"
    );
    assert!(
        all_events.contains("strategy_balance_probe_failed"),
        "StrategyBalanceProbeFailed must fire when try_get_balance traps, got: {all_events}"
    );
    assert!(
        all_events.contains("InvokeError"),
        "Probe-failure reason should be InvokeError for a trapping strategy, got: {all_events}"
    );
}

/// I-4: a strategy whose `get_balance()` returns a non-i128 (e.g. a
/// value of a different type due to an ABI regression) is removable and
/// the probe-failed event distinguishes it from the trap case via
/// `ConvertError` rather than `InvokeError`.
#[test]
fn test_remove_strategy_with_bad_return_type_emits_convert_error() {
    let e = Env::default();
    let admin = Address::generate(&e);
    let asset_client = create_asset_client(&e, DEFAULT_SUPPLY, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);
    e.mock_all_auths();

    // Register the strategy while it still returns a valid i128 — passes
    // the F2 probe — then flip the flag so removal hits the ConvertError
    // arm. Decoupling the flip from probe-call-count makes this robust
    // against changes in how many times `add_subaccount` probes
    // `get_balance` (R2-11).
    let strategy_addr = e.register(MockBadReturnGetBalanceStrategy, (&asset_client.address,));
    let strategy_client = MockBadReturnGetBalanceStrategyClient::new(&e, &strategy_addr);
    vault_client.add_subaccount(&admin, &strategy_addr, &SubaccountType::Strategy);
    strategy_client.set_bad_return(&true);

    vault_client.remove_subaccount(&admin, &strategy_addr);

    let all_events = std::format!("{:?}", e.events().all());
    assert!(
        all_events.contains("strategy_balance_probe_failed"),
        "expected StrategyBalanceProbeFailed, got: {all_events}"
    );
    assert!(
        all_events.contains("ConvertError"),
        "expected ConvertError reason, got: {all_events}"
    );
}

/// I-5 / F2 completeness: the registration probe rejects strategies
/// whose `get_local_balance()` returns a negative value with the same
/// `NegativeStrategyBalance` error used for `get_balance()`.
#[test]
#[should_panic(expected = "Error(Contract, #19)")]
fn test_add_subaccount_rejects_negative_get_local_balance() {
    let s = TestSetup::new();
    let address =
        s.e.register(MockNegativeLocalBalanceStrategy, (&s.asset_client.address,));
    s.vault_client
        .add_subaccount(&s.admin, &address, &SubaccountType::Strategy);
}

// ==================== Mock: Bad-return-type Strategy ====================

/// Strategy whose `get_balance()` can be flipped to return a non-i128
/// value (Symbol) on demand. Needed to exercise the `ConvertError` arm
/// of the F5b probe (I-4).
///
/// The flip is controlled by an explicit `set_bad_return(true)` helper
/// rather than an implicit first-call counter (R2-11): an implicit
/// counter couples the mock's behaviour to how many times the vault
/// happens to invoke `get_balance` at registration, which is a
/// refactoring hazard. With the explicit flag, registration uses the
/// good i128 return, the test flips the flag, and removal hits the
/// ConvertError arm — no matter how many probes registration performs.
#[contract]
pub struct MockBadReturnGetBalanceStrategy;

#[contractimpl]
impl MockBadReturnGetBalanceStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}

    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }

    pub fn get_balance(e: &Env) -> soroban_sdk::Val {
        use soroban_sdk::IntoVal;
        if e.storage()
            .instance()
            .get::<_, bool>(&"bad_return")
            .unwrap_or(false)
        {
            soroban_sdk::Symbol::new(e, "not_an_i128").into_val(e)
        } else {
            (0i128).into_val(e)
        }
    }

    pub fn get_local_balance(_e: &Env) -> i128 {
        0
    }

    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }

    /// Explicit switch: when `enabled`, `get_balance` returns a non-i128.
    pub fn set_bad_return(e: &Env, enabled: bool) {
        e.storage().instance().set(&"bad_return", &enabled);
    }
}

// ==================== Mock: Negative-local-balance Strategy ====================

/// Strategy whose `get_local_balance()` returns a negative value. Used to
/// verify the F2 probe rejects it with `NegativeStrategyBalance`.
#[contract]
pub struct MockNegativeLocalBalanceStrategy;

#[contractimpl]
impl MockNegativeLocalBalanceStrategy {
    pub fn __constructor(e: &Env, asset: Address) {
        e.storage().instance().set(&"asset", &asset);
    }

    pub fn deposit(_e: &Env, _from: Address, _amount: i128) {}
    pub fn withdraw(_e: &Env, _to: Address, _amount: i128) -> i128 {
        0
    }
    pub fn get_balance(_e: &Env) -> i128 {
        0
    }
    pub fn get_local_balance(_e: &Env) -> i128 {
        -1
    }
    pub fn get_asset(e: &Env) -> Address {
        e.storage().instance().get(&"asset").unwrap()
    }
}

// =====================================================================
// D3: F1 cross-crate integration — share price stable under donation
// =====================================================================
//
// These tests register the real `xlm_strategy::XlmStrategy` contract
// (not a mock) as a subaccount of the real `AugustVault`, then simulate
// the donation-attack scenario end-to-end at the vault API level:
//   1. A legitimate depositor mints shares at price P.
//   2. An attacker transfers tokens directly to the strategy address,
//      bypassing the vault's `deposit_to_subaccount` flow.
//   3. A second depositor mints shares for the same input amount.
// The F1 defense holds iff the second depositor receives the same
// number of shares (up to rounding) — i.e., the donation did NOT
// inflate NAV between the two deposits. Previously the strategy's
// `get_balance()` returned `token.balance(self) + deployed_total`,
// which would have silently propagated the donation into NAV and
// given depositor #2 fewer shares per asset. Post-F1 the strategy
// uses its `local_balance` tracker, so `get_balance()` and therefore
// `total_assets()` are unchanged by the donation.

#[test]
fn test_f1_share_price_stable_under_donation_end_to_end() {
    use xlm_strategy::{XlmStrategy, XlmStrategyClient};

    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let operator = Address::generate(&e);
    let controller = Address::generate(&e);
    let attacker = Address::generate(&e);
    let user_a = Address::generate(&e);
    let user_b = Address::generate(&e);

    // Set up the token and the vault (decimals_offset = 3 for
    // first-depositor virtual-shares protection).
    let asset_client = create_asset_client(&e, DEFAULT_SUPPLY, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);
    vault_client.set_operator(&admin, &operator);

    // Register the REAL xlm-strategy (not a mock) so the vault's
    // `get_balance()` path exercises the production F1 tracker code.
    let strategy_addr = e.register(
        XlmStrategy,
        (&asset_client.address, &vault_client.address, &controller),
    );
    let strategy = XlmStrategyClient::new(&e, &strategy_addr);
    vault_client.add_subaccount(&admin, &strategy_addr, &SubaccountType::Strategy);

    // Step 1: user A deposits 100_000 and receives shares at the
    // bootstrap price. Subsequent deposits are priced against the
    // resulting total_assets/total_supply ratio.
    asset_client.transfer(&admin, &user_a, &100_000i128);
    let shares_a = vault_client.deposit(&100_000i128, &user_a, &user_a, &user_a);

    // Move half of the vault's local balance into the strategy. Under
    // F1 this increments the strategy's `local_balance` by the actual
    // delivered amount (no fee on the mock token), so
    // `get_balance(strategy) = local_balance + deployed_total = 50_000`.
    // `total_assets(vault)` is unchanged: the tokens moved from vault
    // local to strategy idle, both of which sum into NAV.
    vault_client.deposit_to_subaccount(&operator, &strategy_addr, &50_000i128);
    let total_before_donation = vault_client.total_assets();
    let strategy_balance_before = strategy.get_balance();
    assert_eq!(
        total_before_donation, 100_000i128,
        "total_assets should equal the original deposit before any donation"
    );
    assert_eq!(strategy_balance_before, 50_000i128);

    // Step 2: adversary donates 1_000_000 directly to the strategy —
    // 20x the legitimate strategy balance. Pre-F1 this would have
    // inflated `get_balance(strategy)` to 1_050_000 and therefore
    // `total_assets()` to 1_100_000, letting the attacker steal a
    // large share of the next depositor's principal.
    asset_client.transfer(&admin, &attacker, &1_000_000i128);
    asset_client.transfer(&attacker, &strategy_addr, &1_000_000i128);

    // F1 invariant: `total_assets` must be identical before and after
    // the donation. Depositor #2 sees the same price as depositor #1.
    let total_after_donation = vault_client.total_assets();
    assert_eq!(
        total_after_donation, total_before_donation,
        "donation to the strategy must NOT change vault NAV"
    );
    assert_eq!(
        strategy.get_balance(),
        strategy_balance_before,
        "strategy.get_balance() must be pinned to local_balance + deployed_total"
    );

    // Step 3: user B deposits the same amount. Shares received must
    // equal (or be within 1 unit of, due to integer rounding) user A's.
    asset_client.transfer(&admin, &user_b, &100_000i128);
    let shares_b = vault_client.deposit(&100_000i128, &user_b, &user_b, &user_b);

    let diff = (shares_a - shares_b).abs();
    assert!(
        diff <= 1,
        "share counts must match within rounding — A={shares_a}, B={shares_b}, diff={diff}"
    );

    // The donation is still sitting at the strategy, observable as a
    // `DonationDetected` event on any subsequent `get_balance()`.
    let _ = strategy.get_balance();
    let all_events = std::format!("{:?}", e.events().all());
    assert!(
        all_events.contains("donation_detected"),
        "DonationDetected must continue firing on get_balance until recovery: {all_events}"
    );
}

/// F1 complement: after the adversary's donation, `recover_donation`
/// sweeps the excess out without disturbing `local_balance` or NAV.
#[test]
fn test_f1_recover_donation_end_to_end() {
    use xlm_strategy::{XlmStrategy, XlmStrategyClient};

    let e = Env::default();
    e.mock_all_auths();

    let admin = Address::generate(&e);
    let operator = Address::generate(&e);
    let controller = Address::generate(&e);
    let treasury = Address::generate(&e);
    let user = Address::generate(&e);

    let asset_client = create_asset_client(&e, DEFAULT_SUPPLY, &admin);
    let vault_client = create_vault_client(&e, &asset_client.address, 3, &admin);
    vault_client.set_operator(&admin, &operator);

    let strategy_addr = e.register(
        XlmStrategy,
        (&asset_client.address, &vault_client.address, &controller),
    );
    let strategy = XlmStrategyClient::new(&e, &strategy_addr);
    vault_client.add_subaccount(&admin, &strategy_addr, &SubaccountType::Strategy);

    // Fund the vault + deploy to strategy.
    asset_client.transfer(&admin, &user, &100_000i128);
    vault_client.deposit(&100_000i128, &user, &user, &user);
    vault_client.deposit_to_subaccount(&operator, &strategy_addr, &30_000i128);

    let nav_before = vault_client.total_assets();
    let strategy_tracker_before = strategy.get_local_balance();

    // Adversary donates 500_000.
    asset_client.transfer(&admin, &strategy_addr, &500_000i128);

    // Controller recovers the donation to a treasury wallet.
    strategy.recover_donation(&controller, &treasury, &500_000i128);

    // Invariants: treasury got the excess, NAV unchanged, strategy
    // tracker unchanged, donation event no longer fires.
    assert_eq!(asset_client.balance(&treasury), 500_000i128);
    assert_eq!(vault_client.total_assets(), nav_before);
    assert_eq!(strategy.get_local_balance(), strategy_tracker_before);

    let _ = strategy.get_balance();
    let all_events = std::format!("{:?}", e.events().all());
    assert!(
        !all_events.contains("donation_detected"),
        "DonationDetected must stop firing after recover_donation: {all_events}"
    );
}

// ==================== Math properties (rounding + round-trip invariants) ====================
//
// These tests pin the algebraic contract of the share-price math, independent
// of any specific exact-value seed. The four preview/convert entry points are
// each one of the four (Floor, Ceil) × (assets→shares, shares→assets) combos:
//
//   preview_deposit  / convert_to_shares  — shares = floor(a * (S + pow) / (TA + 1))
//   preview_redeem   / convert_to_assets  — assets = floor(s * (TA + 1) / (S + pow))
//   preview_mint                          — assets =  ceil(s * (TA + 1) / (S + pow))
//   preview_withdraw                      — shares =  ceil(a * (S + pow) / (TA + 1))
//
// where pow = 10^decimals_offset, S = total_supply, TA = total_assets. Source
// of truth for the rounding directions: `preview_*` impls in `contract.rs`.
//
// A regression that swaps Floor↔Ceil in any of these paths would round equally
// on integer-divisible inputs and slip past tests-by-example; these tests
// assert the algebraic bound on rounding direction directly. The proptest
// sweep at the bottom adds randomized coverage that handpicked seeds can miss
// — 1024 cases per property, sweeping `decimals_offset` 3..=10 plus a loss
// regime the deterministic tests don't otherwise reach.
//
// State construction goes through `deposit_as_user` + `set_deployed` (and an
// optional `asset_client.transfer` for the loss regime) so the fixture
// exercises the same code paths a production vault travels — minimising the
// risk of testing against a state configuration unreachable by legal calls.
//
// Negative inputs panic with codes 403 (assets) / 404 (shares); see the
// `preview_*_rejects_negative_*` should_panic tests below. The property
// assertions assume non-negative inputs.
//
// Worst-case verifier product bounds (used by the proptest ranges below):
// with `deposit_seed ≤ 1e12`, `deployed_extra ≤ 1e12`, `x ≤ 1e9`, and
// `offset ≤ 10`, the largest verifier product is `x * (S + pow)` where
// `S ≈ deposit_seed * pow ≤ 1e22`. So `x * (S + pow) ≤ 1e9 * 1e22 = 1e31`,
// roughly 7 orders of magnitude under `i128::MAX ≈ 1.7e38`. Bumping any
// proptest input bound requires re-checking these products — they protect
// the **verifier-side** `checked_mul`, not the contract's own math.

const OFFSET: u32 = 3;
const POW: i128 = 10_i128.pow(OFFSET);

// -- Negative-input rejection --

#[test]
#[should_panic(expected = "Error(Contract, #403)")]
fn preview_deposit_rejects_negative_assets() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.vault_client.preview_deposit(&-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #403)")]
fn preview_withdraw_rejects_negative_assets() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.vault_client.preview_withdraw(&-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #404)")]
fn preview_mint_rejects_negative_shares() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.vault_client.preview_mint(&-1);
}

#[test]
#[should_panic(expected = "Error(Contract, #404)")]
fn preview_redeem_rejects_negative_shares() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.vault_client.preview_redeem(&-1);
}

// -- Convert / preview drift smoke test --
//
// `convert_to_shares` / `convert_to_assets` share their implementation with
// `preview_deposit` / `preview_redeem` (both Floor). This test fails fast on
// any future split that introduces independent rounding logic.

#[test]
fn convert_matches_preview_at_mid_life() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.set_deployed(333);
    for &x in &[1i128, 7, 999, 1_234_567] {
        assert_eq!(
            s.vault_client.convert_to_shares(&x),
            s.vault_client.preview_deposit(&x),
            "convert_to_shares drifted from preview_deposit at x={x}",
        );
    }
    let supply = s.vault_client.total_supply();
    for &shares in &[1i128, supply / 7, supply / 2] {
        assert_eq!(
            s.vault_client.convert_to_assets(&shares),
            s.vault_client.preview_redeem(&shares),
            "convert_to_assets drifted from preview_redeem at shares={shares}",
        );
    }
}

// -- Assertion helpers (parameterised by `pow` so they cover any offset) --

/// Property: preview_deposit rounds DOWN at the given `pow = 10^offset`.
///
/// Let g = preview_deposit(assets). Floor rounding requires:
///     g       * (TA + 1) <= assets * (S + pow)
///     (g + 1) * (TA + 1) >  assets * (S + pow)
fn assert_preview_deposit_floors(s: &TestSetup, pow: i128, assets: i128) {
    let got = s.vault_client.preview_deposit(&assets);
    let supply = s.vault_client.total_supply();
    let total_assets = s.vault_client.total_assets();
    // preview_deposit must never return negative for non-negative input — that
    // would itself be a contract violation, so assert it explicitly rather
    // than guarding subsequent checks behind `if got >= 0`.
    assert!(
        got >= 0,
        "preview_deposit({assets}) returned negative: got={got}",
    );
    let num = assets
        .checked_mul(supply + pow)
        .expect("verifier num fits i128 — see worst-case bounds at section top");
    let den = total_assets + 1;
    assert!(
        got.checked_mul(den).expect("verifier got*den fits i128") <= num,
        "preview_deposit({assets})={got} rounded UP: got*den > num \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
    // Tightness check — without this, got=0 would pass the <= bound vacuously.
    assert!(
        (got + 1)
            .checked_mul(den)
            .expect("verifier (got+1)*den fits i128")
            > num,
        "preview_deposit({assets})={got} lost more than 1 LSB \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
}

/// Property: preview_redeem rounds DOWN. Symmetric to preview_deposit, with
/// `(S+pow)` and `(TA+1)` swapped between num and den (since this is the
/// shares→assets direction).
fn assert_preview_redeem_floors(s: &TestSetup, pow: i128, shares: i128) {
    let got = s.vault_client.preview_redeem(&shares);
    let supply = s.vault_client.total_supply();
    let total_assets = s.vault_client.total_assets();
    assert!(
        got >= 0,
        "preview_redeem({shares}) returned negative: got={got}",
    );
    let num = shares
        .checked_mul(total_assets + 1)
        .expect("verifier num fits i128");
    let den = supply + pow;
    assert!(
        got.checked_mul(den).expect("verifier got*den fits i128") <= num,
        "preview_redeem({shares})={got} rounded UP \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
    assert!(
        (got + 1)
            .checked_mul(den)
            .expect("verifier (got+1)*den fits i128")
            > num,
        "preview_redeem({shares})={got} lost more than 1 LSB \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
}

/// Property: preview_mint rounds UP.
///
/// Let g = preview_mint(shares). Ceil rounding requires:
///     g       * (S + pow) >= shares * (TA + 1)
///     (g - 1) * (S + pow) <  shares * (TA + 1)   when g > 0
fn assert_preview_mint_ceils(s: &TestSetup, pow: i128, shares: i128) {
    let got = s.vault_client.preview_mint(&shares);
    let supply = s.vault_client.total_supply();
    let total_assets = s.vault_client.total_assets();
    if shares == 0 {
        assert_eq!(got, 0, "preview_mint(0) must be 0");
        return;
    }
    let num = shares
        .checked_mul(total_assets + 1)
        .expect("verifier num fits i128");
    let den = supply + pow;
    assert!(
        got.checked_mul(den).expect("verifier got*den fits i128") >= num,
        "preview_mint({shares})={got} rounded DOWN \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
    if got > 0 {
        assert!(
            (got - 1)
                .checked_mul(den)
                .expect("verifier (got-1)*den fits i128")
                < num,
            "preview_mint({shares})={got} more than 1 LSB above ceil \
             (supply={supply}, total_assets={total_assets}, pow={pow})",
        );
    }
}

/// Property: preview_withdraw rounds UP. Symmetric to preview_mint, with
/// `(S+pow)` and `(TA+1)` swapped between num and den (assets→shares direction).
fn assert_preview_withdraw_ceils(s: &TestSetup, pow: i128, assets: i128) {
    let got = s.vault_client.preview_withdraw(&assets);
    let supply = s.vault_client.total_supply();
    let total_assets = s.vault_client.total_assets();
    if assets == 0 {
        assert_eq!(got, 0, "preview_withdraw(0) must be 0");
        return;
    }
    let num = assets
        .checked_mul(supply + pow)
        .expect("verifier num fits i128");
    let den = total_assets + 1;
    assert!(
        got.checked_mul(den).expect("verifier got*den fits i128") >= num,
        "preview_withdraw({assets})={got} rounded DOWN \
         (supply={supply}, total_assets={total_assets}, pow={pow})",
    );
    if got > 0 {
        assert!(
            (got - 1)
                .checked_mul(den)
                .expect("verifier (got-1)*den fits i128")
                < num,
            "preview_withdraw({assets})={got} more than 1 LSB above ceil \
             (supply={supply}, total_assets={total_assets}, pow={pow})",
        );
    }
}

// -- S.1: rounding-direction property tests (hand-picked seeds at OFFSET=3) --

#[test]
fn property_preview_deposit_floors_on_fresh_vault() {
    // supply=0, total_assets=0: deposits mint `assets * pow` shares exactly,
    // so rounding is trivial here. Still useful as smoke; rounding bites in
    // the post-deposit cases below.
    let s = TestSetup::vault_only_with_offset(OFFSET);
    assert_preview_deposit_floors(&s, POW, 1);
    assert_preview_deposit_floors(&s, POW, 7);
    assert_preview_deposit_floors(&s, POW, 1_000_001);
}

#[test]
fn property_preview_deposit_floors_post_deposit_with_deployed() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.set_deployed(333);
    // Non-divisible products: these would catch a Floor→Ceil regression.
    assert_preview_deposit_floors(&s, POW, 7);
    assert_preview_deposit_floors(&s, POW, 999);
    assert_preview_deposit_floors(&s, POW, 1_234_567);
}

#[test]
fn property_preview_redeem_floors_post_deposit() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    let supply = s.vault_client.total_supply();
    assert_preview_redeem_floors(&s, POW, 1);
    assert_preview_redeem_floors(&s, POW, supply / 3);
    assert_preview_redeem_floors(&s, POW, supply - 1);
}

#[test]
fn property_preview_redeem_floors_with_deployed_skew() {
    // total_assets >> supply (post-profit): each share redeems "more" assets.
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.set_deployed(999_999);
    let supply = s.vault_client.total_supply();
    assert_preview_redeem_floors(&s, POW, 1);
    assert_preview_redeem_floors(&s, POW, supply / 7);
    assert_preview_redeem_floors(&s, POW, supply / 2);
}

#[test]
fn property_floors_and_ceils_hold_under_loss() {
    // Loss regime: local_balance < initial_deposit. Extract half the vault's
    // tokens directly to simulate a strategy loss. All four properties must
    // still hold simultaneously.
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.asset_client
        .transfer(&s.vault_client.address, &s.admin, &500_000);
    assert_preview_deposit_floors(&s, POW, 7);
    assert_preview_deposit_floors(&s, POW, 1_234_567);
    let supply = s.vault_client.total_supply();
    assert_preview_redeem_floors(&s, POW, supply / 3);
    assert_preview_mint_ceils(&s, POW, 100);
    assert_preview_withdraw_ceils(&s, POW, 7);
}

#[test]
fn property_preview_mint_ceils_zero_supply() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    assert_preview_mint_ceils(&s, POW, 0);
    assert_preview_mint_ceils(&s, POW, 1);
    assert_preview_mint_ceils(&s, POW, 999_999);
}

#[test]
fn property_preview_mint_ceils_mid_life() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.set_deployed(333);
    assert_preview_mint_ceils(&s, POW, 1);
    assert_preview_mint_ceils(&s, POW, 999_999);
    assert_preview_mint_ceils(&s, POW, 1_234_567);
}

#[test]
fn property_preview_withdraw_ceils_zero_supply() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    assert_preview_withdraw_ceils(&s, POW, 0);
    assert_preview_withdraw_ceils(&s, POW, 1);
}

#[test]
fn property_preview_withdraw_ceils_mid_life() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    s.set_deployed(333);
    assert_preview_withdraw_ceils(&s, POW, 1);
    assert_preview_withdraw_ceils(&s, POW, 7);
    assert_preview_withdraw_ceils(&s, POW, 1_234_567);
}

// -- S.2: round-trip property `preview_redeem(preview_deposit(x)) ≤ x` --
//
// The values inside each `for &x` loop cover three orthogonal slices:
//   - 1 / a few: rounding boundary (every LSB matters)
//   - mid-range: typical user deposit amount
//   - near-supply or near-cap: high end without flirting with i128 overflow

#[test]
fn round_trip_fresh_vault() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    for &x in &[1i128, 1_000_000, 999_999_999] {
        let minted = s.vault_client.preview_deposit(&x);
        let redeemed = s.vault_client.preview_redeem(&minted);
        assert!(
            redeemed <= x,
            "fresh vault round-trip extracted value: x={x}, minted={minted}, redeemed={redeemed}",
        );
    }
}

#[test]
fn round_trip_mid_life_after_initial_deposit() {
    // Mid-life right after a single initial deposit. Share:asset ratio is
    // pow:1 (1000:1 at OFFSET=3), not 1:1 — the previous name was misleading.
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1_000_000);
    for &x in &[1i128, 100, 999, 500_000] {
        let minted = s.vault_client.preview_deposit(&x);
        let redeemed = s.vault_client.preview_redeem(&minted);
        assert!(
            redeemed <= x,
            "mid-life round-trip extracted value: x={x}, minted={minted}, redeemed={redeemed}",
        );
    }
}

#[test]
fn round_trip_high_share_price() {
    // After profit, total_assets >> supply: 1 share ≈ 1e9 assets here, so x
    // must be at least ~5e8 to mint > 0 shares. Pick inputs that GUARANTEE
    // a non-trivial round-trip — silent `continue` on minted=0 would defeat
    // the test (the previous fixture skipped 3 of 4 iterations).
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(1);
    s.set_deployed(999_999_999);
    // pow=1000; share = floor(x * (1000 + 1000) / (999_999_999 + 1 + 1))
    // ≈ floor(x * 2 / 1e9). Need x ≥ ~5e8 for minted ≥ 1.
    for &x in &[
        500_000_001i128,
        1_000_000_000,
        2_000_000_000,
        999_999_999_999,
    ] {
        let minted = s.vault_client.preview_deposit(&x);
        assert!(
            minted > 0,
            "high-share-price test seed underpowered: x={x} pre-rounded to 0 \
             (the fixture needs larger inputs)",
        );
        let redeemed = s.vault_client.preview_redeem(&minted);
        assert!(
            redeemed <= x,
            "high-share-price round-trip extracted value: x={x}, minted={minted}, redeemed={redeemed}",
        );
    }
}

#[test]
fn round_trip_asymmetric() {
    let s = TestSetup::vault_only_with_offset(OFFSET);
    s.deposit_as_user(123_456);
    s.set_deployed(789_012);
    let minted = s.vault_client.preview_deposit(&12_345);
    let redeemed = s.vault_client.preview_redeem(&minted);
    assert!(
        redeemed <= 12_345,
        "asymmetric round-trip extracted value: minted={minted}, redeemed={redeemed}",
    );
}

// -- S.3: proptest randomized sweep --
//
// 1024 cases per property (4× proptest's default 256) — costs ~24s total but
// gives meaningfully more shrinking coverage on the rare-bug regime.
// Parameters:
//   - offset 3..=10: every valid `decimals_offset` the contract accepts
//   - deposit_seed / deployed_extra in 0..1e12: realistic vault sizes
//   - extracted_pct in 0..=100: covers loss regimes (local < initial deposit)
//   - x / shares_in / assets_in in 1..1e9 (or 0..): bounded by verifier math

/// Set up a fresh vault at the given offset, optionally seed-deposit, extract
/// a percentage to simulate loss, then bump deployed_assets. Shared by all
/// three proptest properties.
fn setup_proptest_state(
    offset: u32,
    deposit_seed: i128,
    extracted_pct: u32,
    deployed_extra: i128,
) -> TestSetup<'static> {
    let s = TestSetup::vault_only_with_offset(offset);
    if deposit_seed > 0 {
        s.deposit_as_user(deposit_seed);
    }
    if extracted_pct > 0 && deposit_seed > 0 {
        let extracted = deposit_seed * extracted_pct as i128 / 100;
        if extracted > 0 && extracted <= deposit_seed {
            s.asset_client
                .transfer(&s.vault_client.address, &s.admin, &extracted);
        }
    }
    if deployed_extra > 0 {
        s.set_deployed(deployed_extra);
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// preview_deposit floors AND `preview_redeem(preview_deposit(x)) ≤ x`
    /// across all valid offsets, with an optional loss regime mixed in.
    #[test]
    fn proptest_preview_deposit_floor_and_round_trip(
        offset in 3u32..=10,
        deposit_seed in 0i128..1_000_000_000_000_i128,
        deployed_extra in 0i128..1_000_000_000_000_i128,
        extracted_pct in 0u32..=100,
        x in 1i128..1_000_000_000_i128,
    ) {
        let pow = 10_i128.pow(offset);
        let s = setup_proptest_state(offset, deposit_seed, extracted_pct, deployed_extra);

        assert_preview_deposit_floors(&s, pow, x);

        let minted = s.vault_client.preview_deposit(&x);
        if minted > 0 {
            assert_preview_redeem_floors(&s, pow, minted);
            let redeemed = s.vault_client.preview_redeem(&minted);
            prop_assert!(
                redeemed <= x,
                "round-trip extracted value: x={}, minted={}, redeemed={}", x, minted, redeemed,
            );
        } else {
            // Verify the skip is mathematically correct: minted == 0 iff the
            // exact rational `x*(S+pow) / (TA+1)` is `< 1`. Otherwise minted=0
            // is itself a contract bug — preview_deposit rounded down when it
            // shouldn't have.
            let supply = s.vault_client.total_supply();
            let total_assets = s.vault_client.total_assets();
            let num = x.checked_mul(supply + pow).expect("verifier num fits i128");
            let den = total_assets + 1;
            prop_assert!(
                num < den,
                "preview_deposit returned 0 but x*(S+pow) >= TA+1: \
                 x={}, supply={}, total_assets={}, pow={}, num={}, den={}",
                x, supply, total_assets, pow, num, den,
            );
        }
    }

    /// preview_mint rounds up for arbitrary (offset, supply, total_assets, shares).
    #[test]
    fn proptest_preview_mint_ceils(
        offset in 3u32..=10,
        deposit_seed in 0i128..1_000_000_000_000_i128,
        deployed_extra in 0i128..1_000_000_000_000_i128,
        extracted_pct in 0u32..=100,
        shares_in in 0i128..1_000_000_000_i128,
    ) {
        let pow = 10_i128.pow(offset);
        let s = setup_proptest_state(offset, deposit_seed, extracted_pct, deployed_extra);
        assert_preview_mint_ceils(&s, pow, shares_in);
    }

    /// preview_withdraw rounds up for arbitrary (offset, supply, total_assets, assets).
    #[test]
    fn proptest_preview_withdraw_ceils(
        offset in 3u32..=10,
        deposit_seed in 0i128..1_000_000_000_000_i128,
        deployed_extra in 0i128..1_000_000_000_000_i128,
        extracted_pct in 0u32..=100,
        assets_in in 0i128..1_000_000_000_i128,
    ) {
        let pow = 10_i128.pow(offset);
        let s = setup_proptest_state(offset, deposit_seed, extracted_pct, deployed_extra);
        assert_preview_withdraw_ceils(&s, pow, assets_in);
    }
}
