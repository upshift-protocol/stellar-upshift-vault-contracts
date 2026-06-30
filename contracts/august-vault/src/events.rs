use soroban_sdk::{contractevent, contracttype, Address, Env};

use crate::storage::SubaccountType;

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminSet {
    #[topic]
    pub admin: Address,
}

pub fn emit_admin_set(e: &Env, admin: &Address) {
    AdminSet {
        admin: admin.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferProposed {
    #[topic]
    pub admin: Address,
    pub pending_admin: Address,
    pub deadline: u64,
}

pub fn emit_admin_transfer_proposed(
    e: &Env,
    admin: &Address,
    pending_admin: &Address,
    deadline: u64,
) {
    AdminTransferProposed {
        admin: admin.clone(),
        pending_admin: pending_admin.clone(),
        deadline,
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferAccepted {
    #[topic]
    pub old_admin: Address,
    #[topic]
    pub new_admin: Address,
}

pub fn emit_admin_transfer_accepted(e: &Env, old_admin: &Address, new_admin: &Address) {
    AdminTransferAccepted {
        old_admin: old_admin.clone(),
        new_admin: new_admin.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminTransferCancelled {
    #[topic]
    pub admin: Address,
}

pub fn emit_admin_transfer_cancelled(e: &Env, admin: &Address) {
    AdminTransferCancelled {
        admin: admin.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperatorSet {
    #[topic]
    pub admin: Address,
    pub old_operator: Option<Address>,
    pub new_operator: Address,
}

pub fn emit_operator_set(
    e: &Env,
    admin: &Address,
    old_operator: &Option<Address>,
    new_operator: &Address,
) {
    OperatorSet {
        admin: admin.clone(),
        old_operator: old_operator.clone(),
        new_operator: new_operator.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AumLimitsChanged {
    #[topic]
    pub admin: Address,
    pub old_increase_bps: u32,
    pub old_decrease_bps: u32,
    pub new_increase_bps: u32,
    pub new_decrease_bps: u32,
}

pub fn emit_aum_limits_changed(
    e: &Env,
    admin: &Address,
    old_increase_bps: u32,
    old_decrease_bps: u32,
    new_increase_bps: u32,
    new_decrease_bps: u32,
) {
    AumLimitsChanged {
        admin: admin.clone(),
        old_increase_bps,
        old_decrease_bps,
        new_increase_bps,
        new_decrease_bps,
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AumWindowLimitsChanged {
    #[topic]
    pub admin: Address,
    pub old_window_duration: u64,
    pub old_cumulative_increase_bps: u32,
    pub old_cumulative_decrease_bps: u32,
    pub new_window_duration: u64,
    pub new_cumulative_increase_bps: u32,
    pub new_cumulative_decrease_bps: u32,
}

pub fn emit_aum_window_limits_changed(e: &Env, event: AumWindowLimitsChanged) {
    event.publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedAssetsChanged {
    #[topic]
    pub caller: Address,
    pub old_amount: i128,
    pub new_amount: i128,
}

pub fn emit_deployed_assets_changed(e: &Env, caller: &Address, old_amount: i128, new_amount: i128) {
    DeployedAssetsChanged {
        caller: caller.clone(),
        old_amount,
        new_amount,
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultPaused {
    #[topic]
    pub admin: Address,
}

pub fn emit_paused(e: &Env, admin: &Address) {
    VaultPaused {
        admin: admin.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VaultUnpaused {
    #[topic]
    pub admin: Address,
}

pub fn emit_unpaused(e: &Env, admin: &Address) {
    VaultUnpaused {
        admin: admin.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubaccountAdded {
    #[topic]
    pub admin: Address,
    #[topic]
    pub subaccount: Address,
    pub subaccount_type: SubaccountType,
}

pub fn emit_subaccount_added(
    e: &Env,
    admin: &Address,
    subaccount: &Address,
    subaccount_type: &SubaccountType,
) {
    SubaccountAdded {
        admin: admin.clone(),
        subaccount: subaccount.clone(),
        subaccount_type: subaccount_type.clone(),
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubaccountRemoved {
    #[topic]
    pub admin: Address,
    #[topic]
    pub subaccount: Address,
    /// The vault-wide `deployed_assets` counter at the time of removal.
    /// This value is only meaningful for `Wallet`-type subaccounts;
    /// `Strategy` balances are tracked live via `get_balance()`.
    pub total_deployed_assets: i128,
    /// For `Strategy` subaccounts, the result of `try_get_balance()` at
    /// removal time: `Some(balance)` if the strategy responded with a
    /// valid i128, `None` if the call trapped or returned an undecodable
    /// value (the vault still proceeds with removal to recover from a
    /// broken strategy). Always `None` for `Wallet` subaccounts.
    ///
    /// This is the on-chain record linking the share-price drop caused
    /// by removing a funded strategy to the removal transaction itself —
    /// without it, off-chain auditors had no way to attribute the NAV
    /// change to the removal event.
    pub strategy_balance: Option<i128>,
    pub subaccount_type: SubaccountType,
}

pub fn emit_subaccount_removed(
    e: &Env,
    admin: &Address,
    subaccount: &Address,
    total_deployed_assets: i128,
    strategy_balance: Option<i128>,
    subaccount_type: &SubaccountType,
) {
    SubaccountRemoved {
        admin: admin.clone(),
        subaccount: subaccount.clone(),
        total_deployed_assets,
        strategy_balance,
        subaccount_type: subaccount_type.clone(),
    }
    .publish(e);
}

/// Emitted when a wallet's tracked net-deployed amount diverges from its
/// on-chain token balance at removal time. Informational only — the vault
/// does NOT use the tracker or the observed balance to adjust
/// `deployed_assets` on removal (that write-down is the admin's explicit
/// responsibility via `update_deployed_assets`). Indicates either external
/// dust sent to the wallet (tracked < balance) or an off-chain spend the
/// vault did not drive (tracked > balance).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletBalanceDiverged {
    #[topic]
    pub subaccount: Address,
    pub tracked_net_deployed: i128,
    pub wallet_balance: i128,
}

pub fn emit_wallet_balance_diverged(
    e: &Env,
    subaccount: &Address,
    tracked_net_deployed: i128,
    wallet_balance: i128,
) {
    WalletBalanceDiverged {
        subaccount: subaccount.clone(),
        tracked_net_deployed,
        wallet_balance,
    }
    .publish(e);
}

/// Reason a strategy's `get_balance` probe failed at removal time. Split
/// from the unified `strategy_balance: None` encoding so off-chain
/// monitoring can distinguish "strategy panicked" (InvokeError — code
/// bug or missing entry point) from "strategy returned a non-i128"
/// (ConvertError — type-mismatch regression).
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StrategyProbeFailure {
    /// The contract call itself failed (trap, missing function, host
    /// error, TTL expiry, etc.).
    InvokeError,
    /// The call succeeded but the returned value could not be decoded
    /// as `i128`.
    ConvertError,
}

/// Emitted from `do_remove_subaccount` when `try_get_balance` cannot
/// produce an i128. Paired with `SubaccountRemoved`'s `strategy_balance:
/// None` but carries the distinguishing reason code so auditors can
/// differentiate the broken-strategy failure modes.
///
/// **Off-chain correlation**: the two events are emitted in the same
/// transaction and share the `subaccount` topic. Indexers rebuilding
/// the full picture should correlate by `(tx_hash, subaccount)`:
/// - `SubaccountRemoved` alone with `strategy_balance = Some(n)` →
///   clean strategy removal, balance `n` exited NAV.
/// - `SubaccountRemoved` + `StrategyBalanceProbeFailed(InvokeError)` →
///   strategy was unreachable (trap, missing method, TTL expiry).
/// - `SubaccountRemoved` + `StrategyBalanceProbeFailed(ConvertError)` →
///   strategy responded but returned a non-i128; ABI regression.
/// - `SubaccountRemoved` alone with `strategy_balance = None` and
///   `subaccount_type = Wallet` → wallet removal, not a strategy
///   probe failure (the event is always `None` for wallets).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyBalanceProbeFailed {
    #[topic]
    pub subaccount: Address,
    pub reason: StrategyProbeFailure,
}

pub fn emit_strategy_balance_probe_failed(
    e: &Env,
    subaccount: &Address,
    reason: StrategyProbeFailure,
) {
    StrategyBalanceProbeFailed {
        subaccount: subaccount.clone(),
        reason,
    }
    .publish(e);
}

/// Emitted at wallet-removal time when the diagnostic `token.balance(wallet)`
/// probe fails (host-level invocation error or undecodable return value). The
/// removal itself succeeds — this event distinguishes "probe failed" from
/// "no divergence" for off-chain monitoring, so a silent skip of the
/// `WalletBalanceDiverged` diagnostic is still visible.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletBalanceProbeFailed {
    #[topic]
    pub subaccount: Address,
    pub tracked_net_deployed: i128,
}

pub fn emit_wallet_balance_probe_failed(e: &Env, subaccount: &Address, tracked_net_deployed: i128) {
    WalletBalanceProbeFailed {
        subaccount: subaccount.clone(),
        tracked_net_deployed,
    }
    .publish(e);
}

/// Emitted when the operator reconciles a wallet's attributed value via
/// `update_wallet_deployed` — atomically moves both the per-wallet tracker
/// and the aggregate `deployed_assets` counter. `delta` is the signed
/// change applied to each (positive for recognised gains / dust recovery,
/// negative for losses). Distinct from `DeployedAssetsChanged`, which
/// signals an aggregate-only reconciliation (e.g. off-chain strategy AUM
/// not tied to a specific wallet).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletDeployedUpdated {
    #[topic]
    pub operator: Address,
    #[topic]
    pub subaccount: Address,
    pub old_tracked: i128,
    pub new_tracked: i128,
    pub delta: i128,
}

pub fn emit_wallet_deployed_updated(
    e: &Env,
    operator: &Address,
    subaccount: &Address,
    old_tracked: i128,
    new_tracked: i128,
    delta: i128,
) {
    WalletDeployedUpdated {
        operator: operator.clone(),
        subaccount: subaccount.clone(),
        old_tracked,
        new_tracked,
        delta,
    }
    .publish(e);
}

/// Emitted once per `update_wallet_deployed_batch` call to record that a
/// group of per-wallet updates was applied as a single atomic unit.
/// Indexers use this as the transaction-level anchor for the N
/// `WalletDeployedUpdated` events that share the same transaction.
///
/// `count` is the number of entries in the batch. `net_delta` is the net
/// move applied to `deployed_assets` — equal to the sum of per-wallet
/// deltas. A zero `net_delta` is possible (e.g. one wallet gains 100k and
/// another loses 100k in the same batch) and still emits this event as
/// an audit trail.
///
/// **Emission order within the transaction** (indexers: scan backward
/// from this event to collect the batch members):
///
/// 1. `DeployedAssetsChanged` — once, from `apply_deployed_assets_change`.
/// 2. `WalletDeployedUpdated` — once per batch entry, in input order.
/// 3. `WalletDeployedBatchApplied` — this event, emitted last.
///
/// The anchor fires last rather than first so that all per-wallet events
/// it summarises have already been emitted when an indexer sees it; the
/// cost is that streaming consumers must buffer the preceding events
/// until the anchor arrives (bounded by `MAX_SUBACCOUNTS = 10`).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletDeployedBatchApplied {
    #[topic]
    pub operator: Address,
    pub count: u32,
    pub net_delta: i128,
}

pub fn emit_wallet_deployed_batch_applied(
    e: &Env,
    operator: &Address,
    count: u32,
    net_delta: i128,
) {
    WalletDeployedBatchApplied {
        operator: operator.clone(),
        count,
        net_delta,
    }
    .publish(e);
}

/// Emitted when the admin seeds the per-wallet net-deployed tracker for a
/// Wallet subaccount. Used as a one-time migration step after an upgrade
/// that introduced the per-wallet tracker, to backfill accounting for
/// wallets that existed before the upgrade.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletNetDeployedSeeded {
    #[topic]
    pub admin: Address,
    #[topic]
    pub subaccount: Address,
    pub old_amount: i128,
    pub new_amount: i128,
}

pub fn emit_wallet_net_deployed_seeded(
    e: &Env,
    admin: &Address,
    subaccount: &Address,
    old_amount: i128,
    new_amount: i128,
) {
    WalletNetDeployedSeeded {
        admin: admin.clone(),
        subaccount: subaccount.clone(),
        old_amount,
        new_amount,
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepositToSubaccount {
    #[topic]
    pub operator: Address,
    #[topic]
    pub subaccount: Address,
    /// Amount the operator originally requested to transfer.
    pub requested_amount: i128,
    /// Amount actually moved out of the vault, measured by balance-differencing.
    /// May differ from `requested_amount` for fee-on-transfer or non-standard tokens.
    pub actual_amount: i128,
}

pub fn emit_deposit_to_subaccount(
    e: &Env,
    operator: &Address,
    subaccount: &Address,
    requested_amount: i128,
    actual_amount: i128,
) {
    DepositToSubaccount {
        operator: operator.clone(),
        subaccount: subaccount.clone(),
        requested_amount,
        actual_amount,
    }
    .publish(e);
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawFromSubaccount {
    #[topic]
    pub operator: Address,
    #[topic]
    pub subaccount: Address,
    pub requested_amount: i128,
    pub actual_amount: i128,
}

pub fn emit_withdraw_from_subaccount(
    e: &Env,
    operator: &Address,
    subaccount: &Address,
    requested_amount: i128,
    actual_amount: i128,
) {
    WithdrawFromSubaccount {
        operator: operator.clone(),
        subaccount: subaccount.clone(),
        requested_amount,
        actual_amount,
    }
    .publish(e);
}
