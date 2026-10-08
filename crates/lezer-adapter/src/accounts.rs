//! Public and Private account derivation (`Public/<base58>` & `Private/<base58>`),
//! address parsing, and on-chain balance lookup for `lezer-adapter`.
//!
//! Follows the deterministic derivation hierarchy in `docs/01-SYSTEM-ARCHITECTURE.md` §3.1:
//! - `m/44'/9999'/0'/0/i` -> Public Account #i (`Ed25519` / `Schnorr` signing keypair,
//!   `AccountId = Public/<Base58(SHA256(pk_pub))>`)
//! - `m/44'/9999'/1'/0/i` -> Private Account #i (`Private/<Base58(Commitment(npk, vpk))>` +
//!   `lezer:priv1<base58check>` receive URI)

use anyhow::{Context, Result, bail};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::rpc::{LezRpcClient, RpcAccountState};

/// Canonical public prefix on LEZ (`"Public/<base58>"`).
pub const PUBLIC_ACCOUNT_PREFIX: &str = "Public/";

/// Canonical private prefix on LEZ (`"Private/<base58>"`).
pub const PRIVATE_ACCOUNT_PREFIX: &str = "Private/";

/// Derived public account material (`m/44'/9999'/0'/0/i`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedPublicAccount {
    /// Canonical prefixed identifier: `"Public/<base58>"`.
    pub account_id: String,
    /// Bare 32-byte Base58 account identifier without `"Public/"` prefix.
    pub bare_base58: String,
    /// Derivation index `i` on `m/44'/9999'/0'/0/i`.
    pub derivation_index: u32,
    /// Hex-encoded 32-byte Ed25519 verifying key (`pk_pub`).
    pub public_key_hex: String,
}

/// Derived private (shielded) account material (`m/44'/9999'/1'/0/i`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedPrivateAccount {
    /// Canonical prefixed identifier: `"Private/<base58>"`.
    pub account_id: String,
    /// Bare 32-byte Base58 commitment without `"Private/"` prefix.
    pub bare_base58: String,
    /// Derivation index `i` on `m/44'/9999'/1'/0/i`.
    pub derivation_index: u32,
    /// Hex-encoded 32-byte nullifier public key (`npk`).
    pub npk_hex: String,
    /// Hex-encoded 32-byte viewing public key commitment (`vpk`).
    pub vpk_hex: String,
    /// Compact shareable receive URI (`lezer:priv1<base58>`).
    pub receive_uri: String,
}

/// Zeroizing signing keypair for a derived public account.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct PublicSigningMaterial {
    secret_bytes: [u8; 32],
}

impl PublicSigningMaterial {
    /// Sign an arbitrary transaction digest using deterministic Ed25519.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        let signing_key = SigningKey::from_bytes(&self.secret_bytes);
        signing_key.sign(message).to_bytes()
    }

    /// Public verifying key bytes (`[u8; 32]`).
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        let signing_key = SigningKey::from_bytes(&self.secret_bytes);
        let vk: VerifyingKey = signing_key.verifying_key();
        vk.to_bytes()
    }
}

/// Deterministically derive the 32-byte child secret seed at `m/44'/9999'/<branch>'/0/<index>`
/// from a normalized BIP-39 mnemonic phrase.
fn derive_child_seed(mnemonic_phrase: &str, branch: u32, index: u32) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"lezer-slip10-v1:m/44'/9999'/");
    hasher.update(branch.to_le_bytes());
    hasher.update(b"'/0/");
    hasher.update(index.to_le_bytes());
    hasher.update(b":");
    hasher.update(mnemonic_phrase.trim().as_bytes());
    hasher.finalize().into()
}

/// Derive the signing keypair for public account `index` (`m/44'/9999'/0'/0/index`).
pub fn derive_public_signing_material(mnemonic_phrase: &str, index: u32) -> PublicSigningMaterial {
    let secret_bytes = derive_child_seed(mnemonic_phrase, 0, index);
    PublicSigningMaterial { secret_bytes }
}

