# Supply Chain Blockchain — Rust

A Aupply chain tracking system With tags built in Rust. For now it works completely fine on multiple terminals, just to show case my project functionality through a single device. But as the project will grow it will work on multiple devices just like real blockchain used to work.  
Uses TCP peer-to-peer communication, SHA-256 Merkle-tree hashing, ED25519 signatures, and >67% consensus voting — with a built-in in-memory mock database so you can test **locally with zero infrastructure**.

---

## Project Structure

```
supply-chain-blockchain/
├── Cargo.toml
├── .env                          ← DB URLs (auto-falls back to mock if missing/invalid)
├── wallet.json                   ← Created at runtime by `cargo run -- wallet`
├── check_layer.json              ← Created at runtime by `cargo run -- manufacturer`
└── src/
    ├── main.rs                   ← CLI router (clap subcommands)
    ├── crypto/
    │   ├── mod.rs
    │   ├── hash.rs               ← SHA-256 generic hashing
    │   └── merkle.rs             ← Merkle root tree
    ├── wallet/
    │   ├── mod.rs
    │   └── cli.rs                ← Wallet creation + wallet.json I/O
    ├── state/
    │   ├── mod.rs
    │   └── check_layer.rs        ← CheckLayer state machine
    ├── network/
    │   ├── mod.rs
    │   ├── message.rs            ← NetworkMessage protocol (length-prefixed JSON/TCP)
    │   ├── peer.rs               ← TCP client + broadcast
    │   └── server.rs             ← Verifier TCP server + message handlers
    ├── consensus/
    │   ├── mod.rs
    │   └── vote.rs               ← >67% voting mechanism
    └── db/
        ├── mod.rs                ← Database trait + factory (auto-fallback)
        └── mock.rs               ← In-memory HashMap mock DB
```

---

## Prerequisites

- **Rust** (stable, 1.75+): https://rustup.rs
- No PostgreSQL needed for local testing (mock DB activates automatically)

```powershell
rustup update stable
```

---

## Building

```powershell
cd "supply-chain-blockchain"
cargo build
```

First build downloads and compiles all dependencies (~2–3 min). Subsequent builds are fast.

---

## Full Multi-Terminal End-to-End Test

Run these **5 steps in 5 separate terminal windows**, in the **exact order shown**.  
Open all terminals in the `supply-chain-blockchain/` directory.

---

### Step 0 — Create Wallets (run once per role)

Each actor needs their own `wallet.json`. Since all 5 actors share the same directory  
in this demo, we'll create wallets sequentially and rename them.

#### 0a — Create a Verifier Wallet

```powershell
# Terminal 0 — one-time setup
cargo run -- wallet
```

**Inputs:**
- Aadhaar Number → `123456789012`
- Govt ID → `VERIF001` *(press Enter if you want to skip and select from menu)*
- If via menu: Select **"Verifier Node"**
- Display name → `Verifier Alpha`

Rename the file:
```powershell
Rename-Item wallet.json verifier_wallet.json
```

#### 0b — Create a Manufacturer Wallet

```powershell
cargo run -- wallet
```

**Inputs:**
- Aadhaar → `111222333444`
- Govt ID → *(press Enter to skip)*
- Role menu → **"Manufacturer (requires company verification)"**
- Company Name → `AcmeCorp`
- Manufacturer Govt ID → `MFR001`   *(pre-seeded in mock DB)*

Rename:
```powershell
Rename-Item wallet.json manufacturer_wallet.json
```

#### 0c — Create Warehouse, Distributor, Receiver Wallets

Repeat the pattern for each role. Use these mock DB credentials:

| Role        | Aadhaar      | Govt ID   | Company    | Menu Choice    |
|-------------|-------------|-----------|------------|----------------|
| Warehouse   | `555666777`  | *(skip)*  | —          | Warehouse Worker |
| Distributor | `888999000`  | *(skip)*  | `FedEx`    | Distributor → `DIST001` |
| Receiver    | `444333222`  | *(skip)*  | —          | Receiver       |

