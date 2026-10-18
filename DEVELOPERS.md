# Lezer Wallet — Developer Guide

> **For dApp builders and integrators.** This guide shows how to connect your Basecamp `ui_qml` application to Lezer Wallet using `@lezer/provider` and `LezerProvider.qml`.

---

## 1. Install Lezer Modules

```bash
lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json
lgpd install lezer_wallet_provider lezer_wallet_ui lezer_faucet_ui lezer_testimonial_ui
```

## 2. Declare Dependency in `metadata.json`

```json
{
  "name": "my_dapp",
  "type": "ui_qml",
  "dependencies": ["lezer_wallet_provider"]
}
```

## 3. Connect with `LezerProvider.qml` (QML)

```qml
import QtQuick 2.15
import "LezerProvider.qml" as Lezer

Item {
    Lezer.LezerProvider {
        id: provider
        onConnected: console.log("Connected:", accounts)
        onAccountsChanged: console.log("Accounts changed:", accounts)
    }

    Component.onCompleted: {
        provider.connect({ name: "My dApp", origin: "my-dapp" })
    }

    // Read balance (public account)
    function readBalance() {
        provider.getBalance("Public/<account_id>", "native")
            .then(bal => console.log("Balance:", bal))
            .catch(err => console.error(err))
    }

    // Propose a transaction
    function sendTransfer() {
        provider.proposeTransaction({
            from: "Public/<sender>",
            to: "Public/<recipient>",
            asset: "native",
            amount: "50",
            memo: "Hello LEZ!"
        }).then(tx => console.log("TX hash:", tx.hash))
    }
}
```

## 4. Connect with `@lezer/provider` (TypeScript)

```typescript
import { LezerProvider } from '@lezer/provider';

const provider = new LezerProvider();

// Connect — prompts user for account selection
const { accounts, sessionToken } = await provider.connect({
  name: 'My dApp',
  origin: 'my-dapp',
});

// Read balance
const balance = await provider.getBalance('Public/<account_id>', 'native');

// Propose transaction — user sees decoded summary + approval sheet
const tx = await provider.proposeTransaction({
  from: 'Public/<sender>',
  to: 'Public/<recipient>',
  asset: 'native',
  amount: '50',
});

// Listen for events
provider.on('accountsChanged', (accs) => console.log(accs));
provider.on('chainChanged', (chainId) => console.log(chainId));
```

## 5. LP-0023 Program Verification

```typescript
import { verifyProgram } from '@lezer/registry';

const badge = await verifyProgram('lezer_testimonial', bytecode);
// Returns: 'VerifiedSource' | 'KnownButUnverified' | 'NoRegistryEntry' | 'MismatchOrError'
```

## 6. Testimonial Integration (`submission_id: lp0021-lezer`)

```typescript
import { submitTestimonial } from '@lezer/provider';

await provider.submitTestimonial({
  text: 'Verified Lezer Wallet E2E!',
  username: '@myhandle',
  submissionId: 'lp0021-lezer',
});
```

## 7. Fork the Starter Template

```bash
git clone https://github.com/zLabsHQ/lezer-wallet.git
cp -r lezer-wallet/templates/lezer-dapp-template my-dapp
cd my-dapp
# Edit metadata.json, src/qml/MainView.qml
lgpd install lezer_wallet_provider
```

## 8. Run the Mock Provider (No Wallet Required)

```typescript
import { MockLezerProvider } from '@lezer/provider';

const mock = new MockLezerProvider();
await mock.connect({ name: 'Test', origin: 'test' });
const accounts = await mock.getAccounts();
// Returns deterministic test accounts for CI/unit tests
```

## 9. Error Codes

| Code | Meaning |
| :--- | :--- |
| `LZR-1003` | Invalid password / vault unlock failed |
| `LZR-2003` | Private balance read without `ReadPrivateBalance` permission |
| `LZR-3001` | User rejected the approval prompt |
| `LZR-3003` | Simulation reverted / deterministic revert detected |
| `LZR-3004` | Unacknowledged risk gate (authority change, unlimited approval) |
| `LZR-3005` | Proposal digest mismatch (tampered intent) |
| `LZR-4002` | Disallowed endpoint (zero-telemetry guard) |
| `LZR-5001` | Prover queue busy (`Semaphore(1)`) |
| `LZR-5004` | Nullifier locked in-flight (double-spend prevention) |
| `LZR-6003` | Bytecode mismatch (LP-0023 `MismatchOrError`) |

## 10. CLI Parity

Every GUI feature has a CLI equivalent. Use `lezer --help` or `lezer <command> --help` for details.

```bash
lezer account new --privacy public --label main
lezer token send --from main --to Public/<recipient> --amount 50
lezer testimonial submit --text "Verified!" --username @me
lezer registry lookup --program lezer_testimonial
LEZER_NO_VAULT=1 lezer verify-adoption --submission-id lp0021-lezer
```

## 11. Zero-Telemetry Policy

Lezer Wallet makes **zero** outbound requests to third-party analytics or tracking services. The only allowed endpoints are:
- User's configured LEZ sequencer (`https://testnet.lez.logos.co` or local node)
- Optional Logos Storage/Codex node for encrypted backup

This is enforced by `AllowedEndpointsGuard` in `lezer-adapter` and verified by `tooling/check-boundaries.sh` in CI.
