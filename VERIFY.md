# Reproducible Build & On-Chain Verification

This repo produces reproducible WASM: compiling with the pinned toolchain yields
a binary whose SHA-256 is byte-for-byte identical to what runs on-chain / what is
attested. **Two build profiles are relevant, and they produce different hashes** —
use the one that matches what you are verifying:

| Profile | Target | Metadata | Applies to |
| --- | --- | --- | --- |
| **A — Legacy** | `wasm32-unknown-unknown` | none | the currently-deployed Gami vaults |
| **B — Attested release** | `wasm32v1-none` | `source_repo` embedded | the `Release & Attest` workflow (new vaults) |

Both profiles share the same toolchain: **Rust 1.95.0**, **soroban-sdk 25.1.1**
(pinned in `Cargo.lock` — do not update it), **Stellar CLI 25.1.0**.

---

## A — Verify the currently-deployed vaults (legacy build)

Both live vaults run the **same** `august-vault` bytecode, built for
`wasm32-unknown-unknown` with no source metadata:

| Vault | Contract ID | On-chain WASM hash |
| --- | --- | --- |
| Gami Earn USDC | `CCL3WITWFFXIHV2I52ECV5DPIEOFSTU3PBPR53ILPLF2IP5KHECXRUTY` | `4b3d9f6b…` |
| Gami Earn XLM  | `CC6TRAPQD3NK7THUKWPV5SL2JHKQGNXZVB6S6MVYFSLRWAKEFUWZKZ7J` | `4b3d9f6b…` |

```
4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73   (91,853 bytes)
```

Reproduce:

```bash
# These vaults are immutable. Build from the repo revision that matches the
# on-chain metadata (rsver 1.95.0, rssdkver 25.1.1) — the initial published
# commit reproduces the current deployment. Check out that commit first if the
# repository has since advanced.
rustup toolchain install 1.95.0
rustup target add wasm32-unknown-unknown --toolchain 1.95.0

# Build for the legacy target using the committed Cargo.lock, then optimize
cargo +1.95.0 build -p august-vault --target wasm32-unknown-unknown --release
stellar contract optimize \
  --wasm target/wasm32-unknown-unknown/release/august_vault.wasm \
  --wasm-out august_vault.optimized.wasm

shasum -a 256 august_vault.optimized.wasm
# 4b3d9f6b09f7127b0ce81b0ce9d8428f3f960e4e3ce13c11c1cbb446cccc9e73   (91,853 bytes)
```

Compare against the on-chain bytecode:

```bash
stellar contract fetch --id CCL3WITWFFXIHV2I52ECV5DPIEOFSTU3PBPR53ILPLF2IP5KHECXRUTY \
  --rpc-url https://mainnet.sorobanrpc.com \
  --network-passphrase "Public Global Stellar Network ; September 2015" \
  --out-file onchain.wasm

shasum -a 256 onchain.wasm                     # 4b3d9f6b…
stellar contract info meta --wasm onchain.wasm # binver 0.1.0, rsver 1.95.0, rssdkver 25.1.1
```

> These vaults predate the `Release & Attest` workflow, so they carry no
> `source_repo` metadata and can't get the stellar.expert auto-badge without a
> redeploy — this manual reproduction is their verification.

---

## B — Verify an attested release build (new vaults)

The `Release & Attest` workflow (`.github/workflows/release.yml`) builds each
contract for the default **`wasm32v1-none`** target and embeds a **`source_repo`**
tag, then attests the result and registers it with stellar.expert. This is the
build new vaults deploy, and it hashes **differently** from the legacy profile —
so use this recipe (not profile A) to reproduce a release/attestation hash.

```bash
# 1. Check out the EXACT revision the release was built from — otherwise you'll
#    hash whatever you currently have checked out. Both are in the release notes:
#    the commit SHA, and the tag <version>-<package>.
git checkout v0.1.0-august-vault        # or: git checkout <commit-from-release-notes>

# 2. Install the pinned toolchain + target
rustup toolchain install 1.95.0
rustup target add wasm32v1-none --toolchain 1.95.0

# 3. Run the same command the release workflow runs (source_repo makes the hash
#    repo-specific, so keep it exactly as below)
RUSTUP_TOOLCHAIN=1.95.0 stellar contract build --optimize \
  --package august-vault \
  --out-dir out \
  --meta source_repo="github:upshift-protocol/stellar-upshift-vault-contracts"

shasum -a 256 out/august_vault.wasm
```

For example, `august-vault` **v0.1.0** produces:

```
f23d13a79d0903150bf2c32a482634c7dfe3f8828474b58e574ac6d5475d23b2   (92,025 bytes)
```

This should match the hash in the release notes, the GitHub build attestation
(`https://github.com/upshift-protocol/stellar-upshift-vault-contracts/attestations`),
and the WASM hash of any vault deployed against it — which then shows as verified
on stellar.expert.

---

## Notes

- `august-vault` is the vault contract; `xlm-strategy` is a separate contract and
  is not one of the deployed vault instances listed above. The release workflow
  attests both.
