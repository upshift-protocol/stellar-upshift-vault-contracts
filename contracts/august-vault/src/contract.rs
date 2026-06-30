use soroban_sdk::{
    contract, contractimpl, panic_with_error, token, Address, Env, MuxedAddress, String,
};
use stellar_contract_utils::math::{mul_div_i128, Rounding};
use stellar_contract_utils::upgradeable::UpgradeableInternal;
use stellar_macros::Upgradeable;
use stellar_tokens::{
    fungible::{Base, FungibleToken, INSTANCE_EXTEND_AMOUNT, INSTANCE_TTL_THRESHOLD},
    vault::{emit_deposit, emit_withdraw, FungibleVault, Vault, VaultTokenError},
};

use crate::errors::VaultError;
use crate::events;
use crate::storage;
use crate::storage::SubaccountType;
use crate::strategy::StrategyClient;

#[derive(Upgradeable)]
#[contract]
#[allow(dead_code)] // Constructed by Soroban runtime, not user code
pub struct AugustVault;

impl UpgradeableInternal for AugustVault {
    fn _require_auth(e: &Env, operator: &Address) {
        storage::require_admin(e, operator);
    }
}

// ==================== Private helpers ====================
//
// These reimplement the OZ conversion math but use `custom_total_assets()`
// instead of `Vault::total_assets()`, which only reads the vault's local
// token balance. This sidesteps the static dispatch constraint where
// `Self::total_assets()` inside OZ's Vault impl always resolves to
// `Vault::total_assets()` at compile time.

impl AugustVault {
    /// Bump the contract's instance TTL so it stays alive on-chain.
    fn bump_instance(e: &Env) {
        e.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_EXTEND_AMOUNT);
    }

    /// The vault's on-chain token balance (excludes deployed capital).
    fn local_balance(e: &Env) -> i128 {
        let asset = Vault::query_asset(e);
        token::Client::new(e, &asset).balance(&e.current_contract_address())
    }

    /// The true total assets under management:
    ///
    /// ```text
    /// total_assets = vault_local_balance
    ///              + Σ strategy.get_balance()   // on-chain, per strategy
    ///              + deployed_assets             // wallet-attributed AUM
    ///                                            // (= Σ WalletNetDeployed, F4)
    /// ```
    ///
    /// **F4 (wallet-only commitment)**: `deployed_assets` is exclusively the
    /// aggregate of wallet-attributed value. Strategies self-report their
    /// full position (idle + deployed) via `IStrategy::get_balance()`, so
    /// `deployed_assets` never doubles as a carrier for strategy off-chain
    /// AUM — that role was ambiguous in the pre-F4 design and has been
    /// removed. The maintained invariant is:
    ///
    /// ```text
    /// Σ WalletNetDeployed[w] == deployed_assets
    /// ```
    ///
    /// Upheld by: `deposit_to_subaccount`, `withdraw_from_subaccount`,
    /// `update_wallet_deployed`, `update_wallet_deployed_batch`, and
    /// `remove_wallet_and_reconcile`, which all move tracker and aggregate
    /// by the same delta. Two escape hatches can break the invariant by
    /// design, for emergency storage fixes: `update_deployed_assets`
    /// (aggregate-only) and `seed_wallet_net_deployed` (per-wallet).
    /// `get_wallet_deployed_assets()` exposes Σ WalletNetDeployed for
    /// off-chain monitoring of divergence between the aggregate and the
    /// tracker sum.
    fn custom_total_assets(e: &Env) -> i128 {
        Self::total_assets_from(e, Self::local_balance(e))
    }

    /// Computes total assets from a pre-fetched local balance, avoiding a
    /// redundant cross-contract call when the caller already has it.
    fn total_assets_from(e: &Env, local_balance: i128) -> i128 {
        let strategy_balances = Self::query_strategy_balances(e);
        let deployed = storage::get_deployed_assets(e);
        let total = local_balance
            .checked_add(strategy_balances)
            .and_then(|v| v.checked_add(deployed))
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        // Defense-in-depth: unreachable under normal operation (all operands
        // are non-negative), but guards against storage corruption.
        if total < 0 {
            panic_with_error!(e, VaultError::MathOverflow);
        }
        total
    }

    /// Sum of `get_balance()` across all Strategy-type subaccounts.
    ///
    /// Calls `try_get_balance()` on each Strategy and maps any invocation
    /// failure (panic, non-i128 return, missing function, expired TTL) to
    /// `VaultError::StrategyUnreachable` (#21). This gives operators a
    /// stable diagnostic error code rather than the strategy's raw trap.
    ///
    /// Note on observability: Soroban rolls back contract events when a
    /// contract call panics, so we do not emit an event on the failure path
    /// — it would not persist. Off-chain monitoring should page on the
    /// `#21` error code appearing on failed vault transactions.
    ///
    /// **Operational risk (unchanged)**: a failing strategy still freezes
    /// `total_assets()`, `max_withdraw()`, `max_redeem()`, and every share/
    /// asset conversion that depends on `total_assets_from`. This is
    /// intentional — silently zeroing a broken strategy's contribution would
    /// under-report NAV and let a bug induce a share-price drop. Recovery:
    /// admin calls `remove_subaccount` (no cross-contract calls) to remove
    /// the broken strategy, after which vault operations resume.
    fn query_strategy_balances(e: &Env) -> i128 {
        let subs = storage::get_subaccounts(e);
        let mut total: i128 = 0;
        for sub in subs.iter() {
            if storage::get_subaccount_type(e, &sub) == SubaccountType::Strategy {
                let balance = match StrategyClient::new(e, &sub).try_get_balance() {
                    // Successful call with a value the vault understands.
                    Ok(Ok(b)) => b,
                    // Either the invocation trapped (outer Err) or the returned
                    // Val could not be decoded as i128 (inner Err). In either
                    // case the strategy cannot report a trustworthy balance;
                    // halt with a clean error code.
                    _ => panic_with_error!(e, VaultError::StrategyUnreachable),
                };
                if balance < 0 {
                    panic_with_error!(e, VaultError::NegativeStrategyBalance);
                }
                total = total
                    .checked_add(balance)
                    .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            }
        }
        total
    }

    /// assets → shares conversion using our custom total.
    /// Formula: shares = (assets × (totalSupply + 10^offset)) / (totalAssets + 1)
    fn convert_to_shares_with_rounding(e: &Env, assets: i128, rounding: Rounding) -> i128 {
        Self::convert_to_shares_with_total(e, assets, Self::custom_total_assets(e), rounding)
    }

    /// Same as `convert_to_shares_with_rounding` but accepts a pre-computed
    /// `total_assets` to avoid redundant cross-contract calls.
    fn convert_to_shares_with_total(
        e: &Env,
        assets: i128,
        total_assets: i128,
        rounding: Rounding,
    ) -> i128 {
        if assets < 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidAssetsAmount);
        }
        if assets == 0 {
            return 0;
        }

        let pow = 10_i128
            .checked_pow(Vault::get_decimals_offset(e))
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        let y = Base::total_supply(e)
            .checked_add(pow)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        let denominator = total_assets
            .checked_add(1)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        mul_div_i128(e, assets, y, denominator, rounding)
    }

    /// shares → assets conversion using our custom total.
    /// Formula: assets = (shares × (totalAssets + 1)) / (totalSupply + 10^offset)
    fn convert_to_assets_with_rounding(e: &Env, shares: i128, rounding: Rounding) -> i128 {
        Self::convert_to_assets_with_total(e, shares, Self::custom_total_assets(e), rounding)
    }

    /// Same as `convert_to_assets_with_rounding` but accepts a pre-computed
    /// `total_assets` to avoid redundant cross-contract calls.
    fn convert_to_assets_with_total(
        e: &Env,
        shares: i128,
        total_assets: i128,
        rounding: Rounding,
    ) -> i128 {
        if shares < 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidSharesAmount);
        }
        if shares == 0 {
            return 0;
        }

        let y = total_assets
            .checked_add(1)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        let pow = 10_i128
            .checked_pow(Vault::get_decimals_offset(e))
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        let denominator = Base::total_supply(e)
            .checked_add(pow)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        mul_div_i128(e, shares, y, denominator, rounding)
    }
}

// ==================== Constructor + Admin functions ====================

