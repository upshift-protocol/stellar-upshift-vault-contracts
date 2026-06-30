//! XLM wrapper strategy contract with vault and controller access controls.
//!
//! Provides the `deposit`/`withdraw` interface expected by the August vault's
//! `IStrategy` trait (defined in `august-vault`), and adds controller-gated
//! functions for deploying/recalling funds to/from external protocol addresses.
//!
//! ## Roles
//!
//! - **Vault** — the only address that may call `deposit` / `withdraw`.
//! - **Controller** (e.g. a Utila EOA) — the only address that may call
//!   `deploy_to_protocol` / `recall_from_protocol`.
//!
//! ## Limitations
//!
//! `recall_from_protocol` performs a simple SEP-41 `transfer(protocol →
//! strategy)`.  The `protocol` address must authorize this transfer via
//! Soroban auth (i.e. `protocol`'s signing key must approve the
//! sub-invocation).  For protocols that expose a custom withdrawal
//! interface (lending pools, AMMs, etc.) you must replace the transfer call
//! with the protocol's client invocation.
#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error, token,
    Address, Env,
};

#[cfg(test)]
mod test;

// ── Errors ──────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum StrategyError {
    /// Caller is not the vault.
    NotVault = 1,
    /// Caller is not the controller.
    NotController = 2,
    /// Amount must be positive.
    InvalidAmount = 3,
    /// Insufficient balance for the requested operation.
    InsufficientBalance = 4,
    /// Target address is invalid for this operation (e.g. self or vault).
    InvalidTarget = 5,
    /// New controller address is invalid (e.g. vault or this contract).
    InvalidController = 6,
    /// Arithmetic overflow.
    MathOverflow = 7,
    /// Balance decreased after a transfer (possible reentrancy or malicious token).
    BalanceDecreasedOnTransfer = 8,
    /// `settle_protocol_returns` attempted to change `deployed_total` by
    /// more than the hardcoded per-call limit (see `SETTLE_INCREASE_BPS`
    /// / `SETTLE_DECREASE_BPS`). Bounds the blast radius of a compromised
    /// controller attempting NAV manipulation via rapid deployed-total
    /// rewrites. The controller must split large reconciliations across
    /// multiple calls.
    ///
    /// **Operational footgun**: once the bootstrap latch is set, a
    /// `deployed_total` that was brought to zero via `recall_from_protocol`
    /// cannot be re-raised through settle alone (`previous == 0` plus
    /// non-zero new_total fails because `max_change = 0`). Recovery:
    /// execute a minimal `deploy_to_protocol` to re-seed a non-zero
    /// baseline, then ramp up via rate-limited settles. This is by
    /// design (settle should not be a NAV-rewrite escape hatch), but
    /// operators should know the recovery sequence.
    SettleRateLimitExceeded = 9,
    /// `recover_donation` was called but `token.balance(self)` does not
    /// exceed the tracked `local_balance` — there is nothing to recover.
    NoDonationToRecover = 10,
    /// `seed_local_balance` was called on a strategy whose `local_balance`
    /// has already been initialised. The seed path is single-use (intended
    /// for a one-time post-upgrade migration from pre-tracker balances);
    /// further corrections must go through the normal deposit/withdraw/
    /// settle flows.
    LocalBalanceAlreadySeeded = 11,
    /// `seed_local_balance` was called with `amount > token.balance(self)`,
    /// which would produce a `tracked > actual_idle` state — silent NAV
    /// overstatement. Seed amounts must not exceed the strategy's actual
    /// on-chain balance at seed time.
    SeedExceedsActualBalance = 12,
    /// `seed_deployed_total` was called after the bootstrap latch already
    /// tripped. The latch is set by any of (a) `seed_deployed_total`
    /// itself, (b) a successful `deploy_to_protocol`, or (c) the first
    /// `settle_protocol_returns` that writes a positive value. "Already
    /// initialized" covers all three sources — once the strategy has
    /// produced a non-zero `deployed_total` through any legitimate path,
    /// further explicit seeding is blocked. After the latch,
    /// `settle_protocol_returns` is the only writer and is always
    /// rate-limited, preventing a compromised controller from cycling
    /// `deploy`→`recall`→`settle-to-huge` via the former `previous == 0`
    /// exemption.
    DeployedTotalAlreadyInitialized = 13,
    /// `deposit` completed but `token.balance(self) < local_balance` after
    /// the tracker was incremented. Indicates the underlying token moved
    /// fewer tokens than declared (fee-on-transfer, non-standard SEP-41,
    /// or an external clawback landing mid-tx). Strategy cannot
    /// reconcile without a governance-level decision, so the deposit is
    /// rolled back.
    ///
    /// **Operational recovery**: once a strategy enters a
    /// `token.balance < local_balance` state (via external clawback or
    /// rebase — the `deposit`-time check catches the fee-on-transfer
    /// case before any state is committed), every subsequent
    /// `deposit_to_subaccount` from the vault will revert with this
    /// error. Recovery requires vault-side intervention: either
    /// `remove_wallet_and_reconcile` or `remove_subaccount` to detach
    /// the strategy, followed by re-registration once the underlying
    /// token is healthy. There is no strategy-side path to clear the
    /// shortfall on-chain — that would let a hostile controller rewrite
    /// `local_balance` directly.
    DepositShortfall = 14,
}

