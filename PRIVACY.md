# Lezer Wallet — Privacy & Security Deep Dive

> **For security researchers, auditors, and privacy-conscious users.** This document details the cryptographic protocols, privacy guarantees, and threat model of Lezer Wallet.

---

## 1. Threat Model

| Adversary | Capability | Mitigation |
| :--- | :--- | :--- |
| **Malicious dApp** | Calls `lezer_wallet_provider` LIDL methods, crafts malicious transaction intents | Session token + capability ACL enforced in `lezer-core`; decoded call effects shown before signing; `ReadPrivateBalance` gate |
| **Compromised QML/WebView** | Reads UI memory, intercepts IPC | No secrets in UI layer; `ui_token` required for `execute_privileged`; all key material in `lezer-core` |
| **Network Observer** | Sees JSON-RPC traffic to sequencer | TLS encryption; private account transactions leak only Pedersen commitments + ZK proofs (no sender/recipient/amount) |
| **Logos Storage Node** | Stores backup ciphertext | `SealedVaultEnvelopeV1` encrypted client-side; node sees only opaque ciphertext + CID |
| **Physical Attacker** | Steals `vault.enc` | Argon2id + XChaCha20-Poly1305; brute-force requires password cracking (t=3, m=64MiB, p=4) |
| **Crash/Power Loss** | Interrupts vault write or ZK proof | Atomic write-temp-fsync-rename; nullifier journal rejects double-spends on restart |

---

## 2. Public vs. Private Accounts

### Public Account (`Public/<base58>`)

- **Address:** `SHA256(ed25519_public_key)` → base58
- **State:** Balance, nonce, `program_owner`, and data are **publicly visible** on-chain.
- **Authorization:** Ed25519 Schnorr signature on transaction.
- **Privacy:** None — like Bitcoin/Ethereum accounts.

### Private Account (`Private/<base58>`)

- **Address:** Deterministic `lezer:priv1...` URI encoding `(vpk, npk)`.
- **State:** Stored on-chain only as a **Pedersen commitment** in the zone Merkle tree.
- **Authorization:** RISC Zero ZK proof (Groth16) that the spender knows `nsk` and the note is unspent.
- **Privacy:** Full — sender, recipient, and amount are hidden. Only a 4-byte view tag is exposed for scanning.

### Key Derivation

```
mnemonic (BIP-39, 24 words)
  └── SLIP-0010 / HKDF
       ├── Public Chain (m/44'/9999'/0'/0/i)
       │    └── Ed25519 Signing Key → Public/<base58>
       └── Private Chain (m/44'/9999'/1'/0/i)
            ├── Ed25519 Nullifier Keypair (nsk → npk)
            └── ML-KEM-768 Viewing Keypair (vsk → vpk)
                 └── lezer:priv1<checksummed-vpk||npk>
```

---

## 3. Privacy Guarantees by Transfer Mode

| Mode | From | To | On-Chain Visibility | ZK Proof |
| :--- | :--- | :--- | :--- | :--- |
| `PublicToPublic` | `Public/` | `Public/` | Full (sender, recipient, amount) | No |
| `ShieldPublicToPrivate` | `Public/` | `Private/` | Sender + amount public; recipient hidden | No (shielding is transparent) |
| `DeshieldPrivateToPublic` | `Private/` | `Public/` | Recipient + amount public; sender hidden | Yes (Groth16) |
| `PrivateToPrivate` | `Private/` | `Private/` | None (commitments only) | Yes (Groth16) |

---

## 4. View-Tag Scanning

- Every incoming private note carries a **4-byte view tag** derived from `vpk` + ephemeral key.
- `lezer-core` scans the note commitment set, checks the view tag first, and only attempts ML-KEM-768 decapsulation on ~1% of notes (statistical match rate).
- **Result:** O(n/256) scanning cost instead of O(n) full decapsulation.

---

## 5. Vault Encryption (`SealedVaultEnvelopeV1`)

```
┌─────────────────────────────────────────────────────────┐
│  SealedVaultEnvelopeV1                                  │
├─────────────────────────────────────────────────────────┤
│  version        : u8    = 0x01                          │
│  salt           : [u8;16] = random()                    │
│  argon2_params  : t=3, m=64MiB, p=4, out=32             │
│  header_mac     : SHA256(version||salt||params||len)    │
│  nonce          : [u8;24] = random()                    │
│  ciphertext     : XChaCha20-Poly1305(key, nonce, data)  │
└─────────────────────────────────────────────────────────┘
```

- **Key Derivation:** Argon2id with 64 MiB memory hardness — resistant to GPU/ASIC brute force.
- **Integrity:** SHA-256 header MAC + XChaCha20-Poly1305 AEAD — any tampering detected.
- **Atomic Write:** `vault.enc.tmp.<pid>` → fsync → rename to `vault.enc`. A crash mid-write leaves the old vault intact.
- **Zeroization:** `Redacted<T>` wrapper + `zeroize` on drop — no key material in swap or crash dumps.

---

## 6. Session & Approval Security

### Session Grant Model

```
dApp requests connect()
       │
       ▼
ConnectPromptSheet (user selects accounts + permissions)
       │
       ▼
SessionBroker issues session_token (HMAC-signed, scoped, expirable)
       │
       ▼
Every subsequent call requires session_token + capability check
       │
       ├─ get_accounts: OK (public accounts only)
       ├─ get_balance(Public/<id>): OK
       ├─ get_balance(Private/<id>): requires ReadPrivateBalance
       ├─ propose_transaction: requires decoded summary + explicit approve
       └─ execute_privileged: requires ui_token (wallet UI only)
```