#[contractimpl]
impl AugustVault {
    pub fn __constructor(
        e: &Env,
        name: String,
        symbol: String,
        asset: Address,
        decimals_offset: u32,
        admin: Address,
    ) {
        Self::bump_instance(e);

        // Fail-fast: verify the asset address exposes a `decimals()` function
        // (basic SEP-41 sanity check — does not guarantee full compliance).
        token::TokenClient::new(e, &asset).decimals();

        // Minimum offset of 3 mitigates the first-depositor share inflation
        // attack by making the required donation 1000× the victim's deposit.
        // OZ's set_decimals_offset enforces offset <= 10
        // (VaultMaxDecimalsOffsetExceeded if exceeded).
        if decimals_offset < 3 {
            panic_with_error!(e, VaultError::InvalidDecimalsOffset);
        }
        Vault::set_asset(e, asset);
        Vault::set_decimals_offset(e, decimals_offset);
        Base::set_metadata(e, Self::decimals(e), name, symbol);

        // Initialize vault management state
        storage::set_admin(e, &admin);
        storage::set_deployed_assets(e, 0);

        events::emit_admin_set(e, &admin);
    }

    pub fn extend_ttl(e: &Env) {
        Self::bump_instance(e);
    }

    // ---- Admin functions ----

    /// Propose a new admin. The pending admin must call `accept_admin`
    /// before `deadline` (ledger timestamp) to complete the transfer.
    /// Revokes any previous pending proposal.
    pub fn propose_admin(e: &Env, admin: Address, new_admin: Address, deadline: u64) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        if new_admin == admin {
            panic_with_error!(e, VaultError::InvalidAdminProposal);
        }
        if deadline <= e.ledger().timestamp() {
            panic_with_error!(e, VaultError::AdminProposalExpired);
        }
        storage::set_pending_admin(e, &new_admin);
        storage::set_admin_proposal_expiry(e, deadline);
        events::emit_admin_transfer_proposed(e, &admin, &new_admin, deadline);
    }

    /// Accept admin role. Must be called by the address previously proposed
    /// via `propose_admin` before the deadline expires. Clears the pending
    /// proposal.
    pub fn accept_admin(e: &Env) {
        Self::bump_instance(e);
        let pending = storage::get_pending_admin(e)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::NoPendingAdmin));
        pending.require_auth();

        let expiry = storage::get_admin_proposal_expiry(e)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::AdminProposalExpired));
        if e.ledger().timestamp() > expiry {
            panic_with_error!(e, VaultError::AdminProposalExpired);
        }

        let old_admin = storage::get_admin(e);
        storage::set_admin(e, &pending);
        storage::remove_pending_admin(e);
        events::emit_admin_transfer_accepted(e, &old_admin, &pending);
    }

    /// Cancel a pending admin transfer proposal. Only the current admin
    /// can cancel.
    pub fn cancel_admin_proposal(e: &Env, admin: Address) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        if storage::get_pending_admin(e).is_none() {
            panic_with_error!(e, VaultError::NoPendingAdmin);
        }
        storage::remove_pending_admin(e);
        events::emit_admin_transfer_cancelled(e, &admin);
    }

    pub fn get_pending_admin(e: &Env) -> Option<Address> {
        storage::get_pending_admin(e)
    }

    /// Note: setting admin == operator concentrates governance and AUM
    /// reporting under a single key. Prefer separate keys in production.
    pub fn set_operator(e: &Env, admin: Address, new_operator: Address) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        let old_operator = storage::get_operator(e);
        storage::set_operator(e, &new_operator);
        events::emit_operator_set(e, &admin, &old_operator, &new_operator);
    }

    /// Per-wallet emergency escape hatch for the `WalletNetDeployed`
    /// tracker. **Restricted to emergency storage corrections** after F4:
    /// the standard operator path for gain/loss recognition and dust
    /// attribution is `update_wallet_deployed` (single wallet) or
    /// `update_wallet_deployed_batch` (multiple wallets atomically),
    /// which move the per-wallet tracker and the aggregate
    /// `deployed_assets` in lockstep and preserve the invariant
    /// `Σ WalletNetDeployed == deployed_assets`.
    ///
    /// Remaining legitimate uses:
    /// - Backfilling the tracker for a wallet registered before per-wallet
    ///   accounting was introduced (one-shot upgrade migration).
    /// - Correcting a divergence between `get_deployed_assets()` and
    ///   `get_wallet_deployed_assets()` after a storage anomaly or a prior
    ///   `update_deployed_assets` correction that left wallet attribution
    ///   out of sync.
    ///
    /// **Warning**: this function can break the wallet-tracker sum
    /// invariant by design. It does NOT touch the aggregate
    /// `deployed_assets` — use it only when the value being attributed is
    /// already recorded in the aggregate by another path. Off-chain
    /// monitoring should alert on divergence between the two views.
    ///
    /// Admin only. Only callable for addresses already registered as
    /// `Wallet` subaccounts — rejects Strategy (balance queried live)
    /// and unregistered addresses. Negative amounts rejected. Does not
    /// enforce `tracker <= deployed_assets`; violations surface as
    /// `DeployedAssetsUnderflow` on subsequent withdrawals.
    ///
    /// **C-2**: rejects `amount < current_tracker` (`WalletSeedBelowTracker`).
    /// Without this guard, an admin could zero the tracker via this path
    /// — which doesn't touch the aggregate — then call `remove_subaccount`
    /// (now passes the F5a `WalletTrackerNotZero` guard) and strand value
    /// in `deployed_assets` with no per-wallet attribution. Downward
    /// reconciliation must go through `update_wallet_deployed` (atomic
    /// aggregate + tracker move under operator auth) or
    /// `remove_wallet_and_reconcile` (atomic dual-auth removal).
    pub fn seed_wallet_net_deployed(e: &Env, admin: Address, subaccount: Address, amount: i128) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);

        if amount < 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        Self::require_whitelisted(e, &subaccount);
        if storage::get_subaccount_type(e, &subaccount) != SubaccountType::Wallet {
            // Strategy balances are queried live; seeding a tracker for
            // them is meaningless and would confuse future reconciliation.
            // Distinct from `InvalidAmount` so off-chain tooling can tell
            // a wrong-type call apart from a bad numeric input.
            panic_with_error!(e, VaultError::InvalidSubaccountType);
        }

        let old_amount = storage::get_wallet_net_deployed(e, &subaccount);
        if amount < old_amount {
            panic_with_error!(e, VaultError::WalletSeedBelowTracker);
        }
        storage::set_wallet_net_deployed(e, &subaccount, amount);
        events::emit_wallet_net_deployed_seeded(e, &admin, &subaccount, old_amount, amount);
    }

    /// Read the per-wallet net-deployed tracker for a subaccount.
    ///
    /// Returns 0 for unregistered addresses, for Strategy subaccounts, and
    /// for Wallet subaccounts that have never been deposited to. Useful for
    /// admins verifying the result of `seed_wallet_net_deployed` and for
    /// monitoring dashboards.
    pub fn get_wallet_net_deployed(e: &Env, subaccount: Address) -> i128 {
        storage::get_wallet_net_deployed(e, &subaccount)
    }

    pub fn set_aum_limits(e: &Env, admin: Address, increase_bps: u32, decrease_bps: u32) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        let old_increase = storage::get_aum_increase_limit(e);
        let old_decrease = storage::get_aum_decrease_limit(e);
        storage::set_aum_limits(e, increase_bps, decrease_bps);
        events::emit_aum_limits_changed(
            e,
            &admin,
            old_increase,
            old_decrease,
            increase_bps,
            decrease_bps,
        );
    }

    /// Configure the cumulative AUM rate-limit window.
    ///
    /// `window_duration` — length of the fixed window in seconds
    /// (min 3,600 = 1 hour, max 604,800 = 7 days). The window resets
    /// entirely on expiry (tumbling window, not sliding).
    ///
    /// `cumulative_increase_bps` / `cumulative_decrease_bps` — maximum
    /// cumulative change (in basis points of the `base_deployed` snapshot
    /// taken when the window resets) allowed within a single window.
    ///
    /// Changing limits resets the active window so the new configuration
    /// takes effect immediately with a clean cumulative state.
    pub fn set_aum_window_limits(
        e: &Env,
        admin: Address,
        window_duration: u64,
        cumulative_increase_bps: u32,
        cumulative_decrease_bps: u32,
    ) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        let old_duration = storage::get_aum_window_duration(e);
        let old_increase = storage::get_aum_cumulative_increase_limit(e);
        let old_decrease = storage::get_aum_cumulative_decrease_limit(e);
        storage::set_aum_window_limits(
            e,
            window_duration,
            cumulative_increase_bps,
            cumulative_decrease_bps,
        );

        // Reset window so stale counters don't interact with new limits.
        let now = e.ledger().timestamp();
        let current_deployed = storage::get_deployed_assets(e);
        storage::reset_aum_window(e, now, current_deployed);

        events::emit_aum_window_limits_changed(
            e,
            events::AumWindowLimitsChanged {
                admin,
                old_window_duration: old_duration,
                old_cumulative_increase_bps: old_increase,
                old_cumulative_decrease_bps: old_decrease,
                new_window_duration: window_duration,
                new_cumulative_increase_bps: cumulative_increase_bps,
                new_cumulative_decrease_bps: cumulative_decrease_bps,
            },
        );
    }

    pub fn pause(e: &Env, admin: Address) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        storage::set_paused(e, true);
        events::emit_paused(e, &admin);
    }

    pub fn unpause(e: &Env, admin: Address) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        storage::set_paused(e, false);
        events::emit_unpaused(e, &admin);
    }

    // ---- Read-only getters ----

    pub fn get_admin(e: &Env) -> Address {
        storage::get_admin(e)
    }

    pub fn get_operator(e: &Env) -> Option<Address> {
        storage::get_operator(e)
    }

    pub fn is_paused(e: &Env) -> bool {
        storage::is_paused(e)
    }

    pub fn get_deployed_assets(e: &Env) -> i128 {
        storage::get_deployed_assets(e)
    }

    pub fn get_aum_increase_limit(e: &Env) -> u32 {
        storage::get_aum_increase_limit(e)
    }

    pub fn get_aum_decrease_limit(e: &Env) -> u32 {
        storage::get_aum_decrease_limit(e)
    }

    /// Length of the cumulative AUM rate-limit window in seconds
    /// (default: 86,400 = 24 hours).
    pub fn get_aum_window_duration(e: &Env) -> u64 {
        storage::get_aum_window_duration(e)
    }

    /// Maximum cumulative AUM increase within a window, in basis points
    /// (default: 1,000 = 10%).
    pub fn get_aum_window_inc_limit(e: &Env) -> u32 {
        storage::get_aum_cumulative_increase_limit(e)
    }

    /// Maximum cumulative AUM decrease within a window, in basis points
    /// (default: 500 = 5%).
    pub fn get_aum_window_dec_limit(e: &Env) -> u32 {
        storage::get_aum_cumulative_decrease_limit(e)
    }

    /// Sum of `IStrategy::get_balance()` across all Strategy-type subaccounts.
    /// Useful for operators to see how much capital is visible on-chain in
    /// strategies vs. the wallet-attributed `deployed_assets`.
    pub fn get_strategy_balances(e: &Env) -> i128 {
        Self::query_strategy_balances(e)
    }

    /// Sum of `WalletNetDeployed` across all Wallet-type subaccounts
    /// currently in the whitelist.
    ///
    /// Exposes the Σ side of the F4 invariant
    /// `Σ WalletNetDeployed == deployed_assets` for off-chain monitoring.
    /// Under normal operation this returns the same value as
    /// `get_deployed_assets()`. Divergence indicates one of the
    /// documented escape hatches (`update_deployed_assets` or
    /// `seed_wallet_net_deployed`) has been used; the gap represents
    /// aggregate value not currently attributed to any wallet.
    ///
    /// Bounded cost: the subaccount list is capped at `MAX_SUBACCOUNTS`
    /// (= 10), so this iterates at most ten map lookups.
    pub fn get_wallet_deployed_assets(e: &Env) -> i128 {
        let subs = storage::get_subaccounts(e);
        let mut total: i128 = 0;
        for sub in subs.iter() {
            if storage::get_subaccount_type(e, &sub) == SubaccountType::Wallet {
                total = total
                    .checked_add(storage::get_wallet_net_deployed(e, &sub))
                    .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            }
        }
        total
    }

    /// Returns the list of currently whitelisted subaccount addresses.
    pub fn get_subaccounts(e: &Env) -> soroban_sdk::Vec<Address> {
        storage::get_subaccounts(e)
    }

    /// Returns the type of a registered subaccount (`Strategy` or `Wallet`).
    ///
    /// Panics with `SubaccountNotWhitelisted` if the address is not in the
    /// current whitelist. Defaults to `Strategy` for subaccounts registered
    /// before this field was introduced (backward compatibility).
    pub fn get_subaccount_type(e: &Env, subaccount: Address) -> SubaccountType {
        Self::require_whitelisted(e, &subaccount);
        storage::get_subaccount_type(e, &subaccount)
    }
}