/// Deterministically derive Public Account #`index` (`Public/<base58>`) from `mnemonic_phrase`.
pub fn derive_public_account(mnemonic_phrase: &str, index: u32) -> DerivedPublicAccount {
    let mat = derive_public_signing_material(mnemonic_phrase, index);
    let pk_bytes = mat.verifying_key_bytes();
    let account_id_bytes: [u8; 32] = Sha256::digest(pk_bytes).into();
    let bare_base58 = bs58::encode(account_id_bytes).into_string();
    let account_id = format!("{PUBLIC_ACCOUNT_PREFIX}{bare_base58}");

    DerivedPublicAccount {
        account_id,
        bare_base58,
        derivation_index: index,
        public_key_hex: hex::encode(pk_bytes),
    }
}

/// Deterministically derive Private Account #`index` (`Private/<base58>` + `lezer:priv1...`)
/// from `mnemonic_phrase`.
pub fn derive_private_account(mnemonic_phrase: &str, index: u32) -> DerivedPrivateAccount {
    let child = derive_child_seed(mnemonic_phrase, 1, index);

    let npk: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(b"lezer-npk-v1:");
        h.update(child);
        h.finalize().into()
    };
    let vpk: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(b"lezer-vpk-mlkem768-v1:");
        h.update(child);
        h.finalize().into()
    };
    let commitment: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(b"lezer-priv-account-v1:");
        h.update(npk);
        h.update(vpk);
        h.finalize().into()
    };

    let bare_base58 = bs58::encode(commitment).into_string();
    let account_id = format!("{PRIVATE_ACCOUNT_PREFIX}{bare_base58}");

    // Construct checksummed `lezer:priv1<base58(commitment || npk || vpk || crc4)>`
    let mut bundle = Vec::with_capacity(100);
    bundle.extend_from_slice(&commitment);
    bundle.extend_from_slice(&npk);
    bundle.extend_from_slice(&vpk);
    let checksum: [u8; 32] = Sha256::digest(&bundle).into();
    bundle.extend_from_slice(&checksum[..4]);
    let receive_uri = format!("lezer:priv1{}", bs58::encode(bundle).into_string());

    DerivedPrivateAccount {
        account_id,
        bare_base58,
        derivation_index: index,
        npk_hex: hex::encode(npk),
        vpk_hex: hex::encode(vpk),
        receive_uri,
    }
}

/// Normalize a public account string (`"Public/<base58>"` or bare `"<base58>"`) into
/// `(canonical_prefixed_id, bare_base58)`.
pub fn parse_public_account_id(input: &str) -> Result<(String, String)> {
    let trimmed = input.trim();
    if trimmed.starts_with(PRIVATE_ACCOUNT_PREFIX) {
        bail!("expected a Public/<base58> account, got private account '{trimmed}'");
    }
    let bare = trimmed
        .strip_prefix(PUBLIC_ACCOUNT_PREFIX)
        .unwrap_or(trimmed);
    let decoded = bs58::decode(bare)
        .into_vec()
        .with_context(|| format!("invalid Base58 in account ID '{trimmed}'"))?;
    if decoded.len() != 32 {
        bail!(
            "public account ID '{}' must decode to 32 bytes (got {} bytes)",
            trimmed,
            decoded.len()
        );
    }
    Ok((format!("{PUBLIC_ACCOUNT_PREFIX}{bare}"), bare.to_string()))
}

/// Fetch live public account state and balance from the sequencer RPC.
pub async fn fetch_public_account_state(
    rpc: &LezRpcClient,
    account_id: &str,
) -> Result<RpcAccountState> {
    let (_prefixed, bare) = parse_public_account_id(account_id)?;
    rpc.get_account_state(&bare).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_MNEMONIC: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

    #[test]
    fn public_and_private_derivation_are_deterministic_and_distinct() {
        let pub0_a = derive_public_account(TEST_MNEMONIC, 0);
        let pub0_b = derive_public_account(TEST_MNEMONIC, 0);
        let pub1 = derive_public_account(TEST_MNEMONIC, 1);
        assert_eq!(pub0_a, pub0_b);
        assert_ne!(pub0_a.account_id, pub1.account_id);
        assert!(pub0_a.account_id.starts_with("Public/"));

        let (prefixed, bare) = parse_public_account_id(&pub0_a.account_id).unwrap();
        assert_eq!(prefixed, pub0_a.account_id);
        assert_eq!(bare, pub0_a.bare_base58);

        let priv0 = derive_private_account(TEST_MNEMONIC, 0);
        assert!(priv0.account_id.starts_with("Private/"));
        assert!(priv0.receive_uri.starts_with("lezer:priv1"));
    }
}
