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

> **Verification is point-in-time.** These vaults are **upgradeable** — the admin
> upgrade authority can replace the WASM (see the README's Security section). A
> matching hash therefore proves the deployed code **at the moment you fetch it**,
> not forever. Re-fetch the live on-chain hash whenever you need fresh assurance;
> a past match does not guarantee the current bytecode.

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
# Compiler/SDK metadata alone can't identify the source revision, so check out
# the exact commit the current on-chain bytecode was built from:
git checkout 33a02f4d17de27d899ca5686e5bb77250222c501

rustup toolchain install 1.95.0
rustup target add wasm32-unknown-unknown --toolchain 1.95.0

# Build for the legacy target, enforcing the committed Cargo.lock, then optimize
cargo +1.95.0 build -p august-vault --target wasm32-unknown-unknown --release --locked
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
> `source_repo` metadata and can't get the stellar.expert auto-badge without
> either a redeploy **or an upgrade to an attested build** (the vault is
> upgradeable via WASM replacement) — meanwhile this manual reproduction is
> their verification.

---

## B — Verify an attested release build (new vaults)

The `Release & Attest` workflow (`.github/workflows/release.yml`) builds each
contract for the **`wasm32v1-none`** target with a **`source_repo`** tag, attests
the result, and registers it with stellar.expert. This is the build new vaults
deploy; it hashes **differently** from the legacy profile A.

> **Reproduce in the pinned container.** WASM output is platform-sensitive — a
> bare build on a different host (e.g. macOS/arm64) yields different bytes. The
> release is built on Linux **x86_64**; the `linux/amd64` `rust:1.95.0` container
> below reproduces that platform on any host and yields the attested hashes
> **byte-for-byte** (verified against the v0.1.0 release).

```bash
git clone https://github.com/upshift-protocol/stellar-upshift-vault-contracts
cd stellar-upshift-vault-contracts
git checkout v0.1.0        # the release tag (its notes also record the commit SHA)

docker run --rm --platform linux/amd64 -v "$PWD:/src:ro" \
  -e RUSTUP_TOOLCHAIN=1.95.0 rust:1.95.0 bash -c '
    set -euo pipefail
    apt-get update -qq && apt-get install -y -qq curl ca-certificates git >/dev/null
    rustup target add wasm32v1-none
    curl -fsSL -o /tmp/s.deb \
      https://github.com/stellar/stellar-cli/releases/download/v25.1.0/stellar-cli_25.1.0_amd64.deb
    echo "0260de467b29883c7cc227a3d8df7b7d8723805ffa54dcfe8171af1255de33c8  /tmp/s.deb" | sha256sum -c -
    dpkg -i /tmp/s.deb 2>/dev/null || apt-get install -y -f -qq
    git config --global --add safe.directory /src
    mkdir /build && git -C /src archive HEAD | tar -x -C /build && cd /build
    for p in august-vault xlm-strategy; do
      stellar contract build --optimize --package "$p" --out-dir "/tmp/$p" \
        --meta source_repo="github:upshift-protocol/stellar-upshift-vault-contracts"
      sha256sum "/tmp/$p"/*.wasm
    done
  '
```

For **v0.1.0** this reproduces the attested hashes exactly:

```
august-vault:  3bd05dfa2bf65299d359e86a4672a45900e8c5768d9f056ad8da5ccd779bcd12   (92,025 bytes)
xlm-strategy:  e3443e37b6da76ef061051f5e170537ef6e596711869c09a1ec9cd9e55345da4
```

These match the release assets, the GitHub build attestation
(`https://github.com/upshift-protocol/stellar-upshift-vault-contracts/attestations`),
and the WASM hash of any vault deployed against them — which then shows as verified
on stellar.expert.

> **Filename note:** compare by **hash**, not filename — the published release asset
> is `<package>_v<version>.wasm` (e.g. `august-vault_v0.1.0.wasm`).

---

## Notes

- `august-vault` is the vault contract; `xlm-strategy` is a separate contract and
  is not one of the deployed vault instances listed above. The release workflow
  attests both.
