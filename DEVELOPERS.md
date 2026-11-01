# Lezer Wallet — Developer Guide

> **For dApp builders and integrators.** This guide shows how to connect your Basecamp `ui_qml` application to Lezer Wallet using `LezerProvider.qml` or the `@lezer/provider` TypeScript SDK.

---

## 1. Install Lezer Modules

```bash
lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json
lgpd install lezer_wallet_provider lezer_wallet_ui lezer_testimonial_ui lezer_faucet_ui
```

## 2. Declare Dependency in `metadata.json`

```json
{
  "name": "my_dapp",
  "type": "ui_qml",
  "dependencies": ["lezer_wallet_provider"]
}
```

## 3. Connect & Propose Transactions with `LezerProvider.qml` (QML)

Copy `LezerProvider.qml` (from `templates/lezer-dapp-template/src/qml/LezerProvider.qml`) into your module's `src/qml/` directory. Your module's identity is authenticated by the Logos Basecamp host (`logos_rust_sdk::current_caller()`), so sessions are cryptographically bound to your module.

```qml
import QtQuick
import QtQuick.Controls

Item {
    LezerProvider {
        id: wallet
        displayName: "My LEZ dApp"
        requestPrivateBalanceRead: false

        onConnected: (accounts) => {
            console.log("Authorized accounts:", JSON.stringify(accounts))
        }
        onProposalResolved: (receipt) => {
            console.log("Included in block:", receipt.status.block_id, "tx:", receipt.tx_hash)
        }
        onFailed: (code, message) => {
            console.error("Wallet error:", code, message)
        }
    }

    // 1. Connect — opens ConnectSheet in lezer_wallet_ui for account selection
    function connectWallet() {
        wallet.connect()
    }

    // 2. Read balance of an authorized account
    function readBalance(accountId) {
        wallet.getBalance(accountId, function (bal) {
            console.log("Balance (atomic LEZ):", bal.balance, "source:", bal.source)
        })
    }

    // 3. Propose a native transfer (public, shield, deshield, or private)
    function sendNative(fromAccount, toAccountOrReceiveUri, atomicAmount) {
        wallet.propose({
            kind: "native_transfer",
            from: fromAccount,
            to: toAccountOrReceiveUri,
            amount: String(atomicAmount)
        })
    }

    // 4. Propose a fungible token transfer (`lezer_token` SPEL program)
    function sendToken(tokenId, fromAccount, toAccount, rawAmount) {
        wallet.propose({
            kind: "token_transfer",
            token_id: tokenId,
            from: fromAccount,
            to: toAccount,
            amount: String(rawAmount)
        })
    }

    // 5. Claim 50 LEZ from the on-chain faucet (`lezer_faucet` SPEL program)
    function claimFaucet(recipientAccount) {
        wallet.propose({
            kind: "faucet_claim",
            recipient: recipientAccount
        })
    }

    // 6. Publish an LP-0021 on-chain testimonial (`lezer_testimonial` SPEL program)
    function publishTestimonial(authorAccount, username, text) {
        wallet.propose({
            kind: "submit_testimonial",
            author: authorAccount,
            username: username,
            text: text
        })
    }
}
```

## 4. Connect with `@lezer/provider` (TypeScript)

```typescript
import { LezerProvider } from '@lezer/provider';

const provider = new LezerProvider();

// Connect — prompts user for account selection in lezer_wallet_ui
const accounts = await provider.connect({
  displayName: 'My dApp',
  requestPrivateBalanceRead: false,
});

// Read balance of a shared account
const balance = await provider.getBalance(accounts[0].accountId);

// Propose a native transfer — user reviews the decoded summary + LP-0023 badge in ApprovalSheet
const receipt = await provider.proposeTransaction({
  kind: 'native_transfer',
  from: accounts[0].accountId,
  to: 'Public/<recipient>',
  amount: '1000000000', // 1 LEZ in atomic units (10^9)
});

// Propose a fungible token transfer
const tokenReceipt = await provider.proposeTransaction({
  kind: 'token_transfer',
  token_id: 'Public/<token_definition_account>',
  from: accounts[0].accountId,
  to: 'Public/<recipient>',
  amount: '2500000',
});
```

