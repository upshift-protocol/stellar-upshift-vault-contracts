use soroban_sdk::{contracttype, panic_with_error, Address, Env, Map, Vec};

use crate::errors::VaultError;

/// Variant names must match the TypeScript `SubaccountType` union in
/// `frontend/lib/subaccount-type.ts` — the frontend encodes these as
/// Soroban symbols for `add_subaccount` calls.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubaccountType {
    Strategy,
    Wallet,
}

#[contracttype]
pub enum StorageKey {
    Admin,
    Operator,
    DeployedAssets,
    Subaccounts,
    AumIncreaseLimit,
    AumDecreaseLimit,
    Paused,
    SubaccountTypes,
    PendingAdmin,
    AdminProposalExpiry,
    // Cumulative AUM window tracking
    AumWindowDuration,
    AumCumulativeIncreaseLimit,
    AumCumulativeDecreaseLimit,
    AumWindowStart,
    AumCumulativeIncrease,
    AumCumulativeDecrease,
    AumWindowBaseDeployed,
    /// Map<Address, i128> — per-wallet net-deployed tracker.
    /// Authoritative cap on wallet pulls: `withdraw_from_subaccount`
    /// rejects any pull exceeding this value with `WalletOverWithdraw`.
    /// Moved in lockstep with `deployed_assets` by
    /// `update_wallet_deployed` when the operator recognises gains,
    /// losses, or external dust. Also used by removal diagnostics
    /// (emits `WalletBalanceDiverged` when tracked ≠ observed at
    /// removal time).
    WalletNetDeployed,
}

pub const DEFAULT_AUM_INCREASE_LIMIT: u32 = 1_000; // 10%
pub const DEFAULT_AUM_DECREASE_LIMIT: u32 = 500; // 5%
pub const MAX_SUBACCOUNTS: u32 = 10;
pub const MIN_AUM_LIMIT_BPS: u32 = 1;
pub const MAX_AUM_LIMIT_BPS: u32 = 10_000;

pub const DEFAULT_AUM_WINDOW_DURATION: u64 = 86_400; // 24 hours
pub const MIN_AUM_WINDOW_DURATION: u64 = 3_600; // 1 hour
pub const MAX_AUM_WINDOW_DURATION: u64 = 604_800; // 7 days

// Compile-time: ensure defaults fall within the valid BPS range.
const _: () = assert!(DEFAULT_AUM_INCREASE_LIMIT >= MIN_AUM_LIMIT_BPS);
const _: () = assert!(DEFAULT_AUM_INCREASE_LIMIT <= MAX_AUM_LIMIT_BPS);
const _: () = assert!(DEFAULT_AUM_DECREASE_LIMIT >= MIN_AUM_LIMIT_BPS);
const _: () = assert!(DEFAULT_AUM_DECREASE_LIMIT <= MAX_AUM_LIMIT_BPS);

// ==================== Admin ====================

pub fn get_admin(e: &Env) -> Address {
    e.storage()
        .instance()
        .get(&StorageKey::Admin)
        .unwrap_or_else(|| panic_with_error!(e, VaultError::Unauthorized))
}

pub fn set_admin(e: &Env, admin: &Address) {
    e.storage().instance().set(&StorageKey::Admin, admin);
}

pub fn require_admin(e: &Env, caller: &Address) {
    caller.require_auth();
    let admin = get_admin(e);
    if *caller != admin {
        panic_with_error!(e, VaultError::Unauthorized);
    }
}

// ==================== Pending Admin ====================
//
// Both PendingAdmin and AdminProposalExpiry share Instance storage TTL.
// Proposals carry an explicit deadline enforced in `accept_admin`; the
// TTL-based expiry is a secondary safeguard that would only matter if
// the contract is completely inactive for the full TTL period (unlikely,
// since `bump_instance` is called by every mutable entry point).

pub fn get_pending_admin(e: &Env) -> Option<Address> {
    e.storage().instance().get(&StorageKey::PendingAdmin)
}

pub fn set_pending_admin(e: &Env, pending: &Address) {
    e.storage()
        .instance()
        .set(&StorageKey::PendingAdmin, pending);
}

pub fn remove_pending_admin(e: &Env) {
    e.storage().instance().remove(&StorageKey::PendingAdmin);
    e.storage()
        .instance()
        .remove(&StorageKey::AdminProposalExpiry);
}

pub fn get_admin_proposal_expiry(e: &Env) -> Option<u64> {
    e.storage().instance().get(&StorageKey::AdminProposalExpiry)
}

pub fn set_admin_proposal_expiry(e: &Env, expiry: u64) {
    e.storage()
        .instance()
        .set(&StorageKey::AdminProposalExpiry, &expiry);
}

// ==================== Operator ====================

pub fn get_operator(e: &Env) -> Option<Address> {
    e.storage().instance().get(&StorageKey::Operator)
}