// ==================== Subaccount Management ====================

impl AugustVault {
    /// Returns the index of `subaccount` in the current whitelist, or `None`.
    fn find_subaccount_index(
        subs: &soroban_sdk::Vec<Address>,
        subaccount: &Address,
    ) -> Option<u32> {
        (0..subs.len()).find(|&i| subs.get(i).unwrap() == *subaccount)
    }

    /// Panics with `SubaccountNotWhitelisted` if `subaccount` is not in the
    /// current whitelist.
    fn require_whitelisted(e: &Env, subaccount: &Address) {
        let subs = storage::get_subaccounts(e);
        if Self::find_subaccount_index(&subs, subaccount).is_none() {
            panic_with_error!(e, VaultError::SubaccountNotWhitelisted);
        }
    }

    /// Returns a `token::Client` for the vault's underlying asset.
    fn token_client(e: &Env) -> token::Client<'_> {
        let asset = Vault::query_asset(e);
        token::Client::new(e, &asset)
    }

    fn subaccounts_without_index(
        e: &Env,
        subs: &soroban_sdk::Vec<Address>,
        idx_to_remove: u32,
    ) -> soroban_sdk::Vec<Address> {
        let mut new_subs = soroban_sdk::Vec::new(e);
        for (i, sub) in subs.iter().enumerate() {
            if (i as u32) != idx_to_remove {
                new_subs.push_back(sub);
            }
        }
        new_subs
    }

    /// Checks that `value / base > limit_bps / 10_000` does NOT hold,
    /// i.e. the ratio is within the allowed limit. Uses cross-multiplication
    /// to avoid integer division truncation. Changes exactly at the limit
    /// are permitted (uses `>` not `>=`).
    ///
    /// Panics with `error_code` if the ratio exceeds the limit, or with
    /// `MathOverflow` on arithmetic overflow.
    fn require_within_bps_limit(
        e: &Env,
        value: i128,
        base: i128,
        limit_bps: u32,
        error_code: VaultError,
    ) {
        let lhs = value
            .checked_mul(10_000)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        let rhs = (limit_bps as i128)
            .checked_mul(base)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        if lhs > rhs {
            panic_with_error!(e, error_code);
        }
    }

    /// Enforces the per-call AUM rate limit: panics with
    /// `AumChangeExceedsLimit` if `delta / old_deployed > limit_bps / 10_000`.
    fn check_aum_rate_limit(e: &Env, delta: i128, old_deployed: i128, limit_bps: u32) {
        Self::require_within_bps_limit(
            e,
            delta,
            old_deployed,
            limit_bps,
            VaultError::AumChangeExceedsLimit,
        );
    }

    /// Checks and updates the cumulative AUM window tracker.
    ///
    /// Resets to a new window when the previous one has expired (or on
    /// first call). Then accumulates `delta` into the appropriate tracker
    /// and panics with `AumCumulativeChangeExceedsLimit` if the cumulative
    /// change within the window exceeds the configured limit.
    fn check_aum_cumulative_limit(e: &Env, delta: i128, old_deployed: i128, is_increase: bool) {
        let now = e.ledger().timestamp();
        let window_duration = storage::get_aum_window_duration(e);
        let window_start = storage::get_aum_window_start(e);

        // Reset window if expired or never initialized (window_start == 0).
        if window_start == 0 || now.saturating_sub(window_start) >= window_duration {
            storage::reset_aum_window(e, now, old_deployed);
        }

        let base_deployed = storage::get_aum_window_base_deployed(e);

        // Skip cumulative check if base is 0: percentage-based limits are
        // undefined at zero. This means the first deployment from a clean state
        // is unrestricted within the window, matching the per-call limiter's
        // behavior (which also skips when old_deployed == 0). The admin
        // controls risk at the zero boundary via operator key management and
        // subaccount whitelisting.
        if base_deployed == 0 {
            return;
        }

        let (cumulative, limit_bps) = if is_increase {
            (
                storage::get_aum_cumulative_increase(e),
                storage::get_aum_cumulative_increase_limit(e),
            )
        } else {
            (
                storage::get_aum_cumulative_decrease(e),
                storage::get_aum_cumulative_decrease_limit(e),
            )
        };

        let new_cumulative = cumulative
            .checked_add(delta)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        Self::require_within_bps_limit(
            e,
            new_cumulative,
            base_deployed,
            limit_bps,
            VaultError::AumCumulativeChangeExceedsLimit,
        );

        if is_increase {
            storage::set_aum_cumulative_increase(e, new_cumulative);
        } else {
            storage::set_aum_cumulative_decrease(e, new_cumulative);
        }
    }
}

