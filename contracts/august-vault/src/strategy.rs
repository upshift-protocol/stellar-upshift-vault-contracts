use soroban_sdk::{contractclient, Address, Env};

/// Interface that strategy subaccounts must implement.
///
/// # `deposit`
///
/// Called by the vault's `deposit_to_subaccount` **after** the underlying
/// tokens have already been transferred to the strategy address via
/// `token::Client::transfer`. This is a notification: the tokens are
/// already in the strategy's balance when this is invoked.
///
/// - `from` — the address that sent the tokens (always the vault).
/// - `amount` — the actual amount transferred (measured via balance-
///   differencing on the vault side).
///
/// ## Authorization (MUST)
///
/// Strategies MUST authenticate this call, since anyone can invoke contract
/// functions directly. Recommended pattern:
/// - call `from.require_auth()`
/// - verify `from` equals a trusted vault address stored during strategy
///   initialization, to ensure only the vault can trigger deposits.
///
/// Strategies that simply hold the underlying token can implement this as
/// a no-op. Strategies that need to react to incoming capital (e.g.
/// auto-stake into a lending protocol) should perform that logic here.
///
/// A zero-amount call must be a no-op (used for interface validation
/// during subaccount registration).
///
/// # `withdraw`
///
/// Transfer up to `amount` of the vault's underlying asset to `to`.
/// Return the actual amount transferred (the vault measures receipt via
/// balance-differencing, so this value is not used for accounting;
/// should be >= 0). A zero-amount call must be a no-op (used for
/// interface validation during subaccount registration).
///
/// ## Authorization (MUST)
///
/// Strategies MUST restrict withdrawals to calls from the vault. The
/// recommended pattern is to first verify `to` equals the stored vault
/// address (cheap address comparison), then call `to.require_auth()`
/// (the vault always calls `withdraw(vault, amount)`). Verifying first
/// avoids paying the auth cost on calls that would revert anyway.
///
/// Note: the vault ignores this return value for accounting purposes and
/// measures actual token balance changes instead (balance-differencing).
#[contractclient(name = "StrategyClient")]
pub trait IStrategy {
    fn deposit(e: Env, from: Address, amount: i128);
    fn withdraw(e: Env, to: Address, amount: i128) -> i128;

    /// Return the strategy's current balance of the underlying token.
    ///
    /// The return value MUST be >= 0; the vault panics with
    /// `NegativeStrategyBalance` if a negative value is returned (both at
    /// registration time in `add_subaccount` and at query time in
    /// `query_strategy_balances`).
    ///
    /// This MUST include both idle tokens held directly by the contract
    /// AND a bookkeeping estimate of capital deployed to external protocols
    /// (e.g. `idle_balance + deployed_total`). The vault uses this to
    /// compute `total_assets` without relying solely on operator-reported
    /// values.
    ///
    /// **Bookkeeping hazard**: when a protocol returns funds via direct
    /// transfer (rather than a controller-initiated pull), the strategy's
    /// `idle_balance` rises while `deployed_total` is unchanged, causing
    /// `get_balance()` to over-report and inflating the vault's
    /// `total_assets()`. Strategies that track `deployed_total` MUST
    /// expose a controller-only reconciliation function (e.g.
    /// `settle_protocol_returns`) so the controller can write
    /// `deployed_total` back down after observing direct repayments
    /// off-chain.
    ///
    /// ```text
    /// total_assets = vault_local_balance
    ///              + Σ strategy.get_balance()   // on-chain, per strategy
    ///              + deployed_assets             // wallet-attributed AUM
    ///                                            // (= Σ WalletNetDeployed)
    /// ```
    ///
    /// **F4 (wallet-only commitment)**: strategies are trusted to report
    /// their complete position (idle + deployed) via this function. The
    /// vault's `deployed_assets` aggregate is therefore exclusively
    /// wallet-attributed — it is never used to carry a strategy's off-chain
    /// AUM on the strategy's behalf. A strategy that omits its deployed
    /// component from `get_balance()` would silently under-report NAV; the
    /// whitelist review (admin approval during `add_subaccount`) is the
    /// enforcement boundary for this behavioural contract.
    fn get_balance(e: Env) -> i128;

    /// Return the token address this strategy manages.
    ///
    /// Used by `add_subaccount` to verify the strategy's configured asset
    /// matches the vault's asset, preventing silent NAV corruption from
    /// mismatched token denominations.
    fn get_asset(e: Env) -> Address;

    /// Return the vault-originated idle balance: tokens received from the
    /// vault via `deposit` that have not yet been deployed to an external
    /// protocol or returned to the vault via `withdraw`.
    ///
    /// Introduced in F1/F2 to make explicit the accounting variable that
    /// isolates vault-controlled flows from untracked external transfers
    /// (direct SEP-41 donations, protocol pushes, airdrops).
    ///
    /// Contract for implementors:
    /// - MUST be incremented by the amount received in `deposit`.
    /// - MUST be decremented by the actual amount sent in `withdraw`.
    /// - MUST NOT include tokens received outside the vault-controlled
    ///   flow (direct transfers, protocol pushes, airdrops, etc.).
    /// - `get_balance()` MUST return `get_local_balance() + <off-chain
    ///   estimate>`; `token.balance(self)` MUST NOT be used as the idle
    ///   component of the returned value. Reading `token.balance(self)`
    ///   for a donation-detection side-check is fine (and expected).
    /// - MUST be >= 0.
    ///
    /// The interface cannot enforce these semantics on-chain — the vault
    /// trusts whitelisted strategies to comply. The whitelist approval is
    /// the enforcement boundary: verify implementation correctness before
    /// registering a strategy.
    fn get_local_balance(e: Env) -> i128;
}
