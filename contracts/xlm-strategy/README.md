# XLM Strategy Contract

A Soroban strategy contract that implements the August vault's `IStrategy` interface and adds controller-gated fund management for deploying capital to external protocols.

## Roles

| Role | Set by | Permissions |
|------|--------|-------------|
| **Vault** | Constructor (immutable) | `deposit`, `withdraw` |
| **Controller** | Constructor, rotatable via `set_controller` | `deploy_to_protocol`, `recall_from_protocol`, `set_controller` |

The vault and controller are independent addresses. A typical setup uses the August vault as the vault address and a Utila wallet (EOA) as the controller.

## Functions

### IStrategy Interface (vault-only)

**`deposit(from, amount)`** - Notification that the vault has transferred tokens into the strategy. Zero-amount calls are no-ops (used by the vault's `add_subaccount` smoke test). No state changes; tokens are already in the strategy's balance when this is called.

**`withdraw(to, amount) -> i128`** - Transfers up to `amount` tokens back to the vault, capped by available balance. Returns the actual amount transferred.

### Controller Management

**`deploy_to_protocol(controller, protocol, amount)`** - Transfers `amount` tokens from the strategy to a `protocol` address. Rejects the strategy itself and the vault as targets. Updates `deployed_total` tracking and emits a `DeployedToProtocol` event.

**`recall_from_protocol(controller, protocol, amount)`** - Transfers `amount` tokens from a `protocol` address back to the strategy via SEP-41 `transfer`. Decrements `deployed_total` (floored at 0) and emits a `RecalledFromProtocol` event.

> **Note:** `recall_from_protocol` performs a simple token transfer. For protocols that require a custom withdrawal call (lending pools, AMMs, etc.), replace the transfer with the protocol's client invocation.

**`set_controller(current, new_controller)`** - Rotates the controller address. Only the current controller may call. Rejects the vault address and the strategy's own address as targets. Emits a `ControllerChanged` event.

### Views

| Function | Returns |
|----------|---------|
| `get_vault()` | Vault address |
| `get_controller()` | Current controller address |
| `get_asset()` | Managed token address |
| `get_balance()` | Token balance held by the strategy |
| `get_deployed_total()` | Total amount currently deployed to external protocols |

### Utilities

**`extend_ttl()`** - Permissionless instance TTL extension. Also called automatically by every mutating function.

## Events

| Event | Topics | Data |
|-------|--------|------|
| `DeployedToProtocol` | controller, protocol | amount |
| `RecalledFromProtocol` | controller, protocol | amount |
| `ControllerChanged` | old_controller | new_controller |

## Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 1 | `NotVault` | Caller is not the vault |
| 2 | `NotController` | Caller is not the controller |
| 3 | `InvalidAmount` | Amount must be positive |
| 4 | `InsufficientBalance` | Strategy balance too low |
| 5 | `InvalidTarget` | Protocol target is self or vault |
| 6 | `InvalidController` | New controller is vault or self |

## End-to-End Flow

```
Vault Operator                    XLM Strategy                 Controller (Utila EOA)
     |                                |                            |
     |-- deposit_to_subaccount ------>|  tokens land here          |
     |                                |                            |
     |                                |<-- deploy_to_protocol -----|  sends to protocol
     |                                |                            |
     |                                |<-- recall_from_protocol ---|  pulls back
     |                                |                            |
     |-- withdraw_from_subaccount --->|  tokens return to vault    |
```

The vault's `withdraw_from_subaccount` can only retrieve tokens currently held by the strategy. If funds are deployed to a protocol, the controller must `recall_from_protocol` first.

## Build and Test

```bash
# Check
cargo check -p xlm-strategy

# Test (68 tests)
cargo test -p xlm-strategy

# Release build
cargo build -p xlm-strategy --release --target wasm32-unknown-unknown
```

## Constructor

```
__constructor(asset, vault, controller)
```

- `asset` - The SEP-41 token this strategy manages (e.g. XLM wrapper)
- `vault` - The August vault address (immutable after deployment)
- `controller` - The initial controller EOA (rotatable via `set_controller`)