// ── Storage keys ────────────────────────────────────────────────────────

// APPEND-ONLY: Soroban encodes `contracttype` enum variants by declaration
// order. Inserting or reordering variants silently re-indexes existing
// persisted keys — catastrophic on upgrade. Only add new variants at the
// end.
#[contracttype]
enum StorageKey {
    Asset,         // Address
    Vault,         // Address
    Controller,    // Address
    DeployedTotal, // i128
    /// i128 — tokens received from the vault via `deposit` that have not
    /// yet been deployed to an external protocol or returned to the vault
    /// via `withdraw`. Moved in lockstep with `DeployedTotal` as funds
    /// transit through the strategy. Deliberately distinct from the raw
    /// on-chain token balance so that untracked inflows (donations,
    /// protocol pushes, airdrops) cannot inflate the vault's NAV through
    /// `get_balance()`.
    LocalBalance,
    /// bool — once set, `seed_deployed_total` is rejected and
    /// `settle_protocol_returns` loses its `previous == 0` bootstrap
    /// exemption. Latched by `seed_deployed_total` itself and by any
    /// successful `deploy_to_protocol` that makes `DeployedTotal` > 0.
    DeployedTotalBootstrapped,
}

// ── TTL constants ───────────────────────────────────────────────────────

const DAY_IN_LEDGERS: u32 = 17_280;
const INSTANCE_EXTEND_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_TTL_THRESHOLD: u32 = INSTANCE_EXTEND_AMOUNT - DAY_IN_LEDGERS;

// ── Settle rate-limit constants (F3) ────────────────────────────────────
//
// Mirror the vault's AUM-limit pattern applied to `deployed_total`: bound
// the per-call change in `settle_protocol_returns` so a compromised
// controller cannot rewrite the strategy's reported off-chain position in
// a single transaction. Hardcoded rather than configurable: the strategy
// has no governance role distinct from the controller, so making them
// controller-adjustable would defeat the defense. Matches the vault's
// `DEFAULT_AUM_INCREASE_LIMIT` / `DEFAULT_AUM_DECREASE_LIMIT`.

const SETTLE_INCREASE_BPS: i128 = 1_000; // 10% — accommodates accrued interest
const SETTLE_DECREASE_BPS: i128 = 500; // 5% — tighter, matches vault loss-recognition pace
const BPS_DIVISOR: i128 = 10_000;

// ── Events ──────────────────────────────────────────────────────────────

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedToProtocol {
    #[topic]
    pub controller: Address,
    #[topic]
    pub protocol: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecalledFromProtocol {
    #[topic]
    pub controller: Address,
    #[topic]
    pub protocol: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerChanged {
    #[topic]
    pub old_controller: Address,
    pub new_controller: Address,
}

/// Emitted when `recall_from_protocol` receives more than `deployed_total`
/// tracks (e.g. protocol yield). The counter is clamped to 0; this event
/// records the discrepancy for off-chain monitoring.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedTotalUnderflow {
    pub tracked: i128,
    pub actual_recall: i128,
}

/// Emitted when the controller reconciles `deployed_total` after funds
/// returned from a protocol outside of `recall_from_protocol` (e.g. push
/// payments, rewards drops, periodic interest accruals). Records the
/// adjustment so off-chain accounting can audit drift.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedTotalSettled {
    #[topic]
    pub controller: Address,
    pub previous: i128,
    pub new_total: i128,
}

/// Emitted by `get_balance` whenever `token.balance(self) > local_balance`
/// — the strategy holds more of the underlying asset than its vault-tracked
/// idle balance records. Possible causes: a direct SEP-41 donation, a
/// protocol push-payment that bypassed `recall_from_protocol`, an airdrop.
/// The event continues to fire on every read until the controller calls
/// `recover_donation` to remove the excess.
///
/// **No explicit strategy topic**: Soroban's event envelope already
/// carries the emitting contract's address, so indexers filtering by
/// strategy instance use the envelope's `contract_id` field — adding
/// `strategy` as a topic would duplicate that information and consume
/// a topic slot (Soroban caps at 4 per event).
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonationDetected {
    pub excess: i128,
    pub tracked: i128,
    pub actual: i128,
}

/// Emitted when the controller transfers excess tokens (tokens beyond
/// `local_balance`) out of the strategy via `recover_donation`. The
/// recipient is admin-chosen — typically a treasury or recovery wallet.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonationRecovered {
    #[topic]
    pub controller: Address,
    #[topic]
    pub recipient: Address,
    pub amount: i128,
}