## 5. Worked Reference Mini-Apps (`modules/`)

1. **`modules/lezer_testimonial_ui` (`TestimonialView.qml`)**:
   - Connects via `LezerProvider.qml`, filters shared `Public/` accounts, and proposes `{ kind: "submit_testimonial", author, username, text }`.
   - `lezer-core` reads the program's `lezer_registry` entry on-chain and shows its registered name, version and declared source@commit (badge `known_but_unverified`; `no_registry_entry` if unregistered), plus an amber privacy notice (`WARN_PUBLIC_ONCHAIN_DATA`), before the user approves.
2. **`modules/lezer_faucet_ui` (`FaucetView.qml`)**:
   - Connects via `LezerProvider.qml`, lets the user select which shared `Public/` account receives 50 LEZ, and proposes `{ kind: "faucet_claim", recipient }`.
   - `lezer-core` pre-checks the recipient's on-chain claim-ticket PDA and the faucet treasury PDA (`custody_transfer`) so rate-limited accounts receive an immediate error instead of a charged revert.

## 6. Fork the Starter Template

```bash
git clone https://github.com/zLabsHQ/lezer-wallet.git
cp -r lezer-wallet/templates/lezer-dapp-template my-dapp
cd my-dapp
# Edit metadata.json and src/qml/MainView.qml
```

## 7. Run the Mock Provider (Unit Tests & Playground)

```typescript
import { MockLezerProvider } from '@lezer/provider';

const mock = new MockLezerProvider();
const accounts = await mock.connect({ displayName: 'Test dApp' });
const receipt = await mock.proposeTransaction({
  kind: 'native_transfer',
  from: accounts[0].accountId,
  to: accounts[0].accountId,
  amount: '500',
});
```

## 8. Error Codes

| Code | Meaning |
| :--- | :--- |
| `LZR-1003` | Invalid password / vault unlock failed |
| `LZR-2001` | Session expired or invalid |
| `LZR-2003` | Private balance read without `ReadPrivateBalance` permission |
| `LZR-2004` | Privileged `wallet_call` invoked by a non-wallet module |
| `LZR-3001` | User rejected the approval prompt |
| `LZR-3003` | Precheck / insufficient balance |
| `LZR-3004` | Unacknowledged risk gate (unverified program or critical authority change) |
| `LZR-3005` | Proposal digest mismatch (tampered summary) |
| `LZR-3006` | Transaction rejected by local pre-check or sequencer |
| `LZR-3007` | Target program is not deployed on the configured network |
| `LZR-4002` | Disallowed endpoint blocked (zero-telemetry guard) |
| `LZR-5001` | Wallet engine busy with an in-flight operation |

## 9. CLI Reference (`lezer`)

Every GUI operation has a CLI counterpart (`--json` supported on every command):

```bash
lezer init --network https://testnet.lez.logos.co
lezer account new public --label alice
lezer account new private --label shielded
lezer account receive shielded
lezer send --from main --to alice --amount 1000000000
lezer program deploy programs/faucet/target/riscv32im-risc0-zkvm-elf/docker/lezer_faucet.bin --payer main
lezer faucet claim --recipient alice
lezer token define --definition tok-def --name "USD Coin" --symbol USDX --decimals 6 --supply 1000000 --authority main --payer main
lezer testimonial submit --author main --username "@alice" --text "Lezer works!"
lezer verify-adoption
lezer registry register --subject lezer_testimonial --name "Lezer Testimonial" --source-url "https://github.com/zLabsHQ/lezer-wallet" --commit "v0.3.1" --version "0.3.1" --payer main
lezer registry list
```

## 10. Zero-Telemetry Policy

Lezer Wallet makes **zero** outbound requests to third-party analytics or tracking services. The only allowed endpoints are:
- User's configured LEZ sequencer (`https://testnet.lez.logos.co` or local node)
- Optional Logos Storage/Codex node for encrypted backup

Enforced by `AllowedEndpointsGuard` in `lezer-adapter` and verified by `tooling/check-boundaries.sh` in CI.