pub fn set_operator(e: &Env, operator: &Address) {
    e.storage().instance().set(&StorageKey::Operator, operator);
}

pub fn require_operator(e: &Env, caller: &Address) {
    caller.require_auth();
    match get_operator(e) {
        Some(op) if *caller == op => {}
        _ => panic_with_error!(e, VaultError::Unauthorized),
    }
}

// ==================== Deployed Assets ====================

pub fn get_deployed_assets(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&StorageKey::DeployedAssets)
        .unwrap_or(0)
}

pub fn set_deployed_assets(e: &Env, amount: i128) {
    if amount < 0 {
        panic_with_error!(e, VaultError::InvalidAmount);
    }
    e.storage()
        .instance()
        .set(&StorageKey::DeployedAssets, &amount);
}

// ==================== Subaccounts ====================

pub fn get_subaccounts(e: &Env) -> Vec<Address> {
    e.storage()
        .instance()
        .get(&StorageKey::Subaccounts)
        .unwrap_or_else(|| Vec::new(e))
}

pub fn set_subaccounts(e: &Env, subaccounts: &Vec<Address>) {
    if subaccounts.len() > MAX_SUBACCOUNTS {
        panic_with_error!(e, VaultError::MaxSubaccountsReached);
    }
    e.storage()
        .instance()
        .set(&StorageKey::Subaccounts, subaccounts);
}

// ==================== Subaccount Types ====================

pub fn get_subaccount_type(e: &Env, subaccount: &Address) -> SubaccountType {
    match e
        .storage()
        .instance()
        .get::<_, Map<Address, SubaccountType>>(&StorageKey::SubaccountTypes)
    {
        Some(m) => m
            .get(subaccount.clone())
            // Pre-upgrade subaccounts may not have a type entry yet.
            // Default to Strategy for backward compatibility.
            .unwrap_or(SubaccountType::Strategy),
        // No types map at all — contract predates SubaccountType support.
        None => SubaccountType::Strategy,
    }
}

pub fn set_subaccount_type(e: &Env, subaccount: &Address, kind: SubaccountType) {
    let mut types: Map<Address, SubaccountType> = e
        .storage()
        .instance()
        .get(&StorageKey::SubaccountTypes)
        .unwrap_or_else(|| Map::new(e));
    types.set(subaccount.clone(), kind);
    e.storage()
        .instance()
        .set(&StorageKey::SubaccountTypes, &types);
}

pub fn remove_subaccount_type(e: &Env, subaccount: &Address) {
    if let Some(mut types) = e
        .storage()
        .instance()
        .get::<_, Map<Address, SubaccountType>>(&StorageKey::SubaccountTypes)
    {
        types.remove(subaccount.clone());
        e.storage()
            .instance()
            .set(&StorageKey::SubaccountTypes, &types);
    }
}

// ==================== Wallet Net-Deployed ====================
//
// Vault-controlled per-wallet accounting: the net capital the vault has
// deployed to each Wallet-type subaccount (deposits in, withdrawals out).
// Only Wallet subaccounts are tracked — Strategy balances are already
// captured live via `IStrategy::get_balance()`.
//
// Stored as a single Map<Address, i128> in instance storage, colocated with
// the subaccount list (bounded by MAX_SUBACCOUNTS = 10). Entries are inserted
// on the first deposit to a wallet and removed on `remove_subaccount`.

pub fn get_wallet_net_deployed(e: &Env, subaccount: &Address) -> i128 {
    match e
        .storage()
        .instance()
        .get::<_, Map<Address, i128>>(&StorageKey::WalletNetDeployed)
    {
        Some(m) => m.get(subaccount.clone()).unwrap_or(0),
        None => 0,
    }
}

pub fn set_wallet_net_deployed(e: &Env, subaccount: &Address, amount: i128) {
    // The per-wallet tracker is a non-negative invariant. This branch is
    // unreachable under normal contract flows (the `.max(0)` in
    // `withdraw_from_subaccount` floors the only subtractive path at 0,
    // and `seed_wallet_net_deployed` rejects negative inputs upstream),
    // but we keep it as a final storage-layer guard. `InvalidAmount` is
    // the correct semantic — `DeployedAssetsUnderflow` implies the
    // vault-wide counter rather than this per-wallet map.
    if amount < 0 {
        panic_with_error!(e, VaultError::InvalidAmount);
    }
    let mut m: Map<Address, i128> = e
        .storage()
        .instance()
        .get(&StorageKey::WalletNetDeployed)
        .unwrap_or_else(|| Map::new(e));
    m.set(subaccount.clone(), amount);
    e.storage()
        .instance()
        .set(&StorageKey::WalletNetDeployed, &m);
}

pub fn remove_wallet_net_deployed(e: &Env, subaccount: &Address) {
    if let Some(mut m) = e
        .storage()
        .instance()
        .get::<_, Map<Address, i128>>(&StorageKey::WalletNetDeployed)
    {
        m.remove(subaccount.clone());
        e.storage()
            .instance()
            .set(&StorageKey::WalletNetDeployed, &m);
    }
}

