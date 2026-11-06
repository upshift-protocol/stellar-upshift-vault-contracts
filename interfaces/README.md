# Contract interfaces

`<package>.rs` is the public interface (functions, types, errors, events) of
each contract, as read back from the attested release WASM by
`stellar contract info interface --output rust`.

The `Reproducible Build` CI job regenerates these from the freshly built WASM
and fails if they differ. Any change to a contract's public surface therefore
shows up as a reviewable diff in this directory, in the same PR that makes it.
That matters for an upgradeable contract: integrators and users depend on this
surface, and an upgrade can silently change it.

## Regenerating

Use the same pinned toolchain as the release build (Rust 1.95.0, Stellar CLI
27.0.0; see VERIFY.md § B):

```bash
make interfaces
```

If you can't run CLI 27.0.0 locally, take the `diff -u` output that the failing
CI step prints and apply it.