### Approval Sheet Contents

Every `propose_transaction` shows:
- Requesting dApp identity (name, origin, icon)
- Target account and asset
- Decoded call effects (outflows, approvals, authority changes)
- LP-0023 verification badge (`VerifiedSource`, `KnownButUnverified`, `NoRegistryEntry`, `MismatchOrError`)
- Gas/cycle estimate + actual usage after execution
- Critical-red warnings for `SetAuthority` mutations or unlimited approvals

---

## 7. Zero-Telemetry & Network Egress

### Allowed Endpoints (Whitelist)

| Endpoint | Purpose | Required |
| :--- | :--- | :--- |
| `sequencer_url` | LEZ sequencer JSON-RPC | Yes |
| `lez_node_url` | Optional local standalone node | No |
| `logos_storage_url` | Optional Codex backup node | No |

### Forbidden (Blocked by `AllowedEndpointsGuard`)

- Analytics endpoints (Google Analytics, Mixpanel, Sentry, etc.)
- Third-party RPC providers (Infura, Alchemy, etc.)
- Social media APIs (Twitter, Discord, etc.)
- Any URL not in the whitelist → `LZR-4002`

### Verification

`tooling/check-boundaries.sh` scans all crate/module/app manifests and source files for unauthorized `reqwest::`, `fetch(`, or `XMLHttpRequest` calls outside the allowlist.

---

## 8. Encrypted Backup on Logos Storage

```
Client Side:
  vault.enc (SealedVaultEnvelopeV1 ciphertext)
       │
       ▼
  POST /api/storage/v1/data → CID: zDvZRwzm...
       │
       ▼
Logos Storage / Codex Node:
  Stores opaque ciphertext blob (no plaintext, no keys)
       │
       ▼
Client Side (Restore):
  GET /api/storage/v1/data/{cid} → SealedVaultEnvelopeV1
       │
       ▼
  Unseal with password → vault restored
```

**Key Property:** The storage node is **untrusted**. It never sees plaintext, keys, or metadata. The CID is content-addressed (SHA-256 of ciphertext), so tampering is detectable.

---

## 9. Nullifier Journal & Double-Spend Prevention

```
PrivateToPrivate Transfer Flow:
  1. Select unspent notes → compute nullifiers
  2. Write-ahead journal: PendingPrivateSpend { nullifiers, witness, tx_intent }
  3. Prove ZK (Witness → r0vm → RapidSnark → Broadcast)
  4. On success: mark notes spent, clear journal
  5. On crash: recover_pending_private_spends() reconciles journal on restart
  6. On retry: LZR-5004 if any nullifier already in-flight
```

**Guarantee:** A nullifier is committed to the journal **before** proving begins. Even if the process crashes between steps 2 and 4, the journal prevents double-spending on restart.

---

## 10. Prover Pipeline (4-Stage)

```
Witness Generation (lezer-core)
       │
       ▼
r0vm Execution (RISC Zero zkVM)
       │
       ▼
RapidSnark Groth16 Compression
       │
       ▼
Broadcast to Sequencer
       │
       ▼
On-Chain Verification (LEZ state machine)
```

- **Concurrency:** `tokio::sync::Semaphore(1)` serializes all proofs — only one ZK proof in flight at a time.
- **Telemetry:** Each stage emits `ProverStageEvent` with wall-clock time and `getrusage` peak RSS.
- **Deferred Lock:** `AutoLockFsm` sets `DeferredLockPending` during proving; vault locks only after the proof completes.

---

## 11. LP-0023 Source Verification

| Badge | Meaning | Signing Policy |
| :--- | :--- | :--- |
| `ProtocolBuiltin` | Built-in LEZ program (native token) | Normal approval flow |
| `KnownButUnverified` | Program self-registered in `lezer_registry` with a declared source@commit; no reproducible-build check | Explicit acknowledgement required |
| `NoRegistryEntry` | Program not in the LP-0023 registry | Explicit acknowledgement required |
| `VerifiedSource` | Reserved: needs a reproducible-build attestation (not implemented) | Never shown today |
| `MismatchOrError` | Reserved: rebuild contradicts the deployed image | Never shown today |

The `lezer_registry` program keys entries by `caller_account_id` from a chained call, so only the program itself can register its own entry. The declared source URL and commit are still the deployer's own claim.

---

## 12. Audit & Conformance

- **Boundary Checks:** `tooling/check-boundaries.sh` verifies `lezer-core` never imports upstream LEZ crates and `modules/`/`apps/` never import internal vault structs.
- **Golden Fixtures:** `tests/conformance/fixtures/` contains canonical JSON vectors for vault sealing, error taxonomy, shard routing, and transaction summaries.
- **Release Gates:** `RC-01` (dApp connect → approval → testimonial), `RC-02` (private transfers + crash recovery), `RC-03` (50-account concurrent shard overflow).
- **Clean-Room Demo:** `./tooling/demo.sh` runs the full evaluator E2E suite in an unmodified clean environment.

---

## 13. Responsible Disclosure

Security issues should be reported privately via GitHub Security Advisories on [`zLabsHQ/lezer-wallet`](https://github.com/zLabsHQ/lezer-wallet/security).

**Do NOT open public issues for vulnerabilities.** We follow a coordinated disclosure policy aligned with the Logos ecosystem.