// ==================== Pause ====================

pub fn is_paused(e: &Env) -> bool {
    e.storage()
        .instance()
        .get(&StorageKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(e: &Env, paused: bool) {
    e.storage().instance().set(&StorageKey::Paused, &paused);
}

pub fn require_not_paused(e: &Env) {
    if is_paused(e) {
        panic_with_error!(e, VaultError::VaultPaused);
    }
}

// ==================== AUM Limits ====================

pub fn get_aum_increase_limit(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&StorageKey::AumIncreaseLimit)
        .unwrap_or(DEFAULT_AUM_INCREASE_LIMIT)
}

pub fn get_aum_decrease_limit(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&StorageKey::AumDecreaseLimit)
        .unwrap_or(DEFAULT_AUM_DECREASE_LIMIT)
}

pub fn set_aum_limits(e: &Env, increase_bps: u32, decrease_bps: u32) {
    if !(MIN_AUM_LIMIT_BPS..=MAX_AUM_LIMIT_BPS).contains(&increase_bps)
        || !(MIN_AUM_LIMIT_BPS..=MAX_AUM_LIMIT_BPS).contains(&decrease_bps)
    {
        panic_with_error!(e, VaultError::InvalidAumLimits);
    }
    e.storage()
        .instance()
        .set(&StorageKey::AumIncreaseLimit, &increase_bps);
    e.storage()
        .instance()
        .set(&StorageKey::AumDecreaseLimit, &decrease_bps);
}

// ==================== AUM Cumulative Window ====================

pub fn get_aum_window_duration(e: &Env) -> u64 {
    e.storage()
        .instance()
        .get(&StorageKey::AumWindowDuration)
        .unwrap_or(DEFAULT_AUM_WINDOW_DURATION)
}

pub fn get_aum_cumulative_increase_limit(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&StorageKey::AumCumulativeIncreaseLimit)
        .unwrap_or(DEFAULT_AUM_INCREASE_LIMIT)
}

pub fn get_aum_cumulative_decrease_limit(e: &Env) -> u32 {
    e.storage()
        .instance()
        .get(&StorageKey::AumCumulativeDecreaseLimit)
        .unwrap_or(DEFAULT_AUM_DECREASE_LIMIT)
}

pub fn get_aum_window_start(e: &Env) -> u64 {
    e.storage()
        .instance()
        .get(&StorageKey::AumWindowStart)
        .unwrap_or(0)
}

pub fn get_aum_cumulative_increase(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&StorageKey::AumCumulativeIncrease)
        .unwrap_or(0)
}

pub fn get_aum_cumulative_decrease(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&StorageKey::AumCumulativeDecrease)
        .unwrap_or(0)
}

pub fn get_aum_window_base_deployed(e: &Env) -> i128 {
    e.storage()
        .instance()
        .get(&StorageKey::AumWindowBaseDeployed)
        .unwrap_or(0)
}

pub fn set_aum_window_limits(
    e: &Env,
    window_duration: u64,
    cumulative_increase_bps: u32,
    cumulative_decrease_bps: u32,
) {
    if !(MIN_AUM_WINDOW_DURATION..=MAX_AUM_WINDOW_DURATION).contains(&window_duration)
        || !(MIN_AUM_LIMIT_BPS..=MAX_AUM_LIMIT_BPS).contains(&cumulative_increase_bps)
        || !(MIN_AUM_LIMIT_BPS..=MAX_AUM_LIMIT_BPS).contains(&cumulative_decrease_bps)
    {
        panic_with_error!(e, VaultError::InvalidAumLimits);
    }
    e.storage()
        .instance()
        .set(&StorageKey::AumWindowDuration, &window_duration);
    e.storage().instance().set(
        &StorageKey::AumCumulativeIncreaseLimit,
        &cumulative_increase_bps,
    );
    e.storage().instance().set(
        &StorageKey::AumCumulativeDecreaseLimit,
        &cumulative_decrease_bps,
    );
}

pub fn reset_aum_window(e: &Env, timestamp: u64, base_deployed: i128) {
    e.storage()
        .instance()
        .set(&StorageKey::AumWindowStart, &timestamp);
    e.storage()
        .instance()
        .set(&StorageKey::AumCumulativeIncrease, &0i128);
    e.storage()
        .instance()
        .set(&StorageKey::AumCumulativeDecrease, &0i128);
    e.storage()
        .instance()
        .set(&StorageKey::AumWindowBaseDeployed, &base_deployed);
}

pub fn set_aum_cumulative_increase(e: &Env, amount: i128) {
    e.storage()
        .instance()
        .set(&StorageKey::AumCumulativeIncrease, &amount);
}

pub fn set_aum_cumulative_decrease(e: &Env, amount: i128) {
    e.storage()
        .instance()
        .set(&StorageKey::AumCumulativeDecrease, &amount);
}
