//! Public native LEZ transfer (`Public/<from> -> Public/<to>`) for `lezer-adapter`.
//!
//! Constructs, signs with the sender's deterministic Ed25519 keypair, and broadcasts
//! (or locally verifies when running against a simulated/unfunded test environment)
//! an authenticated public native transfer on LEZ.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::accounts::{derive_public_account, derive_public_signing_material, parse_public_account_id};
use crate::rpc::LezRpcClient;

/// Receipt produced by executing a public native LEZ transfer (`Public -> Public`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicTransferReceipt {
    /// Deterministic 64-hex transaction hash (`SHA-256` over signed wire payload).
    pub tx_hash: String,
    /// Canonical sender (`"Public/<base58>"`).
    pub from_account: String,
    /// Canonical recipient (`"Public/<base58>"`).
    pub to_account: String,
    /// Transferred native LEZ amount.
    pub amount: u128,
    /// Sender nonce consumed by this transaction.
    pub nonce: u128,
    /// Hex-encoded 64-byte Ed25519 signature over the canonical transfer message.
    pub signature_hex: String,
    /// Whether the transaction was accepted by the live sequencer RPC or recorded in local/simulated mode.
    pub broadcast_status: String,
}

/// Execute a public native LEZ transfer (`Public -> Public`).
///
/// 1. Validates sender and recipient `Public/<base58>` identifiers and ensures `amount > 0`.
/// 2. Queries the sender's current on-chain balance and nonce (when reachable).
/// 3. Constructs the canonical authenticated-transfer message and signs it with the
///    sender's derived Ed25519 signing key (`m/44'/9999'/0'/0/<derivation_index>`).
/// 4. Submits the signed transaction to the sequencer RPC (or completes deterministically
///    when `LEZER_SIMULATE_BROADCAST=1` is set in unit/CLI tests).
pub async fn send_public_native_transfer(
    rpc: &LezRpcClient,
    mnemonic_phrase: &str,
    from_derivation_index: u32,
    from_account: &str,
    to_account: &str,
    amount: u128,
) -> Result<PublicTransferReceipt> {
    if amount == 0 {
        bail!("transfer amount must be greater than zero");
    }

    let (from_prefixed, from_bare) = parse_public_account_id(from_account)?;
    let (to_prefixed, to_bare) = parse_public_account_id(to_account)?;

    let derived = derive_public_account(mnemonic_phrase, from_derivation_index);
    if derived.account_id != from_prefixed {
        bail!(
            "derived account '{}' at index {} does not match sender '{}'",
            derived.account_id,
            from_derivation_index,
            from_prefixed
        );
    }

    let signing_mat = derive_public_signing_material(mnemonic_phrase, from_derivation_index);

    // Query live nonce & balance unless running in offline/simulated broadcast mode
    let simulate_only = std::env::var("LEZER_SIMULATE_BROADCAST").as_deref() == Ok("1");
    let nonce = if simulate_only {
        1
    } else {
        let sender_state = rpc.get_account_state(&from_bare).await?;
        if sender_state.balance < amount {
            bail!(
                "LZR-3003: SimulationReverted — insufficient balance on {} (have {} LEZ, tried to send {} LEZ)",
                from_prefixed,
                sender_state.balance,
                amount
            );
        }
        sender_state.nonce + 1
    };

    // Build canonical transfer message & sign
    let mut msg = Vec::with_capacity(128);
    msg.extend_from_slice(b"lez-authenticated-transfer-v1:");
    msg.extend_from_slice(from_bare.as_bytes());
    msg.extend_from_slice(b"->");
    msg.extend_from_slice(to_bare.as_bytes());
    msg.extend_from_slice(&amount.to_le_bytes());
    msg.extend_from_slice(&nonce.to_le_bytes());

    let signature = signing_mat.sign(&msg);
    let signature_hex = hex::encode(signature);

    let mut tx_hasher = Sha256::new();
    tx_hasher.update(&msg);
    tx_hasher.update(signature);
    let tx_hash = hex::encode(tx_hasher.finalize());

    let broadcast_status = if simulate_only {
        "simulated_confirmed".to_string()
    } else {
        match rpc
            .rpc_call(
                "sendTransaction",
                json!({
                    "from": from_bare,
                    "to": to_bare,
                    "amount": amount.to_string(),
                    "nonce": nonce,
                    "signature": signature_hex,
                }),
            )
            .await
        {
            Ok(_) => "submitted".to_string(),
            Err(e) => {
                return Err(e);
            }
        }
    };

    Ok(PublicTransferReceipt {
        tx_hash,
        from_account: from_prefixed,
        to_account: to_prefixed,
        amount,
        nonce,
        signature_hex,
        broadcast_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::NetworkProfile;

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[tokio::test]
    async fn public_transfer_signs_and_produces_deterministic_tx_hash() {
        unsafe {
            std::env::set_var("LEZER_SIMULATE_BROADCAST", "1");
        }
        let rpc = LezRpcClient::new(NetworkProfile::testnet_default()).unwrap();
        let alice = derive_public_account(TEST_MNEMONIC, 0);
        let bob = derive_public_account(TEST_MNEMONIC, 1);

        let receipt = send_public_native_transfer(
            &rpc,
            TEST_MNEMONIC,
            0,
            &alice.account_id,
            &bob.account_id,
            50,
        )
        .await
        .unwrap();

        assert_eq!(receipt.from_account, alice.account_id);
        assert_eq!(receipt.to_account, bob.account_id);
        assert_eq!(receipt.amount, 50);
        assert_eq!(receipt.tx_hash.len(), 64);
        assert_eq!(receipt.signature_hex.len(), 128);
    }
}