#[contractimpl]
impl AugustVault {
    /// Register a new subaccount with the vault.
    ///
    /// Requires admin authorization and a non-paused vault. For `Strategy`
    /// subaccounts, performs interface validation: zero-amount smoke-tests
    /// (`deposit`, `withdraw`), calls `get_balance()` (rejecting negative
    /// values), and verifies the strategy's asset matches the vault's via
    /// `get_asset()`. `Wallet` subaccounts skip all checks. Capped at
    /// `MAX_SUBACCOUNTS` entries.
    pub fn add_subaccount(
        e: &Env,
        admin: Address,
        subaccount: Address,
        subaccount_type: SubaccountType,
    ) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        storage::require_not_paused(e);

        let mut subs = storage::get_subaccounts(e);

        if Self::find_subaccount_index(&subs, &subaccount).is_some() {
            panic_with_error!(e, VaultError::SubaccountAlreadyRegistered);
        }

        if subs.len() >= storage::MAX_SUBACCOUNTS {
            panic_with_error!(e, VaultError::MaxSubaccountsReached);
        }

        // Smoke-test only for Strategy subaccounts: verify the address exposes
        // a compatible IStrategy interface and that the strategy's configured
        // asset matches the vault's asset. Probes zero-amount deposit/withdraw,
        // get_balance, get_local_balance (F2), and get_asset so a strategy
        // missing any of them is rejected at registration rather than failing
        // later in production.
        if subaccount_type == SubaccountType::Strategy {
            let vault_addr = e.current_contract_address();
            let strategy = StrategyClient::new(e, &subaccount);
            strategy.deposit(&vault_addr, &0);
            strategy.withdraw(&vault_addr, &0);
            let balance = strategy.get_balance();
            if balance < 0 {
                panic_with_error!(e, VaultError::NegativeStrategyBalance);
            }
            // F2: exercise the new interface method so strategies that
            // pre-date F2 (or mocks that forgot to implement it) cannot be
            // whitelisted. A negative return is as invalid as for
            // `get_balance` — local balance is a non-negative quantity.
            let local = strategy.get_local_balance();
            if local < 0 {
                panic_with_error!(e, VaultError::NegativeStrategyBalance);
            }
            let strategy_asset = strategy.get_asset();
            if strategy_asset != Vault::query_asset(e) {
                panic_with_error!(e, VaultError::AssetMismatch);
            }
        }

        subs.push_back(subaccount.clone());
        storage::set_subaccounts(e, &subs);
        storage::set_subaccount_type(e, &subaccount, subaccount_type.clone());