Rename each:
```powershell
Rename-Item wallet.json warehouse_wallet.json
Rename-Item wallet.json distributor_wallet.json
Rename-Item wallet.json receiver_wallet.json
```

---

### Terminal 1 — Start Verifier Node

```powershell
cargo run -- verifier --wallet verifier_wallet.json --port 9000
```

**Expected output:**
```
╔══════════════════════════════════════════╗
║     VERIFIER NODE  — ACTIVE LISTENER     ║
╚══════════════════════════════════════════╝
  Node Address : 3f8a1c2d...
  Listening on : 0.0.0.0:9000
  Waiting for incoming transactions...
```

**Leave this terminal running.** It prints each vote as messages arrive.

> To run multiple verifiers (3-node consensus), start verifiers on ports 9001, 9002 too:
> ```powershell
> cargo run -- verifier --wallet verifier_wallet.json --port 9001
> cargo run -- verifier --wallet verifier_wallet.json --port 9002
> ```
> Then use `--peers 127.0.0.1:9000,127.0.0.1:9001,127.0.0.1:9002` in later commands.

---

### Terminal 2 — Manufacturer: Initialize Chain

```powershell
cargo run -- manufacturer --wallet manufacturer_wallet.json --peers 127.0.0.1:9000
```

**Inputs when prompted:**
- **FINAL_HASH**: Press Enter → enter batch name: `BatchAlpha2024`
  - Auto-generates: `a3f7...` (note this hash — you'll use it everywhere)
- **Initial Agent ID**: `agent001`
- **Warehouse 0 address**: `warehouse-delhi`
- **Warehouse 1 address**: `warehouse-mumbai`
- **Warehouse 2 address**: Press Enter to finish
- **Distributor address**: `distributor-fedex`

**Expected output:**
```
CheckLayer created:
{
  "warehouse_0": { "w_a": "3f2a...", "a_id": "c8d1...", "status": "open" },
  "warehouse_1": { "w_a": "8b1c...", "a_id": "",        "status": "lock" },
  "Distributor":  { "w_a": "f4e2...", "a_id": "",        "status": "lock" }
}
Batch ID: 9a3b1c...
Saved to: check_layer.json
```

**Verifier Terminal 1 will print:**
```
→ InitCheckLayer received...
→ Merkle verification: MATCH
Casting vote: APPROVE
```

---

### Terminal 3 — Warehouse Worker: Handoff at Warehouse 0

```powershell
cargo run -- warehouse --wallet warehouse_wallet.json --peers 127.0.0.1:9000
```

**Inputs:**
- **FINAL_HASH**: `<paste the exact hash from manufacturer step>`
- **Agent ID**: `agent001`

**Expected:**
```
CONSENSUS REACHED
Votes: 1/1 (100.0%) — Threshold: 67%
State transition applied!
  warehouse_0: { ..., "status": "done" }
  warehouse_1: { ..., "status": "open" }
```

---

### Terminal 3 Again — Warehouse Worker: Handoff at Warehouse 1

```powershell
cargo run -- warehouse --wallet warehouse_wallet.json --peers 127.0.0.1:9000
```

Same inputs (FINAL_HASH + Agent ID). The index automatically advances.

---

### Terminal 4 — Distributor: Finalize & Create Small Box Tags

```powershell
cargo run -- distributor --wallet distributor_wallet.json --peers 127.0.0.1:9000
```

**Inputs:**
- **FINAL_HASH**: `<same hash from manufacturer>`
- **Agent ID**: `dist_agent_001`
- **Receiver wallet address**: `<paste the wallet_address from receiver_wallet.json>`
  ```powershell
  # Quick way to find it:
  Get-Content receiver_wallet.json | Select-String "wallet_address"
  ```
- **Number of small boxes**: `3`
- **Box 1 hash**: Press Enter → auto-generated
- **Box 2 hash**: Press Enter → auto-generated
- **Box 3 hash**: Press Enter → auto-generated

**Save the auto-generated box hashes that print** — you'll need them in Terminal 5.

**Expected:**
```
📦  Small Box Tag:
   Tag ID   : a8f3c1...
   Boxes    : 3
   Merkle   : 7b2d9e...
   Distributor finalization complete!
```

---

### Terminal 5 — Receiver: Final Verification

```powershell
cargo run -- receiver --wallet receiver_wallet.json --peers 127.0.0.1:9000
```

**Inputs:**
- **FINAL_HASH**: `<same batch FINAL_HASH>`
- **How many boxes**: `3`
- **Box 1 hash**: `<paste the auto-generated box hash from distributor step>`
- **Box 2 hash**: `<paste box 2 hash>`
- **Box 3 hash**: `<paste box 3 hash>`

**Expected (success):**
```
  ════════════════════════════════════
  CHAIN APPENDED — Shipment Complete! 
  ════════════════════════════════════
```

**Expected (tampered hashes):**
```
🚨  FLAGGED — Data mismatch detected!
```

---

### Check State at Any Time

```powershell
cargo run -- status
```

Prints the current JSON state of `check_layer.json`.

---

## Command Reference

| Command | Description |
|---------|-------------|
| `cargo run -- wallet` | Create a new wallet interactively |
| `cargo run -- verifier --port 9000` | Start verifier node on port 9000 |
| `cargo run -- manufacturer --peers 127.0.0.1:9000` | Init shipment chain |
| `cargo run -- warehouse --peers 127.0.0.1:9000` | Submit warehouse handoff |
| `cargo run -- distributor --peers 127.0.0.1:9000` | Distributor finalization |
| `cargo run -- receiver --peers 127.0.0.1:9000` | Final receiver check |
| `cargo run -- status` | Print current chain state |

### Common Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--wallet <path>` | `wallet.json` | Path to wallet file |
| `--port <port>` | `9000` | TCP port for verifier |
| `--peers <addr,...>` | `127.0.0.1:9000` | Comma-separated verifier addresses |
| `--layer-file <path>` | `check_layer.json` | Path to check layer file |

---

## Multi-Verifier Consensus Testing

To test the >67% threshold, run 3 verifiers:

```powershell
# Terminal A
cargo run -- verifier --wallet verifier_wallet.json --port 9000

# Terminal B  
cargo run -- verifier --wallet verifier_wallet.json --port 9001

# Terminal C
cargo run -- verifier --wallet verifier_wallet.json --port 9002
```

Then use `--peers 127.0.0.1:9000,127.0.0.1:9001,127.0.0.1:9002` in all actor commands.

With 3 verifiers, you need **at least 2 approvals** (66.6% is NOT enough — must be strictly > 67%, so 3/3 = 100% or 2/3 = 66.7% which also doesn't pass — you need 3/3 for 3 nodes, or use 5 nodes where 4/5=80% passes).

---

## Mock Database Credentials

Pre-seeded for local testing (no real DB needed):

| Type | Govt ID | Company |
|------|---------|---------|
| Manufacturer | `MFR001` | `AcmeCorp` |
| Manufacturer | `MFR002` | `GlobalGoods` |
| Distributor | `DIST001` | — |
| Distributor | `DIST002` | — |

---

## Architecture Notes

- **Hashing**: All IDs, addresses, and tags stored as SHA-256 hex hashes. Never raw strings.
- **Transport**: Length-prefixed JSON over TCP (4-byte big-endian length header + JSON body).
- **Signatures**: ED25519 via `ed25519-dalek`. Private key never leaves `wallet.json`.
- **State file**: `check_layer.json` is the shared state between terminals (acts as a simple local ledger). In production, replace with a replicated distributed store.
- **Consensus**: `approve_count / total_nodes > 0.67`. Votes are deduplicated by `voter_address`.

---

## Running Unit Tests

```powershell
cargo test
```

Tests cover:
- SHA-256 determinism and uniqueness
- Merkle root computation (ordering, single leaf, even/odd lengths)
- Voting threshold logic (67% boundary cases)