/// Emitted when the controller seeds `local_balance` to a non-zero value
/// via `seed_local_balance`. Intended as a one-time post-upgrade migration
/// step: strategies deployed before the tracker existed will have
/// `local_balance == 0` but `token.balance(self) > 0`; seeding records the
/// known vault-originated portion so `get_balance()` stops flagging the
/// legitimate pre-upgrade balance as a donation.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBalanceSeeded {
    #[topic]
    pub controller: Address,
    pub amount: i128,
}

/// Emitted when the controller seeds `deployed_total` via
/// `seed_deployed_total`. One-shot, mirrors `LocalBalanceSeeded`. After
/// this event fires, all `settle_protocol_returns` calls are
/// rate-limited (including those with `previous == 0`), closing the
/// deploy → recall → settle-to-huge bypass.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployedTotalSeeded {
    #[topic]
    pub controller: Address,
    pub amount: i128,
}

/// Emitted by `get_balance` on the `actual_idle < tracked` branch — the
/// strategy's on-chain balance is below what the tracker records. Unlike
/// `DonationDetected`, this represents a **loss** relative to the vault's
/// expected idle: fee-on-transfer behaviour, rebasing downward, admin
/// clawback on the token, or a bug in the tracker accounting.
///
/// Post-R2-2, `get_balance` also returns the conservative `actual_idle`
/// (not `tracked`) while this event is firing, so the vault's NAV
/// under-reports rather than over-reports the degraded state. The
/// strategy does not panic because that would freeze every share/asset
/// conversion in the vault (via `query_strategy_balances`) until the
/// admin removed the subaccount; emitting the event plus returning the
/// conservative value keeps the degraded state observable without
/// bricking the vault.
///
/// **No explicit strategy topic**: same rationale as `DonationDetected`
/// — the event envelope's `contract_id` already identifies the emitting
/// strategy.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackerExceedsBalance {
    pub tracked: i128,
    pub actual: i128,
    pub shortfall: i128,
}

// ── Contract ────────────────────────────────────────────────────────────

#[contract]
pub struct XlmStrategy;

#[contractimpl]
impl XlmStrategy {
    // ── Constructor ─────────────────────────────────────────────────────