        events::emit_subaccount_added(e, &admin, &subaccount, &subaccount_type);
    }

    /// Removes a subaccount from the vault's whitelist. Whitelist-and-
    /// tracker cleanup only — does NOT adjust `deployed_assets`.
    ///
    /// For Wallet subaccounts with non-zero attributed value, use
    /// `remove_wallet_and_reconcile`: it does the aggregate write-down and
    /// the removal atomically under dual auth, closing the pricing window
    /// of the two-step sequence (`update_deployed_assets` +
    /// `remove_subaccount`) where NAV is wrong between transactions.
    /// `remove_subaccount` rejects Wallet subaccounts whose
    /// `WalletNetDeployed` tracker is non-zero (`WalletTrackerNotZero`) to
    /// prevent that exact window being opened by mistake.
    ///
    /// Remaining use cases for `remove_subaccount`:
    /// - Strategy removal (balances tracked live via `get_balance()`).
    /// - Wallet cleanup after a full pull via `withdraw_from_subaccount`
    ///   has zeroed the tracker.
    /// - Emergency decommissioning where the operator key is unavailable:
    ///   the admin must first rotate the operator (via
    ///   `set_operator`) and then go through `remove_wallet_and_reconcile`
    ///   or `update_wallet_deployed` to zero the tracker. There is no
    ///   admin-only path to zero a tracker, by design (C-2): that would
    ///   let an admin strand value in `deployed_assets`.
    ///
    /// Intentionally skips pause check: removal may be needed for
    /// decommissioning strategies/wallets during emergencies.
    ///
    /// **Degraded-token resilience**: for Wallet subaccounts the diagnostic
    /// `token.balance(subaccount)` call is wrapped in `try_balance`, so a
    /// trapped or undecodable response (e.g. token instance TTL expired)
    /// only skips the advisory `WalletBalanceDiverged` event (emitting
    /// `WalletBalanceProbeFailed` instead so the degraded state is still
    /// observable). Removal itself always succeeds, so the admin can
    /// decommission a wallet even during a token outage.
    ///
    /// The wallet's per-wallet net-deployed tracker
    /// (`WalletNetDeployed[subaccount]`) is cleared so a future
    /// re-registration starts fresh.
    pub fn remove_subaccount(e: &Env, admin: Address, subaccount: Address) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);

        // Reject removal of any subaccount whose per-wallet tracker is
        // non-zero. The state-based check (not type-based) closes two
        // gaps identified in review:
        //   - (primary intent) Wallet removals with attributed value
        //     strand `deployed_assets` or open a pricing window if
        //     reconciled in a separate tx. Use `remove_wallet_and_reconcile`
        //     for atomic dual-auth cleanup, or drain via
        //     `withdraw_from_subaccount` until the tracker reaches zero.
        //   - (defense-in-depth) A storage-corrupted `SubaccountType`
        //     that misreports a Wallet as Strategy would otherwise
        //     bypass the guard entirely. Strategy subaccounts never
        //     legitimately have non-zero `WalletNetDeployed`, so checking
        //     unconditionally is safe and strictly more conservative.
        //
        // Checked here — not in `do_remove_subaccount` — so the atomic
        // reconcile path still works for wallets that legitimately have
        // a non-zero tracker.
        if storage::get_wallet_net_deployed(e, &subaccount) != 0 {
            panic_with_error!(e, VaultError::WalletTrackerNotZero);
        }

        Self::do_remove_subaccount(e, &admin, &subaccount);
    }

    /// Shared removal logic: whitelist + type cleanup, Wallet diagnostic
    /// events, tracker cleanup, terminal `SubaccountRemoved` event. Assumes
    /// the caller has already authenticated the `remover` address. Used by
    /// `remove_subaccount` (admin-only) and `remove_wallet_and_reconcile`
    /// (admin + operator co-auth).
    fn do_remove_subaccount(e: &Env, remover: &Address, subaccount: &Address) {
        let subs = storage::get_subaccounts(e);

        let idx = match Self::find_subaccount_index(&subs, subaccount) {
            Some(i) => i,
            None => panic_with_error!(e, VaultError::SubaccountNotWhitelisted),
        };

        let new_subs = Self::subaccounts_without_index(e, &subs, idx);
        // Read the type before removing it from storage (needed for the event).
        let sub_type = storage::get_subaccount_type(e, subaccount);
        storage::set_subaccounts(e, &new_subs);
        storage::remove_subaccount_type(e, subaccount);

        // Clean up the per-wallet tracker entry if present. This happens
        // for both Strategy and Wallet types for safety (Strategy entries
        // should never exist, but a stray entry from storage corruption
        // or a future refactor won't leak).
        //
        // For wallets, emit a `WalletBalanceDiverged` advisory when the
        // vault-controlled tracker disagrees with the wallet's on-chain
        // balance. Divergence is expected (airdrops, external transfers,
        // irrecoverable loss), not a bug — raising would strand wallets
        // that cannot be reconciled back to the tracker. Aggregate NAV
        // reconciliation is a separate admin step via
        // `update_deployed_assets` (see doc comment above). If the probe
        // itself fails (trap, undecodable response, expired token
        // instance), emit `WalletBalanceProbeFailed` so the degraded
        // state is still observable; removal must never be blocked by
        // an advisory event.
        let mut strategy_balance: Option<i128> = None;

        if sub_type == SubaccountType::Wallet {
            let tracked = storage::get_wallet_net_deployed(e, subaccount);
            match Self::token_client(e).try_balance(subaccount) {
                Ok(Ok(observed)) if observed != tracked => {
                    events::emit_wallet_balance_diverged(e, subaccount, tracked, observed);
                }
                Ok(Ok(_)) => {}
                _ => {
                    events::emit_wallet_balance_probe_failed(e, subaccount, tracked);
                }
            }
        } else {
            // F5b: capture the strategy's self-reported balance at removal
            // time so the `SubaccountRemoved` event is the on-chain record
            // linking the share-price impact to this transaction. Use
            // `try_get_balance` so a broken strategy (panics, non-i128
            // return) cannot block removal — a broken strategy is exactly
            // the recovery case this function must handle.
            //
            // I-4: distinguish the two failure modes (`InvokeError` —
            // trap / missing entry point; `ConvertError` — returned a
            // non-i128) via a separate `StrategyBalanceProbeFailed`
            // event. `SubaccountRemoved` keeps a single `None` encoding
            // for both, but indexers can cross-reference the probe-
            // failure event for the specific reason.
            strategy_balance = match StrategyClient::new(e, subaccount).try_get_balance() {
                Ok(Ok(b)) => Some(b),
                Ok(Err(_)) => {
                    events::emit_strategy_balance_probe_failed(
                        e,
                        subaccount,
                        events::StrategyProbeFailure::ConvertError,
                    );
                    None
                }
                Err(_) => {
                    events::emit_strategy_balance_probe_failed(
                        e,
                        subaccount,
                        events::StrategyProbeFailure::InvokeError,
                    );
                    None
                }
            };
        }
        storage::remove_wallet_net_deployed(e, subaccount);

        let deployed_assets = storage::get_deployed_assets(e);
        events::emit_subaccount_removed(
            e,
            remover,
            subaccount,
            deployed_assets,
            strategy_balance,
            &sub_type,
        );
    }

    /// Atomically reconcile the aggregate `deployed_assets` counter AND
    /// remove a Wallet subaccount in a single transaction.
    ///
    /// Closes the pricing window between the two-step sequence:
    ///   1. `update_deployed_assets(operator, new_total)`
    ///   2. `remove_subaccount(admin, wallet)`
    ///
    /// Between those transactions NAV is wrong in one direction or the
    /// other (write-down first: NAV under-reports while tokens still sit
    /// at the wallet → deposits mint too many shares; remove first: NAV
    /// over-reports against stranded value → redeems overpay). Combining
    /// both into one call eliminates the window.
    ///
    /// Requires both `admin` (whitelist authority) and `operator` (AUM
    /// authority) to authorise — the combined function unions their
    /// permissions without granting either role unilateral access to the
    /// other's powers. When a single entity holds both keys (documented
    /// supported configuration on `set_operator`), the operator's
    /// `require_auth` is skipped to avoid Soroban's `Auth, ExistingValue`
    /// error that otherwise fires on a repeated auth of the same address.
    /// Subject to the same AUM rate limits and pause semantics as
    /// `update_deployed_assets`.
    ///
    /// Wallet-only: rejects Strategy subaccounts with
    /// `InvalidSubaccountType` (Strategy balances are queried live via
    /// `get_balance()`, so they don't need aggregate reconciliation on
    /// removal — use `remove_subaccount` directly).
    ///
    /// Always emits `DeployedAssetsChanged` (via `apply_deployed_assets_change`)
    /// even when `new_deployed_total == old_deployed` — the no-op reconcile
    /// case still produces an audit trail, which matters because the
    /// tracker for the removed wallet is dropped by the subsequent removal
    /// step and the aggregate-level event is the only place that records
    /// the transaction touched wallet-attributed state.
    pub fn remove_wallet_and_reconcile(
        e: &Env,
        admin: Address,
        operator: Address,
        subaccount: Address,
        new_deployed_total: i128,
    ) {
        Self::bump_instance(e);
        storage::require_admin(e, &admin);
        // Skip operator's require_auth if admin == operator (single-key
        // configuration). Repeated auth on the same Address in one frame
        // trips Soroban's host-level `Auth, ExistingValue` error. Still
        // verify the role against storage so a compromised admin can't
        // bypass the operator check by passing themselves in both slots.
        if admin != operator {
            storage::require_operator(e, &operator);
        } else {
            match storage::get_operator(e) {
                Some(op) if operator == op => {}
                _ => panic_with_error!(e, VaultError::Unauthorized),
            }
        }

        if new_deployed_total < 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        Self::require_whitelisted(e, &subaccount);
        if storage::get_subaccount_type(e, &subaccount) != SubaccountType::Wallet {
            panic_with_error!(e, VaultError::InvalidSubaccountType);
        }

        let old_deployed = storage::get_deployed_assets(e);
        // Always call — emits the DeployedAssetsChanged audit trail even on
        // a no-op reconcile so off-chain monitoring can tell the tracker
        // was dropped as part of this transaction.
        Self::apply_deployed_assets_change(e, &operator, old_deployed, new_deployed_total);
        Self::do_remove_subaccount(e, &admin, &subaccount);
    }

    /// Transfer `amount` of the underlying token from the vault to `subaccount`.
    ///
    /// For `Strategy` subaccounts, notifies the strategy via `IStrategy::deposit`
    /// after the transfer. The strategy's balance is captured live by
    /// `IStrategy::get_balance()` in `total_assets`, so `deployed_assets` is NOT
    /// incremented. `Wallet` subaccounts receive the tokens directly with no
    /// further notification; their balance is not queried on-chain, so
    /// `deployed_assets` IS incremented. No AUM rate limit is applied —
    /// subaccount transfers don't change total_assets.
    ///
    /// Requires operator authorization, a non-paused vault, and a whitelisted
    /// subaccount.
    pub fn deposit_to_subaccount(e: &Env, operator: Address, subaccount: Address, amount: i128) {
        Self::bump_instance(e);
        storage::require_operator(e, &operator);
        storage::require_not_paused(e);

        if amount <= 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        Self::require_whitelisted(e, &subaccount);

        // Balance verification: measure the vault's balance change rather than
        // trusting that the transfer moved the exact amount. This guards against
        // fee-on-transfer or non-standard token behavior.
        let token_client = Self::token_client(e);
        let vault_addr = e.current_contract_address();

        let balance_before = token_client.balance(&vault_addr);
        token_client.transfer(&vault_addr, &subaccount, &amount);
        let balance_after = token_client.balance(&vault_addr);

        let actual_sent = balance_before
            .checked_sub(balance_after)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        if actual_sent <= 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        let sub_type = storage::get_subaccount_type(e, &subaccount);

        // Notify the strategy that tokens have arrived. The tokens are already
        // in the strategy's balance at this point.
        // Wallet subaccounts need no notification — they just hold the tokens.
        if sub_type == SubaccountType::Strategy {
            StrategyClient::new(e, &subaccount).deposit(&vault_addr, &actual_sent);
        }

        // Strategy balances are queried live via get_balance() in total_assets,
        // so only Wallet subaccounts need deployed_assets bookkeeping. Also
        // update the per-wallet net-deployed tracker so removal can reconcile
        // using vault-controlled accounting instead of the wallet's external
        // on-chain balance.
        if sub_type == SubaccountType::Wallet {
            let new_deployed = storage::get_deployed_assets(e)
                .checked_add(actual_sent)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            storage::set_deployed_assets(e, new_deployed);

            let new_wallet_net = storage::get_wallet_net_deployed(e, &subaccount)
                .checked_add(actual_sent)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            storage::set_wallet_net_deployed(e, &subaccount, new_wallet_net);
        }

        events::emit_deposit_to_subaccount(e, &operator, &subaccount, amount, actual_sent);
    }

    /// Request `amount` of the underlying token back from `subaccount`.
    ///
    /// For `Strategy` subaccounts, calls `IStrategy::withdraw` (push model).
    /// Strategy balances are queried live via `get_balance()`, so
    /// `deployed_assets` is NOT decremented.
    ///
    /// For `Wallet` subaccounts, pulls tokens via `transfer_from` using a
    /// pre-approved SEP-41 allowance. The wallet owner (EOA or contract) must
    /// have called `token.approve(wallet, vault, amount, ledger)` beforehand.
    /// `deployed_assets` IS decremented for wallets.
    ///
    /// Measures actual tokens received via balance-differencing. No AUM rate
    /// limit is applied — subaccount transfers don't change total_assets.
    ///
    /// Requires operator authorization, a non-paused vault, and a whitelisted
    /// subaccount.
    pub fn withdraw_from_subaccount(e: &Env, operator: Address, subaccount: Address, amount: i128) {
        Self::bump_instance(e);
        storage::require_operator(e, &operator);
        storage::require_not_paused(e);

        if amount <= 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        Self::require_whitelisted(e, &subaccount);

        // Balance verification: measure the vault's balance change rather than
        // trusting the strategy's return value. Rejects the call if nothing was
        // actually received.
        let token_client = Self::token_client(e);
        let vault_addr = e.current_contract_address();

        let balance_before = token_client.balance(&vault_addr);

        let sub_type = storage::get_subaccount_type(e, &subaccount);

        match sub_type {
            SubaccountType::Strategy => {
                // Push model: ask strategy to send tokens back
                StrategyClient::new(e, &subaccount).withdraw(&vault_addr, &amount);
            }
            SubaccountType::Wallet => {
                // Pull model: vault pulls tokens via pre-approved allowance.
                // The wallet owner must have approved the vault as spender.
                // Allowance is enforced atomically by the token's transfer_from.
                token_client.transfer_from(&vault_addr, &subaccount, &vault_addr, &amount);
            }
        }

        let balance_after = token_client.balance(&vault_addr);

        let actual_received = balance_after
            .checked_sub(balance_before)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        if actual_received <= 0 {
            panic_with_error!(e, VaultError::SubaccountReturnedNothing);
        }

        // Strategy balances are queried live via get_balance() in total_assets,
        // so only Wallet subaccounts need deployed_assets bookkeeping.
        //
        // A Wallet pull must not exceed the per-wallet tracker. An over-pull
        // means the operator has recognised a gain or is pulling external
        // dust without first reconciling via `update_wallet_deployed` —
        // allowing it to proceed would silently consume another wallet's
        // share of `deployed_assets` in multi-wallet vaults (later surfaces
        // as `DeployedAssetsUnderflow` on an innocent wallet's pull). The
        // panic forces the operator onto the explicit reconciliation path
        // where tracker and aggregate move together.
        //
        // Note: the only way a Wallet subaccount's tracker reads as 0 is if
        // (a) no deposit_to_subaccount or update_wallet_deployed has ever
        // run for it, or (b) its on-chain value was zeroed via withdraw.
        // In both cases the correct response to an attempted pull is to
        // reject — there is nothing for the vault to give.
        if sub_type == SubaccountType::Wallet {
            let tracked = storage::get_wallet_net_deployed(e, &subaccount);
            if actual_received > tracked {
                panic_with_error!(e, VaultError::WalletOverWithdraw);
            }

            // `actual_received <= tracked`, so the subtraction is exact.
            let new_wallet_net = tracked - actual_received;
            storage::set_wallet_net_deployed(e, &subaccount, new_wallet_net);

            let old_deployed = storage::get_deployed_assets(e);
            let new_deployed = old_deployed
                .checked_sub(actual_received)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            // Reachable when an admin action has driven the aggregate below
            // a wallet's tracker (e.g. `seed_wallet_net_deployed` without a
            // matching `update_deployed_assets` write-up, or an aggregate
            // write-down via `update_deployed_assets` that didn't account
            // for tracker sums). Documented admin-responsibility cases,
            // not corruption.
            if new_deployed < 0 {
                panic_with_error!(e, VaultError::DeployedAssetsUnderflow);
            }
            storage::set_deployed_assets(e, new_deployed);
        }

        events::emit_withdraw_from_subaccount(e, &operator, &subaccount, amount, actual_received);
    }

    /// Aggregate-only emergency escape hatch for `deployed_assets`.
    ///
    /// **Deprecated for routine use** after F4: strategies self-report
    /// their full position (idle + deployed) via `IStrategy::get_balance()`,
    /// so `deployed_assets` no longer has a legitimate "strategy off-chain
    /// AUM" component. The standard operator path for wallet-attributed
    /// reconciliation is `update_wallet_deployed` (single wallet) or
    /// `update_wallet_deployed_batch` (multiple wallets atomically) — both
    /// move the per-wallet tracker and the aggregate in lockstep, so the
    /// invariant `Σ WalletNetDeployed == deployed_assets` is preserved.
    ///
    /// Remaining legitimate uses:
    /// - Recovery from a storage anomaly where the invariant has already
    ///   broken and the aggregate needs a correction that cannot be
    ///   attributed to specific wallets.
    /// - Temporary aggregate correction during a multi-step migration that
    ///   will restore the invariant in a follow-up transaction.
    ///
    /// **Warning**: this function can break the wallet-tracker sum
    /// invariant by design. Off-chain monitoring should compare
    /// `get_deployed_assets()` against `get_wallet_deployed_assets()` and
    /// alert on divergence.
    ///
    /// Emergency write-offs: with the default 5% decrease limit, fully
    /// writing off a loss (setting deployed_assets to 0) requires the
    /// admin to first widen the limit via `set_aum_limits`, or multiple
    /// calls to step down.
    pub fn update_deployed_assets(e: &Env, operator: Address, amount: i128) {
        Self::bump_instance(e);
        storage::require_operator(e, &operator);

        if amount < 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        // Always route through `apply_deployed_assets_change` — it emits
        // `DeployedAssetsChanged` unconditionally, so a call that happens
        // to arrive with the current value (e.g. double-submit, precision
        // rounding that collapsed to the stored scaled value) produces a
        // zero-delta audit event instead of silently succeeding.
        let old_deployed = storage::get_deployed_assets(e);
        Self::apply_deployed_assets_change(e, &operator, old_deployed, amount);
    }

    /// Shared machinery for transitioning `deployed_assets` from `old` to
    /// `new`: partial overflow check, pause-gated increase rejection, AUM
    /// per-call and cumulative rate limits, the storage write, and the
    /// `DeployedAssetsChanged` event emission. Used by every function that
    /// mutates the aggregate counter (`update_deployed_assets`,
    /// `update_wallet_deployed`, `update_wallet_deployed_batch`,
    /// `remove_wallet_and_reconcile`).
    ///
    /// Emitting the event here — rather than deferring to each caller —
    /// makes the invariant "every aggregate move produces a
    /// `DeployedAssetsChanged` event" compile-enforceable: off-chain
    /// indexers that track NAV by subscribing to one event name see every
    /// change regardless of entry point.
    ///
    /// `actor` is recorded in the event as the address that caused the
    /// change — operator for update functions, operator in the dual-auth
    /// `remove_wallet_and_reconcile` case.
    ///
    /// Expects `new >= 0`. Safe to call with `old == new`: every caller
    /// routes through unconditionally, so same-value calls (double-submit,
    /// precision rounding that collapses to the stored scaled value)
    /// still emit a zero-delta `DeployedAssetsChanged` event as an audit
    /// trail instead of looking silently successful to the frontend.
    ///
    /// Partial overflow check: verifies `local + new` fits in i128 but does
    /// NOT fully validate `total_assets` (which also includes
    /// `strategy_balances`). Coupling this to strategy health via
    /// `query_strategy_balances` would freeze the function when a strategy
    /// is broken — the exact case where it is needed. Remaining overflow is
    /// caught by `total_assets_from()`'s `checked_add`, which surfaces as a
    /// clean `MathOverflow` panic rather than silent corruption.
    fn apply_deployed_assets_change(
        e: &Env,
        actor: &Address,
        old_deployed: i128,
        new_deployed: i128,
    ) {
        let local = Self::local_balance(e);
        let _ = local
            .checked_add(new_deployed)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        // H-2 fix: while paused, only allow decreasing deployed assets. This
        // prevents a compromised operator from inflating share prices during
        // an emergency pause. Decreases remain permitted so the operator can
        // reconcile after recovering funds.
        if storage::is_paused(e) && new_deployed > old_deployed {
            panic_with_error!(e, VaultError::VaultPaused);
        }

        // AUM rate limiting — enforce only when value actually changed.
        // old_deployed == 0: percentage-based rate limiting is undefined when
        // the baseline is zero. Changes at the zero boundary are unrestricted;
        // the admin controls risk via operator key management and subaccount
        // whitelisting.
        if old_deployed > 0 && new_deployed != old_deployed {
            if new_deployed > old_deployed {
                let increase = new_deployed
                    .checked_sub(old_deployed)
                    .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
                Self::check_aum_rate_limit(
                    e,
                    increase,
                    old_deployed,
                    storage::get_aum_increase_limit(e),
                );
                Self::check_aum_cumulative_limit(e, increase, old_deployed, true);
            } else {
                let decrease = old_deployed
                    .checked_sub(new_deployed)
                    .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
                Self::check_aum_rate_limit(
                    e,
                    decrease,
                    old_deployed,
                    storage::get_aum_decrease_limit(e),
                );
                Self::check_aum_cumulative_limit(e, decrease, old_deployed, false);
            }
        }

        storage::set_deployed_assets(e, new_deployed);
        events::emit_deployed_assets_changed(e, actor, old_deployed, new_deployed);
    }

    /// Reconcile a single Wallet subaccount's attributed value. Atomically
    /// moves both the per-wallet tracker (to `new_tracked`) and the
    /// aggregate `deployed_assets` counter (by the same delta), so
    /// subsequent `withdraw_from_subaccount` calls can pull the full value
    /// without tripping the per-wallet over-pull check.
    ///
    /// Use cases:
    ///   - Recognising a gain (wallet appreciated): `new_tracked > current`.
    ///   - Recognising a loss (wallet depreciated): `new_tracked < current`.
    ///   - Recognising external dust the operator wants to recover:
    ///     `new_tracked = current + dust_amount`, then pull.
    ///
    /// Subject to the same AUM rate limits and pause semantics as
    /// `update_deployed_assets`: increases are rejected while paused;
    /// decreases remain allowed for loss recovery.
    ///
    /// For aggregate-only reconciliation (e.g. strategy external AUM not
    /// attributable to a specific wallet) use `update_deployed_assets`
    /// instead.
    pub fn update_wallet_deployed(
        e: &Env,
        operator: Address,
        subaccount: Address,
        new_tracked: i128,
    ) {
        Self::bump_instance(e);
        storage::require_operator(e, &operator);

        if new_tracked < 0 {
            panic_with_error!(e, VaultError::InvalidAmount);
        }

        Self::require_whitelisted(e, &subaccount);
        if storage::get_subaccount_type(e, &subaccount) != SubaccountType::Wallet {
            panic_with_error!(e, VaultError::InvalidSubaccountType);
        }

        // Always produce an audit event — even on delta==0 (double-submit,
        // precision rounding) — so successful notifications correspond to
        // real on-chain records. The shared helper emits
        // `DeployedAssetsChanged` on every call; we additionally emit
        // `WalletDeployedUpdated` here so per-wallet observers see a
        // record even when the aggregate didn't move.
        let old_tracked = storage::get_wallet_net_deployed(e, &subaccount);
        let delta = new_tracked
            .checked_sub(old_tracked)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));

        let old_deployed = storage::get_deployed_assets(e);
        let new_deployed = old_deployed
            .checked_add(delta)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        if new_deployed < 0 {
            panic_with_error!(e, VaultError::DeployedAssetsUnderflow);
        }

        Self::apply_deployed_assets_change(e, &operator, old_deployed, new_deployed);
        storage::set_wallet_net_deployed(e, &subaccount, new_tracked);
        events::emit_wallet_deployed_updated(
            e,
            &operator,
            &subaccount,
            old_tracked,
            new_tracked,
            delta,
        );
    }

    /// Reconcile multiple Wallet subaccounts atomically in a single call.
    /// Introduced as the F4 primary operator path for wallet-attributed
    /// reconciliation: replaces ad-hoc sequences of `update_wallet_deployed`
    /// calls with one transaction that moves the aggregate exactly once
    /// (so the AUM rate limit is applied to the net delta rather than to
    /// each intermediate step).
    ///
    /// Each `(subaccount, new_tracked)` entry is validated independently
    /// (whitelisted, Wallet type, non-negative tracker). The net delta
    /// `Σ(new_tracked - old_tracked)` is applied to `deployed_assets` via
    /// the shared `apply_deployed_assets_change` helper, so the AUM rate
    /// limit is measured against the net (a batch with +100k on wallet A
    /// and -100k on wallet B passes even when each individual step would
    /// exceed the limit). Pause is enforced **per entry**: while paused,
    /// any positive entry delta panics with `VaultPaused` regardless of
    /// the batch's net — this matches `update_wallet_deployed` (where
    /// per-entry and net deltas coincide) and prevents a paused vault
    /// from accepting attribution shifts that inflate any individual
    /// tracker. The aggregate-level pause check downstream still runs
    /// but is redundant given the per-entry guard.
    ///
    /// Validation rules (all-or-nothing — any failure reverts the whole
    /// batch, no partial updates reach storage):
    /// - Non-empty batch (`EmptyBatch`, #26). Empty calls are almost
    ///   always a client bug and silently succeeding would mask it.
    /// - No duplicate subaccount addresses (`DuplicateSubaccountInBatch`,
    ///   #27). Duplicates would make the net delta depend on pair order.
    /// - Each subaccount whitelisted and Wallet type
    ///   (`SubaccountNotWhitelisted` #4, `InvalidSubaccountType` #22).
    /// - Each `new_tracked >= 0` (`InvalidAmount` #3).
    ///
    /// Emits one `WalletDeployedUpdated` per entry plus a single
    /// `WalletDeployedBatchApplied` (txn-level anchor for indexers) and
    /// the usual `DeployedAssetsChanged` from `apply_deployed_assets_change`.
    pub fn update_wallet_deployed_batch(
        e: &Env,
        operator: Address,
        updates: soroban_sdk::Vec<(Address, i128)>,
    ) {
        Self::bump_instance(e);
        storage::require_operator(e, &operator);

        let count = updates.len();
        if count == 0 {
            panic_with_error!(e, VaultError::EmptyBatch);
        }

        // Fast-fail on oversized input before the O(n²) duplicate scan.
        // Every valid entry must resolve to a whitelisted Wallet subaccount
        // (checked in the first pass below), and the whitelist itself is
        // capped at `MAX_SUBACCOUNTS = 10`, so a larger batch cannot
        // succeed. Rejecting up-front bounds the CPU cost at ~45
        // comparisons regardless of input size.
        if count > storage::MAX_SUBACCOUNTS {
            panic_with_error!(e, VaultError::MaxSubaccountsReached);
        }

        // Duplicate detection: O(n²) over at most MAX_SUBACCOUNTS entries
        // (= 10), so worst case is 45 address comparisons. A Map<Address, ()>
        // would be linear but the constant overhead is larger than the
        // bounded quadratic at n=10.
        for i in 0..count {
            let (addr_i, _) = updates.get(i).unwrap();
            for j in (i + 1)..count {
                let (addr_j, _) = updates.get(j).unwrap();
                if addr_i == addr_j {
                    panic_with_error!(e, VaultError::DuplicateSubaccountInBatch);
                }
            }
        }

        // First pass: validate every entry and accumulate the net delta.
        // Validation runs before any storage writes so a malformed entry
        // aborts the batch without partial state changes.
        //
        // Per-entry pause enforcement: the aggregate-level pause check
        // downstream is gated on the net delta, which lets a paused
        // batch like (+A, -B) through. Mirror the H-2 intent at entry
        // granularity by rejecting any positive delta while paused —
        // preserves the single-call semantics (`update_wallet_deployed`
        // already blocks per-entry increases because per-entry == net
        // there) and still permits multi-wallet loss recognition during
        // an emergency.
        let paused = storage::is_paused(e);
        let mut net_delta: i128 = 0;
        for entry in updates.iter() {
            let (subaccount, new_tracked) = entry;
            if new_tracked < 0 {
                panic_with_error!(e, VaultError::InvalidAmount);
            }
            Self::require_whitelisted(e, &subaccount);
            if storage::get_subaccount_type(e, &subaccount) != SubaccountType::Wallet {
                panic_with_error!(e, VaultError::InvalidSubaccountType);
            }
            let old_tracked = storage::get_wallet_net_deployed(e, &subaccount);
            let delta = new_tracked
                .checked_sub(old_tracked)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            if paused && delta > 0 {
                panic_with_error!(e, VaultError::VaultPaused);
            }
            net_delta = net_delta
                .checked_add(delta)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        }

        // Aggregate move first — if it trips pause / AUM rate limits /
        // underflow, we panic before touching any tracker, preserving the
        // all-or-nothing guarantee.
        let old_deployed = storage::get_deployed_assets(e);
        let new_deployed = old_deployed
            .checked_add(net_delta)
            .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
        if new_deployed < 0 {
            panic_with_error!(e, VaultError::DeployedAssetsUnderflow);
        }
        Self::apply_deployed_assets_change(e, &operator, old_deployed, new_deployed);

        // Second pass: commit each tracker and emit the per-wallet event.
        // `checked_sub` is redundant with the first pass (same operands,
        // apply_deployed_assets_change does not touch wallet trackers,
        // Soroban has no reentrancy) but unchecked arithmetic is a code
        // smell in a smart contract — keep the check for defense-in-depth.
        for entry in updates.iter() {
            let (subaccount, new_tracked) = entry;
            let old_tracked = storage::get_wallet_net_deployed(e, &subaccount);
            let delta = new_tracked
                .checked_sub(old_tracked)
                .unwrap_or_else(|| panic_with_error!(e, VaultError::MathOverflow));
            storage::set_wallet_net_deployed(e, &subaccount, new_tracked);
            events::emit_wallet_deployed_updated(
                e,
                &operator,
                &subaccount,
                old_tracked,
                new_tracked,
                delta,
            );
        }

        events::emit_wallet_deployed_batch_applied(e, &operator, count, net_delta);
    }
}

