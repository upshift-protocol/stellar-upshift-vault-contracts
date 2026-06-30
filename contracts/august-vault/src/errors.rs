use soroban_sdk::contracterror;

// VaultError discriminants: 1-99 (this contract's domain errors).
// VaultTokenError (upstream OZ vault library): 400+ range.
// Keep these ranges disjoint when adding new variants.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum VaultError {
    Unauthorized = 1,
    VaultPaused = 2,
    InvalidAmount = 3,
    SubaccountNotWhitelisted = 4,
    SubaccountAlreadyRegistered = 5,
    // 6: reserved (formerly SubaccountHasFunds, removed in AUGUST-4432)
    AumChangeExceedsLimit = 7,
    MathOverflow = 8,
    MaxSubaccountsReached = 9,
    InvalidAumLimits = 10,
    SubaccountReturnedNothing = 11,
    DeployedAssetsUnderflow = 12,
    // 13: reserved (formerly InsufficientAllowance, removed in M-1 fix —
    //     allowance is now enforced atomically by the token's transfer_from)
    NoPendingAdmin = 14,
    InvalidAdminProposal = 15,
    AdminProposalExpired = 16,
    /// Strategy's configured asset does not match the vault's asset.
    AssetMismatch = 17,
    /// decimals_offset must be >= 3 to mitigate first-depositor inflation attacks.
    InvalidDecimalsOffset = 18,
    /// A strategy's `get_balance()` returned a negative value.
    NegativeStrategyBalance = 19,
    /// Cumulative AUM change within the current time window exceeds the limit.
    AumCumulativeChangeExceedsLimit = 20,
    /// A strategy's `get_balance()` call failed (panicked, returned a non-i128,
    /// or otherwise could not be invoked). The vault freezes total-assets-dependent
    /// operations until the admin removes the failing subaccount via
    /// `remove_subaccount` (which does not make cross-contract calls).
    StrategyUnreachable = 21,
    /// An operation specific to `Wallet` subaccounts was invoked against a
    /// `Strategy` subaccount (or vice versa). Distinct from `InvalidAmount`
    /// so monitoring can tell a type-mismatch apart from a bad numeric input.
    InvalidSubaccountType = 22,
    /// `withdraw_from_subaccount` attempted to pull more from a Wallet than
    /// the per-wallet tracker records. Forces the operator to first reconcile
    /// the wallet's attributed value via `update_wallet_deployed` (dust /
    /// gain recognition) — otherwise an over-pull would silently consume
    /// another wallet's share of `deployed_assets`, corrupting NAV.
    WalletOverWithdraw = 23,
    /// `remove_subaccount` was called on a subaccount whose
    /// `WalletNetDeployed` tracker is non-zero. Primary case (Wallet):
    /// removing without reconciliation would either strand value in
    /// `deployed_assets` (if removed first) or open a pricing window
    /// between write-down and removal (if reconciled separately).
    /// Operators must either drain the wallet to zero via
    /// `withdraw_from_subaccount` or use `remove_wallet_and_reconcile`,
    /// which closes the window atomically under dual auth.
    ///
    /// **Defense-in-depth case (Strategy)**: the post-R2 guard is
    /// state-based, not type-based — it trips on any subaccount with a
    /// non-zero tracker regardless of `SubaccountType`. A Strategy
    /// subaccount never legitimately has a non-zero
    /// `WalletNetDeployed`, so hitting this error on a Strategy
    /// indicates storage corruption (stray tracker entry, wrong type
    /// stored). The fix in that case is to investigate the
    /// corruption, not to drain-then-remove.
    WalletTrackerNotZero = 24,
    /// `seed_wallet_net_deployed` was called with `amount < current_tracker`.
    /// Seeding is an admin-only path that does not touch the aggregate
    /// `deployed_assets`; allowing downward seeds would let an admin zero
    /// a tracker to bypass `WalletTrackerNotZero` (#24) and then remove
    /// the wallet, stranding value in the aggregate. Downward reconciliation
    /// must go through `update_wallet_deployed` or
    /// `remove_wallet_and_reconcile` (both move the aggregate in lockstep).
    WalletSeedBelowTracker = 25,
    /// `update_wallet_deployed_batch` was called with an empty `updates`
    /// vector. Rejected for explicitness — a no-op batch call is almost
    /// always a client-side bug (e.g. filtered list collapsed to empty)
    /// that would otherwise silently succeed.
    EmptyBatch = 26,
    /// `update_wallet_deployed_batch` was called with the same subaccount
    /// address appearing more than once in `updates`. Rejected because
    /// the net-delta computation would otherwise depend on iteration
    /// order (later pair overwrites earlier tracker value but the delta
    /// applied to the aggregate sums all pairs), breaking the atomic
    /// lockstep between per-wallet tracker and aggregate.
    DuplicateSubaccountInBatch = 27,
}

// Compile-time: ensure all discriminants stay in the 1-99 range,
// disjoint from VaultTokenError (400+).
const _: () = assert!(VaultError::Unauthorized as u32 >= 1);
const _: () = assert!(VaultError::DuplicateSubaccountInBatch as u32 <= 99);
