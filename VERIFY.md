# Reproducible Build & On-Chain Verification

This document lets anyone independently verify that the `august-vault` bytecode
deployed on **Stellar Mainnet (Public network)** was built from the source in
this repository.

The build is reproducible: compiling this repo with the toolchain below produces
an optimized WASM whose SHA-256 hash is **byte-for-byte identical** to the
bytecode running on-chain. If your locally-built hash equals the on-chain hash,
you have cryptographic proof that the live contract is exactly this source.

## Deployed instances

Both live vaults run the **same** `august-vault` bytecode:

| Vault | Contract ID | On-chain WASM hash |
| --- | --- | --- |
| Gami Earn USDC | `CCL3WITWFFXIHV2I52ECV5DPIEOFSTU3PBPR53ILPLF2IP5KHECXRUTY` | `4b3d9f6b…` |
| Gami Earn XLM  | `CC6TRAPQD3NK7THUKWPV5SL2JHKQGNXZVB6S6MVYFSLRWAKEFUWZKZ7J` | `4b3d9f6b…` |

```
4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73   (91,853 bytes, optimized)
```

## Required toolchain (must match exactly)

| Component | Version |
| --- | --- |
| Rust | `1.95.0` |
| soroban-sdk | `25.1.1` (pinned in `Cargo.lock` — do not update it) |
| Build target | `wasm32-unknown-unknown` |
| Stellar CLI (for `optimize`) | `25.1.0` |

> **Build target matters.** These contracts were built for `wasm32-unknown-unknown`.
> Newer Stellar CLI `contract build` defaults to the `wasm32v1-none` target, which
> produces a *different* (still valid) hash. To reproduce **these** deployments you
> must use `wasm32-unknown-unknown`, exactly as shown below.

## Reproduce the hash

```bash
# 1. Install the exact toolchain + target
rustup toolchain install 1.95.0
rustup target add wasm32-unknown-unknown --toolchain 1.95.0

# 2. Build august-vault (release) using the committed Cargo.lock
cargo +1.95.0 build -p august-vault \
  --target wasm32-unknown-unknown --release

# 3. Optimize with Stellar CLI 25.1.0 (deployments use the optimized WASM)
stellar contract optimize \
  --wasm target/wasm32-unknown-unknown/release/august_vault.wasm \
  --wasm-out august_vault.optimized.wasm

# 4. Hash it
shasum -a 256 august_vault.optimized.wasm
# expected: 4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73
```

The optimized WASM should be **91,853 bytes**.

## Compare against the on-chain bytecode

```bash
# Fetch the deployed WASM and hash it (repeat for either contract ID)
stellar contract fetch \
  --id CCL3WITWFFXIHV2I52ECV5DPIEOFSTU3PBPR53ILPLF2IP5KHECXRUTY \
  --rpc-url https://mainnet.sorobanrpc.com \
  --network-passphrase "Public Global Stellar Network ; September 2015" \
  --out-file onchain.wasm

shasum -a 256 onchain.wasm
# 4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73
```

A matching hash proves the deployed contract is built from this exact source.

## Confirm the embedded build metadata

The deployed WASM records the toolchain it was built with — it should agree with
the versions above:

```bash
stellar contract info meta --wasm onchain.wasm
# binver:   0.1.0
# rsver:    1.95.0
# rssdkver: 25.1.1#...
```

## Notes

- `august-vault` is the only contract these two vaults run. `xlm-strategy` is a
  separate contract and is not deployed as either of the instances above.
- The on-chain bytecode predates [stellar.expert's automated build attestation](https://github.com/stellar-expert/soroban-build-workflow),
  which builds the `wasm32v1-none` target and injects a `source_repo` tag — so the
  automated "verified" badge would only match a future redeploy/upgrade built by
  that workflow. The manual reproduction above provides equivalent assurance for
  the contracts as they are deployed today.