// ==================== Fungible Token (shares) ====================
//
// Implements the FungibleToken trait, delegating to OZ's Base for share
// accounting (transfer, balance, allowance, etc.). No pricing logic here.
//
// `transfer`, `transfer_from`, and `approve` extend Instance TTL before
// calling Base — the OZ library manages Persistent/Temporary TTL but
// leaves Instance TTL to the implementor.

#[contractimpl(contracttrait)]
impl FungibleToken for AugustVault {
    type ContractType = Vault;

    fn decimals(e: &Env) -> u32 {
        Vault::decimals(e)
    }

    fn transfer(e: &Env, from: Address, to: MuxedAddress, amount: i128) {
        Self::bump_instance(e);
        Base::transfer(e, &from, &to, amount);
    }

    fn transfer_from(e: &Env, spender: Address, from: Address, to: Address, amount: i128) {
        Self::bump_instance(e);
        Base::transfer_from(e, &spender, &from, &to, amount);
    }

    fn approve(e: &Env, owner: Address, spender: Address, amount: i128, live_until_ledger: u32) {
        Self::bump_instance(e);
        Base::approve(e, &owner, &spender, amount, live_until_ledger);
    }
}

// ==================== ERC-4626 Vault Surface (reimplemented) ====================
//
// Every method that touches share pricing is reimplemented here to use
// `custom_total_assets()` (local balance + strategy balances + deployed assets). Only
// `query_asset` keeps the trait default since it does not depend on `total_assets()`.
//
// OZ's low-level `deposit_internal` / `withdraw_internal` are called directly
// for token transfers and share minting/burning. The high-level deposit/mint/
// withdraw/redeem methods are fully reimplemented here (custom conversions,
// pause check, event emission).

