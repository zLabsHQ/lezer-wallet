//! `lezer-wasm` — Gear 1 Pure-Browser Public Key Derivation, Ed25519 Signing,
//! and RISC0 `u32`-Word Transaction Builder (`docs/07` §3.2).
//!
//! Enables the Lezer Browser Extension (`apps/lezer-extension`) to derive `Public/<base58>`
//! accounts, sign public transfers, and submit SPEL program calls (`submit_testimonial`,
//! `TOKEN_ID`, `ASSOCIATED_TOKEN_ACCOUNT_ID`) with zero native binary installation.

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroize;

/// Public account derived inside the pure-WASM signing engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmPublicAccount {
    pub account_id: String,
    pub public_key_hex: String,
    pub derivation_index: u32,
}

/// Signed public transaction envelope produced by Gear 1 (`lezer-wasm`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmSignedPublicTx {
    pub tx_hash: String,
    pub from_account: String,
    pub to_account: String,
    pub amount: String,
    pub nonce: u128,
    pub signature_hex: String,
    pub gear: String,
}

fn derive_seed(mnemonic: &str, index: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"lezer-slip10-v1:m/44'/9999'/");
    hasher.update(0u32.to_le_bytes());
    hasher.update(b"'/0/");
    hasher.update(index.to_le_bytes());
    hasher.update(b":");
    hasher.update(mnemonic.trim().as_bytes());
    hasher.finalize().into()
}

/// Derive a `Public/<base58>` account deterministically from `mnemonic` and `index`.
pub fn wasm_derive_public_account(mnemonic: &str, index: u32) -> WasmPublicAccount {
    let mut secret = derive_seed(mnemonic, index);
    let signing_key = SigningKey::from_bytes(&secret);
    secret.zeroize();
    let vk_bytes = signing_key.verifying_key().to_bytes();
    let id_bytes: [u8; 32] = Sha256::digest(vk_bytes).into();
    let account_id = format!("Public/{}", bs58::encode(id_bytes).into_string());

    WasmPublicAccount {
        account_id,
        public_key_hex: hex::encode(vk_bytes),
        derivation_index: index,
    }
}

/// Sign a public native LEZ transfer in pure WASM (`Gear 1: zero-install browser signing`).
pub fn wasm_sign_public_transfer(
    mnemonic: &str,
    index: u32,
    to_account: &str,
    amount: u128,
    nonce: u128,
) -> WasmSignedPublicTx {
    let from = wasm_derive_public_account(mnemonic, index);
    let mut secret = derive_seed(mnemonic, index);
    let signing_key = SigningKey::from_bytes(&secret);
    secret.zeroize();

    let from_bare = from
        .account_id
        .strip_prefix("Public/")
        .unwrap_or(&from.account_id);
    let to_bare = to_account.trim().strip_prefix("Public/").unwrap_or(to_account.trim());

    let mut msg = Vec::with_capacity(128);
    msg.extend_from_slice(b"lez-authenticated-transfer-v1:");
    msg.extend_from_slice(from_bare.as_bytes());
    msg.extend_from_slice(b"->");
    msg.extend_from_slice(to_bare.as_bytes());
    msg.extend_from_slice(&amount.to_le_bytes());
    msg.extend_from_slice(&nonce.to_le_bytes());

    let sig = signing_key.sign(&msg).to_bytes();
    let signature_hex = hex::encode(sig);

    let mut tx_hasher = Sha256::new();
    tx_hasher.update(&msg);
    tx_hasher.update(sig);
    let tx_hash = hex::encode(tx_hasher.finalize());

    WasmSignedPublicTx {
        tx_hash,
        from_account: from.account_id,
        to_account: format!("Public/{to_bare}"),
        amount: amount.to_string(),
        nonce,
        signature_hex,
        gear: "gear1_pure_wasm_ed25519".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn wasm_derives_identical_public_account_and_signs_transfer() {
        let acct = wasm_derive_public_account(TEST_MNEMONIC, 0);
        assert_eq!(
            acct.account_id,
            "Public/BH2zpUcwfB6cTKjYp2N8FFQNYSxjighmDfMYveFCC1Wf"
        );

        let signed = wasm_sign_public_transfer(
            TEST_MNEMONIC,
            0,
            "Public/iVnXw8NmAXxEAgwAmxpnbD4V2ZRVBsYEJrFWQngfyEX",
            50,
            1,
        );
        assert_eq!(signed.tx_hash.len(), 64);
        assert_eq!(signed.signature_hex.len(), 128);
        assert_eq!(signed.gear, "gear1_pure_wasm_ed25519");
    }
}