    /// Initialize the strategy.
    ///
    /// * `asset`      – the token this strategy manages (e.g. XLM wrapper).
    /// * `vault`      – the August vault address; only address that may
    ///                   call `deposit` / `withdraw`.
    /// * `controller` – the EOA (e.g. Utila wallet) that may call
    ///                   `deploy_to_protocol` / `recall_from_protocol`.
    pub fn __constructor(e: &Env, asset: Address, vault: Address, controller: Address) {
        Self::bump_instance(e);
        let self_addr = e.current_contract_address();
        if vault == controller || vault == self_addr || controller == self_addr {
            panic_with_error!(e, StrategyError::InvalidController);
        }

        e.storage().instance().set(&StorageKey::Asset, &asset);
        e.storage().instance().set(&StorageKey::Vault, &vault);
        e.storage()
            .instance()
            .set(&StorageKey::Controller, &controller);
        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotal, &0i128);
    }

    // ── IStrategy interface (vault-only) ────────────────────────────────

    /// Notification called by the vault after it has already transferred
    /// tokens in. Validates the caller is the vault and, under F1, also
    /// updates the internal `local_balance` tracker so `get_balance()` can
    /// distinguish vault-originated idle from tokens that arrived outside
    /// the vault-controlled flow (donations, protocol pushes, airdrops).
    ///
    /// Zero-amount calls validate the vault relationship but skip auth,
    /// token logic, and the tracker update (used for interface validation
    /// during `add_subaccount` registration). Negative amounts are
    /// rejected with `InvalidAmount`.
    pub fn deposit(e: &Env, from: Address, amount: i128) {
        Self::bump_instance(e);
        Self::require_vault(e, &from);
        if amount == 0 {
            return;
        }
        if amount < 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }
        from.require_auth();

        let new_local = Self::get_local_balance(e)
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        e.storage()
            .instance()
            .set(&StorageKey::LocalBalance, &new_local);

        // I-2: verify the token actually moved at least `amount` in by
        // comparing the post-transfer balance to the new tracker. Guards
        // against fee-on-transfer tokens or non-standard SEP-41
        // implementations that would silently produce `tracked > actual`
        // — the exact state `DonationDetected` cannot flag (it only
        // fires on the opposite side). Rolls back the deposit if
        // reconciliation is impossible.
        let actual_idle = Self::token_client(e).balance(&e.current_contract_address());
        if actual_idle < new_local {
            panic_with_error!(e, StrategyError::DepositShortfall);
        }
    }

    /// Transfer up to `amount` of the asset back to the vault.
    /// Returns the actual amount transferred (capped by `local_balance`
    /// under F1 — not the raw token balance, so donated/untracked tokens
    /// stay in the strategy until `recover_donation` is called explicitly).
    /// Returns 0 for non-positive amounts (vault relationship is still
    /// validated for interface verification during registration).
    pub fn withdraw(e: &Env, to: Address, amount: i128) -> i128 {
        Self::bump_instance(e);
        Self::require_vault(e, &to);
        if amount <= 0 {
            return 0;
        }
        to.require_auth();

        let token_client = Self::token_client(e);
        let self_addr = e.current_contract_address();
        let tracked = Self::get_local_balance(e);
        let actual = core::cmp::min(amount, tracked);
        if actual > 0 {
            token_client.transfer(&self_addr, &to, &actual);
            let new_local = tracked
                .checked_sub(actual)
                .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
            e.storage()
                .instance()
                .set(&StorageKey::LocalBalance, &new_local);
        }
        actual
    }

    // ── Controller-only management ──────────────────────────────────────

    /// Move funds from this strategy into an external protocol address.
    /// Only the controller (e.g. Utila wallet) may call this.
    ///
    /// Caps on `local_balance` (not raw `token.balance`) under F1: donated
    /// or protocol-pushed tokens sit outside the tracker and must not be
    /// re-deployed through this path. Use `recover_donation` first if
    /// there are legitimately-untracked funds to reclassify.
    ///
    /// **Side effect (R2-4)**: the first successful call trips the
    /// `DeployedTotalBootstrapped` latch, permanently disabling
    /// `seed_deployed_total` and subjecting all subsequent
    /// `settle_protocol_returns` calls to rate-limiting even when
    /// `deployed_total` has been recalled back to zero. This is the
    /// core C-1 defense; callers should treat the first deploy as a
    /// one-way commitment on the reconciliation path.
    pub fn deploy_to_protocol(e: &Env, controller: Address, protocol: Address, amount: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);
        Self::require_valid_protocol_target(e, &protocol);

        if amount <= 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }

        let tracked = Self::get_local_balance(e);
        if tracked < amount {
            panic_with_error!(e, StrategyError::InsufficientBalance);
        }

        let token_client = Self::token_client(e);
        let self_addr = e.current_contract_address();
        token_client.transfer(&self_addr, &protocol, &amount);

        let new_local = tracked
            .checked_sub(amount)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        e.storage()
            .instance()
            .set(&StorageKey::LocalBalance, &new_local);

        let deployed = Self::get_deployed_total(e);
        let new_deployed = deployed
            .checked_add(amount)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotal, &new_deployed);

        // Trip the bootstrap latch: once any real capital has moved out
        // of the strategy into a protocol, `settle_protocol_returns` can
        // no longer use the `previous == 0` exemption as a bypass path
        // (see `check_settle_rate_limit`).
        if !Self::is_deployed_total_bootstrapped(e) {
            Self::set_deployed_total_bootstrapped(e);
        }

        DeployedToProtocol {
            controller,
            protocol,
            amount,
        }
        .publish(e);
    }

    /// Pull funds back from an external protocol into this strategy.
    /// Only the controller (e.g. Utila wallet) may call this.
    ///
    /// This performs a simple SEP-41 token transfer from `protocol` to this
    /// contract. The `protocol` address must authorize this transfer via
    /// Soroban auth. For protocols requiring a custom withdrawal call,
    /// replace the transfer with the protocol's client interface.
    ///
    /// Uses balance-differencing (balance before/after the transfer) to
    /// determine the actual tokens received, which may differ from `amount`
    /// for fee-on-transfer tokens.
    ///
    /// Note: `deployed_total` is a bookkeeping estimate that saturates at
    /// zero. If the recalled amount exceeds `deployed_total` (e.g. due to
    /// protocol yield), the counter floors at zero rather than going negative.
    pub fn recall_from_protocol(e: &Env, controller: Address, protocol: Address, amount: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);
        Self::require_valid_protocol_target(e, &protocol);

        if amount <= 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }

        let token_client = Self::token_client(e);
        let self_addr = e.current_contract_address();

        let balance_before = token_client.balance(&self_addr);
        token_client.transfer(&protocol, &self_addr, &amount);
        let balance_after = token_client.balance(&self_addr);

        let actual_received = balance_after
            .checked_sub(balance_before)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        if actual_received < 0 {
            panic_with_error!(e, StrategyError::BalanceDecreasedOnTransfer);
        }
        if actual_received == 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }

        let deployed = Self::get_deployed_total(e);
        if actual_received > deployed {
            DeployedTotalUnderflow {
                tracked: deployed,
                actual_recall: actual_received,
            }
            .publish(e);
        }
        let new_deployed = deployed.saturating_sub(actual_received).max(0);
        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotal, &new_deployed);

        // F1: recalled funds are now vault-tracked idle — add the full
        // actual_received to local_balance (which covers both the recalled
        // principal and any yield the protocol returned on top).
        let new_local = Self::get_local_balance(e)
            .checked_add(actual_received)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        e.storage()
            .instance()
            .set(&StorageKey::LocalBalance, &new_local);

        RecalledFromProtocol {
            controller,
            protocol,
            amount: actual_received,
        }
        .publish(e);
    }

    /// Reconcile `deployed_total` after funds have returned to the strategy
    /// outside of `recall_from_protocol` (e.g. a protocol pushed yield or
    /// principal back via direct SEP-41 transfer). Only the controller may
    /// call this.
    ///
    /// Why this exists: `get_balance()` reports `idle + deployed_total`. The
    /// strategy cannot observe direct transfers to itself, so when funds
    /// arrive outside the recall flow, `idle` increases while
    /// `deployed_total` is unchanged — over-reporting the strategy's true
    /// position and inflating the vault's `total_assets()`. The controller
    /// must call this function to write `deployed_total` back down to its
    /// true value.
    ///
    /// `new_total` is the new absolute value of `deployed_total` (not a
    /// delta). The controller is expected to compute this off-chain by
    /// reading the protocol's actual outstanding balance.
    ///
    /// Negative values are rejected. Setting `new_total` larger than the
    /// current value is allowed (e.g. to reflect newly accrued interest
    /// that the protocol has not yet paid out).
    ///
    /// **Rate limited (F3)**: the per-call change is bounded to
    /// `SETTLE_INCREASE_BPS` / `SETTLE_DECREASE_BPS` of the current
    /// `deployed_total`. A single transaction cannot rewrite the entire
    /// position — legitimate large reconciliations must be applied over
    /// multiple calls.
    ///
    /// The first settle on a pristine strategy (`previous == 0` and the
    /// `DeployedTotalBootstrapped` latch unset) is unbounded by design
    /// because there is no baseline to measure against. That call **trips
    /// the latch as a side effect** (R2-1), so every subsequent settle —
    /// including settles from zero after `recall_from_protocol` drains
    /// the position — is rate-limited. Combined with the bootstrap
    /// exemption in `check_settle_rate_limit`, this closes the cyclic
    /// bypass (deploy → recall → settle-to-huge) AND the first-call-on-
    /// pristine bypass that the initial C-1 fix left open.
    pub fn settle_protocol_returns(e: &Env, controller: Address, new_total: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);

        if new_total < 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }

        let previous = Self::get_deployed_total(e);
        Self::check_settle_rate_limit(e, previous, new_total);

        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotal, &new_total);

        // R2-1: trip the bootstrap latch on the first settle that writes
        // a non-zero value to `deployed_total`. Without this, a
        // controller-compromised-at-day-0 could call
        // `settle_protocol_returns(HUGE)` on a pristine strategy
        // (`previous == 0`, latch unset) and inflate NAV unbounded.
        // After this write the latch is set, so any future
        // `previous == 0` settle falls through to rate-limit with
        // `max_change = 0` and positive deltas are rejected.
        if new_total > 0 && !Self::is_deployed_total_bootstrapped(e) {
            Self::set_deployed_total_bootstrapped(e);
        }

        DeployedTotalSettled {
            controller,
            previous,
            new_total,
        }
        .publish(e);
    }

    // ── Admin ───────────────────────────────────────────────────────────

    /// Rotate the controller address. Only the current controller may call.
    pub fn set_controller(e: &Env, current: Address, new_controller: Address) {
        Self::bump_instance(e);
        current.require_auth();
        Self::require_controller(e, &current);

        let vault: Address = e.storage().instance().get(&StorageKey::Vault).unwrap();
        if new_controller == vault || new_controller == e.current_contract_address() {
            panic_with_error!(e, StrategyError::InvalidController);
        }

        e.storage()
            .instance()
            .set(&StorageKey::Controller, &new_controller);

        ControllerChanged {
            old_controller: current,
            new_controller,
        }
        .publish(e);
    }

    /// Permissionless TTL extension.
    pub fn extend_ttl(e: &Env) {
        Self::bump_instance(e);
    }

    // ── Views ───────────────────────────────────────────────────────────

    /// The vault address that may call `deposit` / `withdraw`.
    pub fn get_vault(e: &Env) -> Address {
        e.storage().instance().get(&StorageKey::Vault).unwrap()
    }

    /// The controller address that may call `deploy_to_protocol` / `recall_from_protocol`.
    pub fn get_controller(e: &Env) -> Address {
        e.storage().instance().get(&StorageKey::Controller).unwrap()
    }

    /// The token this strategy manages.
    pub fn get_asset(e: &Env) -> Address {
        Self::bump_instance(e);
        e.storage().instance().get(&StorageKey::Asset).unwrap()
    }

    /// Total strategy balance: vault-tracked idle + bookkeeping estimate of
    /// capital deployed to external protocols. Under F1 this is
    /// `local_balance + deployed_total` rather than
    /// `token.balance(self) + deployed_total`, so direct donations and
    /// protocol pushes cannot inflate the value the vault consumes through
    /// `total_assets()`.
    ///
    /// Emits `DonationDetected` on every read where the raw token balance
    /// exceeds the tracker, continuously alerting monitoring until the
    /// controller runs `recover_donation` to restore `token.balance ==
    /// local_balance`.
    pub fn get_balance(e: &Env) -> i128 {
        Self::bump_instance(e);
        let self_addr = e.current_contract_address();
        let actual_idle = Self::token_client(e).balance(&self_addr);
        let tracked = Self::get_local_balance(e);
        let deployed = Self::get_deployed_total(e);

        let idle_component = if actual_idle > tracked {
            let excess = actual_idle
                .checked_sub(tracked)
                .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
            DonationDetected {
                excess,
                tracked,
                actual: actual_idle,
            }
            .publish(e);
            // Conservative: return `tracked` (exclude donation from NAV).
            tracked
        } else if actual_idle < tracked {
            // R2-2 / I-3: under the current implementation this branch
            // is not reachable through the strategy's own flows —
            // `deposit` refuses to commit when the token delivers less
            // than declared (see `DepositShortfall`), and every other
            // tracker-mutating path moves in lockstep with an on-chain
            // transfer. It CAN arise from external token behaviour:
            // fee-on-transfer applied retroactively, rebasing downward,
            // or a token with an admin clawback / blacklist capability
            // acting on the strategy address after the fact.
            //
            // Return the conservative `actual_idle` here (not `tracked`)
            // so NAV under-reports the degraded state rather than
            // continuing to count vanished tokens. Panicking would
            // freeze every share/asset conversion via
            // `query_strategy_balances` in the vault; returning the
            // optimistic `tracked` would silently over-report NAV,
            // letting depositors mint too few shares and redeemers pull
            // too much. `min(actual_idle, tracked)` is strictly
            // conservative, mirrors the donation-side behaviour (return
            // `tracked`, not `actual_idle` when `actual > tracked`),
            // and keeps the degraded state observable via the emitted
            // event without bricking the vault.
            let shortfall = tracked
                .checked_sub(actual_idle)
                .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
            TrackerExceedsBalance {
                tracked,
                actual: actual_idle,
                shortfall,
            }
            .publish(e);
            actual_idle
        } else {
            tracked
        };

        idle_component
            .checked_add(deployed)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow))
    }

    /// F2: vault-originated idle balance — tokens received from the vault
    /// via `deposit` that have not yet been deployed externally or
    /// returned to the vault. Does not include donations, protocol
    /// push-payments, airdrops, or any other untracked external transfer.
    pub fn get_local_balance(e: &Env) -> i128 {
        e.storage()
            .instance()
            .get(&StorageKey::LocalBalance)
            .unwrap_or(0)
    }

    /// Bookkeeping estimate of funds currently deployed to external protocols.
    /// Saturates at zero; may not reflect yield accrued in protocols.
    /// Note: does not call `bump_instance` — only the vault calls
    /// `get_balance`/`get_asset` cross-contract, which do bump TTL.
    pub fn get_deployed_total(e: &Env) -> i128 {
        e.storage()
            .instance()
            .get(&StorageKey::DeployedTotal)
            .unwrap_or(0)
    }

    /// True once the strategy has produced a non-zero `deployed_total`
    /// via any legitimate path: `seed_deployed_total`, a successful
    /// `deploy_to_protocol`, or the first positive
    /// `settle_protocol_returns`. The name is deliberately higher-level
    /// than the internal latch ("bootstrapped") — future mechanism
    /// changes that alter how the gate is implemented can preserve
    /// `is_deployed_total_initialized()` as a stable predicate.
    ///
    /// Exposed publicly so off-chain monitoring and tests can assert
    /// the latch is a one-way gate. The same information is derivable
    /// from event history (`DeployedTotalSeeded`, `DeployedToProtocol`,
    /// first positive `DeployedTotalSettled`); this view is the
    /// synchronous shortcut.
    pub fn is_deployed_total_initialized(e: &Env) -> bool {
        Self::is_deployed_total_bootstrapped(e)
    }

    /// F1: transfer donated or untracked-external tokens out of the
    /// strategy. "Donated" is defined as `token.balance(self) -
    /// local_balance`: any positive excess over the vault-tracked idle
    /// balance, regardless of how it got there (direct SEP-41 transfer,
    /// protocol push, airdrop). Only the controller may call.
    ///
    /// `recipient` cannot be the strategy itself or the vault
    /// (`InvalidTarget`) — those addresses would confuse accounting or
    /// re-enter the tracker through the wrong path. `amount` must not
    /// exceed the computed excess.
    ///
    /// Does not mutate `local_balance`: the tracker already represented
    /// the legitimate vault-tracked portion and is unchanged by recovery.
    ///
    /// **Re-entrancy posture (R2-10 / round-3 refinement)**: the
    /// post-transfer balance-delta assertion is the load-bearing defense.
    /// Sequence:
    /// ```text
    /// 1. actual_idle = token.balance(self)              // read
    /// 2. token.transfer(self, recipient, amount)        // callback window
    /// 3. balance_after = token.balance(self)            // read
    /// 4. assert balance_after == actual_idle - amount
    /// ```
    /// **Any** re-entrant path that triggers an additional balance
    /// movement at `self` — direct (hostile token re-calls
    /// `recover_donation`, `withdraw`, `deposit`, etc.) or indirect
    /// (token calls the vault, vault calls back into the strategy) —
    /// nets a delta at step 3 that disagrees with `actual_idle - amount`,
    /// panicking with `BalanceDecreasedOnTransfer` and reverting the
    /// entire transaction (including the inner frames' state changes).
    /// The assertion is a `!=`, not `<`, so drift in either direction
    /// (including a hostile token crediting extra tokens to the strategy
    /// during its callback) is rejected.
    ///
    /// Auth checks on specific entry points (`require_vault` on
    /// `deposit`/`withdraw`, `require_controller` on settle/deploy/recall/
    /// recover) are **defense-in-depth** rather than the primary
    /// defense — they block direct hostile-token re-entry in the simple
    /// case, but vault-mediated indirect re-entry (token → vault →
    /// strategy) passes auth because the vault IS the authorized caller.
    /// The balance-delta check catches that path regardless.
    pub fn recover_donation(e: &Env, controller: Address, recipient: Address, amount: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);
        Self::require_valid_protocol_target(e, &recipient);

        if amount <= 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }

        let self_addr = e.current_contract_address();
        let actual_idle = Self::token_client(e).balance(&self_addr);
        let tracked = Self::get_local_balance(e);
        let excess = actual_idle
            .checked_sub(tracked)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));

        if excess <= 0 {
            panic_with_error!(e, StrategyError::NoDonationToRecover);
        }
        if amount > excess {
            panic_with_error!(e, StrategyError::InsufficientBalance);
        }

        Self::token_client(e).transfer(&self_addr, &recipient, &amount);

        // I-1: re-verify the balance delta after the external transfer.
        // Mirrors the `BalanceDecreasedOnTransfer` pattern used by
        // `recall_from_protocol`. Catches a hostile SEP-41 that re-enters
        // `recover_donation` during its transfer callback (drains more
        // than `amount`), or a token whose transfer fails-open. The
        // expected post-balance is exactly `actual_idle - amount`; any
        // other value indicates tampering or a non-standard token.
        let balance_after = Self::token_client(e).balance(&self_addr);
        let expected = actual_idle
            .checked_sub(amount)
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));
        if balance_after != expected {
            panic_with_error!(e, StrategyError::BalanceDecreasedOnTransfer);
        }

        DonationRecovered {
            controller,
            recipient,
            amount,
        }
        .publish(e);
    }

    /// One-time post-upgrade migration: initialize `local_balance` to a
    /// known pre-upgrade value. Only callable once (when the current
    /// tracker is zero) so a compromised controller cannot rewrite the
    /// idle portion of NAV through this path. Negative values are
    /// rejected.
    ///
    /// **C-3**: `amount` is also capped at the strategy's actual on-chain
    /// balance at seed time — preventing a typo or malicious seed from
    /// producing a silent `tracked > actual_idle` state (which would
    /// inflate NAV without emitting `DonationDetected`).
    ///
    /// After the initial seed, all tracker movement goes through the
    /// normal deposit/withdraw/deploy/recall flows.
    pub fn seed_local_balance(e: &Env, controller: Address, amount: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);

        if amount < 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }
        if Self::get_local_balance(e) != 0 {
            panic_with_error!(e, StrategyError::LocalBalanceAlreadySeeded);
        }

        let actual_balance = Self::token_client(e).balance(&e.current_contract_address());
        if amount > actual_balance {
            panic_with_error!(e, StrategyError::SeedExceedsActualBalance);
        }

        e.storage()
            .instance()
            .set(&StorageKey::LocalBalance, &amount);

        LocalBalanceSeeded { controller, amount }.publish(e);
    }

    /// One-time post-upgrade migration: initialize `deployed_total` to a
    /// known external-protocol position. Single-use — guarded by the
    /// `DeployedTotalBootstrapped` latch, which is also tripped by any
    /// successful `deploy_to_protocol`. Once the latch is set,
    /// `settle_protocol_returns` is the only writer to `deployed_total`
    /// and is always rate-limited — closing the
    /// deploy → recall → settle-to-huge bypass.
    ///
    /// Use cases: migrating from a pre-bootstrap deployment that already
    /// has capital at an external protocol, or recording an initial
    /// off-chain position before the first on-chain deploy.
    pub fn seed_deployed_total(e: &Env, controller: Address, amount: i128) {
        Self::bump_instance(e);
        controller.require_auth();
        Self::require_controller(e, &controller);

        if amount < 0 {
            panic_with_error!(e, StrategyError::InvalidAmount);
        }
        if Self::is_deployed_total_bootstrapped(e) {
            panic_with_error!(e, StrategyError::DeployedTotalAlreadyInitialized);
        }

        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotal, &amount);
        Self::set_deployed_total_bootstrapped(e);

        DeployedTotalSeeded { controller, amount }.publish(e);
    }

    // ── Internal helpers ────────────────────────────────────────────────

    fn bump_instance(e: &Env) {
        e.storage()
            .instance()
            .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_EXTEND_AMOUNT);
    }

    fn token_client(e: &Env) -> token::Client<'_> {
        let asset: Address = e.storage().instance().get(&StorageKey::Asset).unwrap();
        token::Client::new(e, &asset)
    }

    fn require_vault(e: &Env, addr: &Address) {
        let vault: Address = e.storage().instance().get(&StorageKey::Vault).unwrap();
        if *addr != vault {
            panic_with_error!(e, StrategyError::NotVault);
        }
    }

    fn require_controller(e: &Env, addr: &Address) {
        let controller: Address = e.storage().instance().get(&StorageKey::Controller).unwrap();
        if *addr != controller {
            panic_with_error!(e, StrategyError::NotController);
        }
    }

    /// Bound the per-call `deployed_total` change in
    /// `settle_protocol_returns` so a single transaction by a compromised
    /// controller cannot inflate or collapse the strategy's reported
    /// off-chain AUM to an arbitrary value.
    ///
    /// Zero-delta calls (no-ops) always pass. The **`DeployedTotalBootstrapped`
    /// latch** is the gate: once tripped, every settle with `previous == 0`
    /// falls through to the bps math with `max_change = 0`, rejecting any
    /// positive delta. The latch is set by three paths, each of which is
    /// the legitimate way to produce an initial non-zero `deployed_total`:
    ///   1. `seed_deployed_total` — explicit one-shot migration.
    ///   2. First successful `deploy_to_protocol` — organic bootstrap
    ///      from vault-deposited funds.
    ///   3. First settle that writes `new_total > 0` on a pristine
    ///      strategy (R2-1) — the bootstrap flow where the operator
    ///      wants to recognise an external position without passing
    ///      through `deploy_to_protocol`.
    ///
    /// The first two close the cyclic bypass (deploy → recall → settle);
    /// the third closes the first-call-on-pristine bypass. For a genuinely
    /// pristine strategy (pre-bootstrap, latch unset), settle still
    /// returns early on this exempt path — but trips the latch as a side
    /// effect in `settle_protocol_returns` so subsequent calls are gated.
    fn check_settle_rate_limit(e: &Env, previous: i128, new_total: i128) {
        // Zero-delta always passes (double-submits, precision-rounded
        // no-ops) so an audit event can still be emitted upstream.
        if new_total == previous {
            return;
        }

        // Pre-bootstrap strategies have no meaningful baseline. Once any
        // `deploy_to_protocol`, `seed_deployed_total`, or first positive
        // `settle_protocol_returns` has tripped the latch, this branch
        // no longer applies.
        //
        // R2-9: `<=` rather than `==` is defensive. `get_deployed_total`
        // saturates at 0 (`recall_from_protocol` uses `.saturating_sub`;
        // settle and seed reject negative inputs), so `previous < 0` is
        // unreachable under current write paths. If a future refactor
        // lets `deployed_total` go negative (e.g. removing the
        // saturation), `<=` continues to behave as a no-op instead of
        // falling through to bps math with a negative baseline, which
        // would produce a negative `max_change` and accept every delta.
        if previous <= 0 && !Self::is_deployed_total_bootstrapped(e) {
            return;
        }

        let (delta, limit_bps) = if new_total > previous {
            (
                new_total
                    .checked_sub(previous)
                    .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow)),
                SETTLE_INCREASE_BPS,
            )
        } else {
            (
                previous
                    .checked_sub(new_total)
                    .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow)),
                SETTLE_DECREASE_BPS,
            )
        };

        // previous can be 0 here (post-bootstrap, recalled to zero). In
        // that case max_change = 0 and any positive delta fails —
        // intentional. Operators must use `deploy_to_protocol` to create
        // a new non-zero position, not `settle`.
        let max_change = previous
            .checked_mul(limit_bps)
            .and_then(|v| v.checked_div(BPS_DIVISOR))
            .unwrap_or_else(|| panic_with_error!(e, StrategyError::MathOverflow));

        if delta > max_change {
            panic_with_error!(e, StrategyError::SettleRateLimitExceeded);
        }
    }

    fn is_deployed_total_bootstrapped(e: &Env) -> bool {
        e.storage()
            .instance()
            .get(&StorageKey::DeployedTotalBootstrapped)
            .unwrap_or(false)
    }

    fn set_deployed_total_bootstrapped(e: &Env) {
        e.storage()
            .instance()
            .set(&StorageKey::DeployedTotalBootstrapped, &true);
    }

    /// Reject protocol addresses that would cause accounting confusion
    /// or no-op transfers: the strategy itself and the vault.
    fn require_valid_protocol_target(e: &Env, protocol: &Address) {
        if *protocol == e.current_contract_address() {
            panic_with_error!(e, StrategyError::InvalidTarget);
        }
        let vault: Address = e.storage().instance().get(&StorageKey::Vault).unwrap();
        if *protocol == vault {
            panic_with_error!(e, StrategyError::InvalidTarget);
        }
    }
}