#[contractimpl(contracttrait)]
impl FungibleVault for AugustVault {
    // ---- Reimplemented total_assets ----

    fn total_assets(e: &Env) -> i128 {
        Self::custom_total_assets(e)
    }

    // ---- Reimplemented conversions ----

    fn convert_to_shares(e: &Env, assets: i128) -> i128 {
        Self::convert_to_shares_with_rounding(e, assets, Rounding::Floor)
    }

    fn convert_to_assets(e: &Env, shares: i128) -> i128 {
        Self::convert_to_assets_with_rounding(e, shares, Rounding::Floor)
    }

    // ---- Reimplemented previews ----

    fn preview_deposit(e: &Env, assets: i128) -> i128 {
        Self::convert_to_shares_with_rounding(e, assets, Rounding::Floor)
    }

    fn preview_mint(e: &Env, shares: i128) -> i128 {
        Self::convert_to_assets_with_rounding(e, shares, Rounding::Ceil)
    }

    fn preview_withdraw(e: &Env, assets: i128) -> i128 {
        Self::convert_to_shares_with_rounding(e, assets, Rounding::Ceil)
    }

    fn preview_redeem(e: &Env, shares: i128) -> i128 {
        Self::convert_to_assets_with_rounding(e, shares, Rounding::Floor)
    }

