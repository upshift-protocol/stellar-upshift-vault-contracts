# Security Audit Coverage

This document records which security review the contracts in this repository
have received, exactly which source revision that review covered, and how the
reviewed source relates to the code in this repository and to the bytecode
deployed on Stellar Mainnet.

> **Summary.** The `august-vault` and `xlm-strategy` contracts were assessed by
> Halborn in March–April 2026. The source in this repository is the audited,
> remediated code **plus one post-audit addition** to `august-vault` (atomic
> multi-wallet reconciliation, see [§3](#3-changes-not-covered-by-the-audit)).
> That addition is outside the audit's stated scope. The `xlm-strategy`
> contract is unchanged since the last remediation commit Halborn reviewed.

---

## 1. Halborn assessment (March–April 2026)

| Item | Value |
| --- | --- |
| Auditor | [Halborn](https://www.halborn.com) |
| Report | *Stellar Vaults – Upshift*, Security Assessment. Engagement 2026-03-25 → 2026-03-30, report last updated 2026-04-28. |
| Contracts in scope | `august-vault` (`contract.rs`, `errors.rs`, `events.rs`, `lib.rs`, `storage.rs`, `strategy.rs`) and `xlm-strategy` (`lib.rs`) |
| Source reviewed | Private development repository `fractal-protocol/stellar-vaults` (this repository was created later from that source, see §2) |
| Submitted snapshot | `47207d768bc39c6fa6379f46f399744818eda662` (2026-03-20) |
| Assessed commit | `18c36059acd4581b837d7b094eac9e66225cefd2` (2026-04-21) |
| Remediation commits reviewed | `6ff1456ddb9bd1e8e3d4b64fcefcd6e507181c04` (2026-04-02), `9f549ae7158aed8f370eebd88cc2cc147e1747b3` (2026-04-07), `9d9860db36898f4e8f2b65c893522c39805db880` (2026-04-07), `d19a8e382f02ecbf65d00823c961681ecd61d5ad` (2026-04-23) |
| Stated exclusion | "New features/implementations after the remediation commit IDs." |

### Findings

Fourteen findings: 0 critical, 0 high, 2 medium, 5 low, 7 informational.

| # | Finding | Severity | Status |
| --- | --- | --- | --- |
| 7.1 | Operator-attested NAV with no on-chain verification path | Medium | Partially solved (2026-04-14). `IStrategy::get_balance()` added and used in NAV; the recommended `IProtocol` adapter layer deferred, residual risk accepted. |
| 7.2 | Strategy donation attack / untracked external transfers inflate NAV | Medium | Solved (2026-04-23) |
| 7.3 | Absence of `IProtocol` interface prevents DeFi integration | Low | Risk accepted (2026-04-14) |
| 7.4 | `IStrategy` should enforce `get_balance()` semantics | Low | Solved (2026-04-23) |
| 7.5 | Strategy asset not validated against vault asset at registration | Low | Solved (2026-04-07) |
| 7.6 | First-depositor share inflation attack possible when `decimals_offset` is zero | Low | Solved (2026-04-02) |
| 7.7 | `recall_from_protocol` does not use balance-differencing | Low | Solved (2026-04-07) |
| 7.8 | `remove_subaccount` does not reconcile wallet `deployed_assets` on removal | Informational | Solved (2026-04-07) |
| 7.9 | No rate limit on `settle_protocol_returns` | Informational | Solved (2026-04-23) |
| 7.10 | `remove_subaccount` does not enforce accounting preconditions on wallet removal | Informational | Solved (2026-04-23) |
| 7.11 | Authorization check occurs after identity equality check | Informational | Solved (2026-04-07) |
| 7.12 | Inconsistent TTL management across vault and strategy | Informational | Solved (2026-04-07) |
| 7.13 | `update_total_assets` misleadingly named | Informational | Solved (2026-04-02) |
| 7.14 | Strategy removal does not record NAV impact on-chain | Informational | Solved (2026-04-23) |

Two items remain open by decision rather than omission. Both concern the
`IProtocol` adapter layer that would let a strategy report positions held in
external DeFi protocols on-chain. The current deployment model uses custody
wallets and a single token-wrapper strategy with an externally owned
counterparty, so the adapter is deferred until a venue that can report
positions on-chain is integrated.

---

## 2. How the audited source maps to this repository

This repository was created on 2026-06-30 with a single squashed
[initial commit](../../../commit/33a02f4d17de27d899ca5686e5bb77250222c501)
(`33a02f4`). Its `contracts/`, `Cargo.toml` and `Cargo.lock` are byte-identical
to the private development repository's `main` branch immediately before the
contract source was moved here.

That state is the last remediation commit Halborn reviewed (`d19a8e3`) plus the
following commits, in order:

| Private commit | Date | Change | In audit scope? |
| --- | --- | --- | --- |
| `8d47c471ee08922b141f924cfa567d0f52fdb809`, `dfea1ad27eec8c29749a108c9af3b005a22b974a` | 2026-04-24 | Atomic multi-wallet reconciliation (`update_wallet_deployed_batch`, `get_wallet_deployed_assets`, two error codes, one event, docstring re-scoping) | **No** |
| `c0b8363b27a025123bb1ca3be0c6fb1e2e904ced` | 2026-04-27 | Per-entry pause enforcement inside the batch function | **No** |
| `45bc0461ff9a27784cbec714e02269a6fdc6ed5a` | 2026-05-12 | Property-based tests for share-price math (`proptest` dev-dependency, tests only, not in the WASM) | n/a (test code) |

Per-file coverage of the source in this repository:

| File | Relation to `d19a8e3` | Covered by audit |
| --- | --- | --- |
| `contracts/xlm-strategy/src/lib.rs` | identical | Yes |
| `contracts/august-vault/src/storage.rs` | identical | Yes |
| `contracts/august-vault/src/lib.rs` | identical | Yes |
| `contracts/august-vault/src/strategy.rs` | docstring changes only | Yes (behaviour unchanged) |
| `contracts/august-vault/src/contract.rs` | new function `update_wallet_deployed_batch`, new view `get_wallet_deployed_assets`, docstring changes | **Partially**, see §3 |
| `contracts/august-vault/src/errors.rs` | two new variants (`EmptyBatch` = 26, `DuplicateSubaccountInBatch` = 27) | Partially |
| `contracts/august-vault/src/events.rs` | one new event (`WalletDeployedBatchApplied`) | Partially |
| `Cargo.lock` (production dependencies) | identical: `soroban-sdk` 25.1.1, `stellar-tokens` 0.6.0 | Yes |

Later commits in this repository (`v0.1.0`, `v0.1.1`) change only the crate
version strings, the pinned toolchain, CI and documentation. Contract source is
unchanged since `33a02f4`.

---

## 3. Changes not covered by the audit

The following production code in `august-vault` post-dates the last
remediation commit and is therefore outside Halborn's stated scope. It was
implemented in response to a design recommendation raised during the
remediation review (commit to a wallet-only model for `deployed_assets` and
provide an atomic batch primitive), but the implementation itself was not
reviewed in the published report.

- **`update_wallet_deployed_batch(operator, Vec<(Address, i128)>)`** —
  operator-only. Reconciles several Wallet subaccounts in one transaction.
  Validates: non-empty, at most `MAX_SUBACCOUNTS` (10) entries, no duplicate
  addresses, every address whitelisted and of type Wallet, every new tracker
  value non-negative. Computes the net delta across all entries and applies it
  to `deployed_assets` exactly once through the same `apply_deployed_assets_change`
  helper (AUM rate limits, pause check, underflow check) that the audited
  single-wallet path uses. While the vault is paused, any entry with a positive
  delta is rejected regardless of the batch's net. All-or-nothing: any failure
  reverts the whole batch.
- **`get_wallet_deployed_assets()`** — read-only. Returns Σ `WalletNetDeployed`
  over whitelisted Wallet subaccounts, exposing the invariant
  `Σ WalletNetDeployed == deployed_assets` for off-chain monitoring.
- **Error codes** `EmptyBatch` (26) and `DuplicateSubaccountInBatch` (27).
- **Event** `WalletDeployedBatchApplied { operator, count, net_delta }`,
  emitted once per batch after the per-entry `WalletDeployedUpdated` events.
- **Documentation** in `contract.rs` and `strategy.rs` re-scoping
  `deployed_assets` as exclusively wallet-attributed and describing
  `update_deployed_assets` and `seed_wallet_net_deployed` as emergency escape
  hatches. No behaviour change.

The new entrypoint is operator-privileged and moves NAV, so it should be
included in the next review.

---

## 4. Deployed bytecode

The vaults live on Stellar Mainnet run the **legacy build profile** of commit
`33a02f4` (target `wasm32-unknown-unknown`, Rust 1.95.0, Stellar CLI 25.1.0,
no `source_repo` metadata). Building that commit with that toolchain
reproduces the on-chain bytecode byte-for-byte:

```
4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73   august_vault (91,853 bytes)
```

The attested release builds (`v0.1.0`, `v0.1.1`; hashes in
[`reproducible-hashes.txt`](../reproducible-hashes.txt)) are compiled from the
same contract source with a different target and CLI version and a bumped
crate version string, so they hash differently. The coverage statement above
applies equally to them.

The vaults are upgradeable, so any hash match is **point-in-time**. Always
re-fetch the live on-chain hash before relying on it. The full procedure, for
both build profiles, is in [VERIFY.md](../VERIFY.md).
