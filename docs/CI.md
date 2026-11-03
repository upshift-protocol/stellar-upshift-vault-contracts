# CI and monitoring

What runs, when, and what each job protects. Every third-party action is
pinned to a full commit SHA; Dependabot (`.github/dependabot.yml`) proposes
weekly bumps.

## On every PR and push to `main` (`ci.yml`)

`CI Pass` is the single required status check. It fails if any job below
fails; skipped jobs (docs-only PRs) count as passing.

| Job | Protects against |
| --- | --- |
| Rustfmt, Clippy (`-D warnings`), Typo Check, ShellCheck | Style drift, lint-level bugs, script bugs |
| Cargo Deny | Disallowed licenses, banned crates, unknown sources (advisories are non-blocking here; see Nightly) |
| Build & Test | Unit, integration and property tests; 256 KB Soroban WASM limit |
| E2E Tests | Full deploy + flow against a pinned local Stellar quickstart |
| Code Coverage (PRs) | Line coverage below 80% |
| Reproducible Build | Built WASM ≠ `reproducible-hashes.txt`, **or** public interface ≠ `interfaces/<pkg>.rs` |
| Workflow Lint | actionlint + zizmor on the workflows themselves (template injection, unpinned actions, broad permissions) |
| Scout (advisory, not in `CI Pass`) | Soroban-specific static analysis. The job summary separates **new** findings from ones reviewed in `.github/scout-baseline.json`; new criticals get a PR annotation. Fails only if Scout itself breaks |

## Scheduled

| Workflow | When | What | On failure |
| --- | --- | --- | --- |
| `mainnet-monitor.yml` | Hourly | Fetches each contract in `deployments/mainnet.txt` from two RPC providers and compares its sha256 to the approved hash | **Drift:** `mainnet-wasm-drift` issue assigned to `ALERT_ASSIGNEES` + Slack; each distinct drift re-alerts even if an older issue is open. **RPC failure:** `mainnet-monitor-degraded` issue. Weekly, re-enables every scheduled workflow so GitHub's 60-day inactivity rule can't turn them off |
| `nightly.yml` | Daily 03:23 UTC | Strict `cargo deny check advisories`; full test suite at `PROPTEST_CASES=20000` | Opens or comments on a `nightly-failure` issue |
| `mutants.yml` | Sundays 04:41 UTC | `cargo-mutants` across the workspace, 8 shards | Report-only: missed mutants in each shard's summary and artifact |
| `scorecard.yml` | Mondays + pushes to `main` | OpenSSF Scorecard | Results in the Security tab |

Alert issues are opened through `scripts/ci/upsert-issue.sh`, which only trusts
issues and comments written by the `github-actions` bot.

### Monitor configuration

| Setting | Purpose |
| --- | --- |
| `secrets.SLACK_WEBHOOK_URL` | Slack incoming webhook for drift alerts (strongly recommended) |
| `vars.ALERT_ASSIGNEES` | Comma-separated logins to assign; defaults to the owners team members |
| `vars.MAINNET_RPC_PRIMARY`, `vars.MAINNET_RPC_SECONDARY` | RPC provider overrides |

The Stellar CLI used by Reproducible Build and the monitor is installed by
`.github/actions/install-stellar-cli` (pinned version + sha256).

## Approved-state files

These are owned by `@upshift-protocol/stellar-contracts-owners`, and changing
them is how an intended change gets approved:

- `reproducible-hashes.txt`: expected release WASM hashes.
- `interfaces/<pkg>.rs`: expected public contract interface (see `interfaces/README.md`).
- `.github/scout-baseline.json`: Scout findings reviewed as false positives, each with its reason. Matched on detector + file + code fragment, so a new finding is never hidden by an old entry.
- `deployments/mainnet.txt`: expected live mainnet WASM. **Update it in a PR after an approved upgrade**, or the monitor keeps alerting.

## Repository settings (not in git)

- `main`: code-owner review required, admins included (`enforce_admins`); the
  last push must be approved by someone else; stale reviews dismissed;
  conversations must be resolved; signed commits.
- `v*` tags: restricted by ruleset; the `release` environment requires an owner
  other than the triggering user, with no admin bypass.
- Secret scanning with push protection, Dependabot alerts and security updates.