    // ---- Reimplemented max functions ----
    //
    // Return 0 when paused. Additionally, max_withdraw and max_redeem are
    // capped by the vault's local token balance (excluding deployed capital),
    // since the vault can only transfer tokens it physically holds.

    fn max_deposit(e: &Env, _receiver: Address) -> i128 {
        if storage::is_paused(e) {
            return 0;
        }
        i128::MAX
    }

    fn max_mint(e: &Env, _receiver: Address) -> i128 {
        if storage::is_paused(e) {
            return 0;
        }
        i128::MAX
    }

    fn max_withdraw(e: &Env, owner: Address) -> i128 {
        if storage::is_paused(e) {
            return 0;
        }
        let local_balance = Self::local_balance(e);
        let total_assets = Self::total_assets_from(e, local_balance);
        let entitled = Self::convert_to_assets_with_total(
            e,
            Base::balance(e, &owner),
            total_assets,
            Rounding::Floor,
        );
        core::cmp::min(entitled, local_balance)
    }

    fn max_redeem(e: &Env, owner: Address) -> i128 {
        if storage::is_paused(e) {
            return 0;
        }
        let user_shares = Base::balance(e, &owner);
        let local_balance = Self::local_balance(e);
        let total_assets = Self::total_assets_from(e, local_balance);
        let shares_for_available =
            Self::convert_to_shares_with_total(e, local_balance, total_assets, Rounding::Floor);
        core::cmp::min(user_shares, shares_for_available)
    }

    // ---- Reimplemented deposit/mint/withdraw/redeem ----
    //
    // Same flow as OZ: validate → authorize operator → compute shares/assets →
    // call internal → emit event.
    // Differences: (1) uses our conversion math, (2) adds pause check,
    // (3) extends instance TTL, (4) guards against zero-value conversions,
    // (5) handles operator authorization directly (OZ internals are called
    // instead of OZ high-level methods which would handle auth themselves).

    fn deposit(e: &Env, assets: i128, receiver: Address, from: Address, operator: Address) -> i128 {
        Self::bump_instance(e);
        storage::require_not_paused(e);
        operator.require_auth();

        // ERC-4626 interface symmetry: max_deposit always returns i128::MAX
        // when not paused (already checked above), so this can never trigger.
        // Kept for spec compliance should max_deposit gain a real cap later.
        if assets > Self::max_deposit(e, receiver.clone()) {
            panic_with_error!(e, VaultTokenError::VaultExceededMaxDeposit);
        }

        let shares = Self::preview_deposit(e, assets);
        if assets > 0 && shares == 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidAssetsAmount);
        }
        Vault::deposit_internal(e, &receiver, assets, shares, &from, &operator);
        emit_deposit(e, &operator, &from, &receiver, assets, shares);

        shares
    }

    fn mint(e: &Env, shares: i128, receiver: Address, from: Address, operator: Address) -> i128 {
        Self::bump_instance(e);
        storage::require_not_paused(e);
        operator.require_auth();

        // ERC-4626 interface symmetry (see deposit comment above).
        if shares > Self::max_mint(e, receiver.clone()) {
            panic_with_error!(e, VaultTokenError::VaultExceededMaxMint);
        }

        let assets = Self::preview_mint(e, shares);
        if shares > 0 && assets == 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidSharesAmount);
        }
        Vault::deposit_internal(e, &receiver, assets, shares, &from, &operator);
        emit_deposit(e, &operator, &from, &receiver, assets, shares);

        assets
    }

    fn withdraw(
        e: &Env,
        assets: i128,
        receiver: Address,
        owner: Address,
        operator: Address,
    ) -> i128 {
        Self::bump_instance(e);
        storage::require_not_paused(e);
        operator.require_auth();

        // Fetch local_balance once and thread total_assets through to avoid
        // redundant cross-contract calls.
        let local_balance = Self::local_balance(e);
        let total_assets = Self::total_assets_from(e, local_balance);

        let entitled = Self::convert_to_assets_with_total(
            e,
            Base::balance(e, &owner),
            total_assets,
            Rounding::Floor,
        );
        let max_assets = core::cmp::min(entitled, local_balance);
        if assets > max_assets {
            panic_with_error!(e, VaultTokenError::VaultExceededMaxWithdraw);
        }

        let shares = Self::convert_to_shares_with_total(e, assets, total_assets, Rounding::Ceil);
        if assets > 0 && shares == 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidAssetsAmount);
        }
        Vault::withdraw_internal(e, &receiver, &owner, assets, shares, &operator);
        emit_withdraw(e, &operator, &receiver, &owner, assets, shares);

        shares
    }

    fn redeem(e: &Env, shares: i128, receiver: Address, owner: Address, operator: Address) -> i128 {
        Self::bump_instance(e);
        storage::require_not_paused(e);
        operator.require_auth();

        // Fetch local_balance once and thread total_assets through to avoid
        // redundant cross-contract calls.
        let local_balance = Self::local_balance(e);
        let total_assets = Self::total_assets_from(e, local_balance);

        let user_shares = Base::balance(e, &owner);
        let shares_for_available =
            Self::convert_to_shares_with_total(e, local_balance, total_assets, Rounding::Floor);
        let max_shares = core::cmp::min(user_shares, shares_for_available);
        if shares > max_shares {
            panic_with_error!(e, VaultTokenError::VaultExceededMaxRedeem);
        }

        let assets = Self::convert_to_assets_with_total(e, shares, total_assets, Rounding::Floor);
        if shares > 0 && assets == 0 {
            panic_with_error!(e, VaultTokenError::VaultInvalidSharesAmount);
        }
        Vault::withdraw_internal(e, &receiver, &owner, assets, shares, &operator);
        emit_withdraw(e, &operator, &receiver, &owner, assets, shares);

        assets
    }
}
