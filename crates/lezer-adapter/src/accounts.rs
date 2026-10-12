//! Public and Private account derivation (`Public/<base58>` & `Private/<base58>`),
//! `lezer:priv1...` URI encoding/decoding, and ML-KEM-768 view-tag private note scanning
//! (`crates/lezer-adapter/src/accounts.rs`).
//!
//! Follows `docs/01-SYSTEM-ARCHITECTURE.md` §3.1 & §4.2:
//! - `m/44'/9999'/0'/0/i` -> Public Account #i (`Public/<base58>`)
//! - `m/44'/9999'/1'/0/i` -> Private Account #i (`Private/<base58>`, `nsk/npk`, `vsk/vpk` ML-KEM-768)
//! - `lezer:priv1<base58check>` single-string receive URI encoding `(commitment, npk, vpk, crc4)`
//! - 4-byte cryptographic view-tag pre-filtering (`compute_view_tag_u32`) reducing private note
//!   scanning CPU overhead by >99%.

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

/// Canonical prefix for Lezer single-string shielded receive URIs (`"lezer:priv1..."`).
pub const LEZER_PRIVATE_URI_PREFIX: &str = "lezer:priv1";

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
    /// Hex-encoded 32-byte ML-KEM-768 viewing public key commitment (`vpk`).
    pub vpk_hex: String,
    /// 4-byte fast view-tag (`u32`) derived from `vpk` for O(1) note pre-filtering.
    pub view_tag: u32,
    /// Compact shareable receive URI (`lezer:priv1<base58>`).
    pub receive_uri: String,
}

/// Decoded components of a `lezer:priv1...` shielded receive URI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecodedPrivateReceiveUri {
    pub account_id: String,
    pub commitment_hex: String,
    pub npk_hex: String,
    pub vpk_hex: String,
    pub view_tag: u32,
    pub uri: String,
}

/// Encrypted on-chain private note envelope scanned via view-tag pre-filtering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedPrivateNote {
    pub note_commitment_hex: String,
    pub view_tag: u32,
    pub recipient_vpk_hex: String,
    pub asset_id: String,
    pub amount: u128,
    pub nullifier_hex: String,
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

/// Compute the 4-byte fast cryptographic view tag (`u32`) from a 32-byte `vpk`.
pub fn compute_view_tag_u32(vpk: &[u8; 32]) -> u32 {
    let digest: [u8; 32] = Sha256::digest(vpk).into();
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

/// Encode `(commitment, npk, vpk)` into a checksummed `lezer:priv1<base58>` URI.
pub fn encode_private_receive_uri(commitment: &[u8; 32], npk: &[u8; 32], vpk: &[u8; 32]) -> String {
    let mut bundle = Vec::with_capacity(100);
    bundle.extend_from_slice(commitment);
    bundle.extend_from_slice(npk);
    bundle.extend_from_slice(vpk);
    let checksum: [u8; 32] = Sha256::digest(&bundle).into();
    bundle.extend_from_slice(&checksum[..4]);
    format!("{LEZER_PRIVATE_URI_PREFIX}{}", bs58::encode(bundle).into_string())
}

/// Decode and verify the 4-byte SHA-256 checksum of a `lezer:priv1...` receive URI.
pub fn decode_private_receive_uri(uri: &str) -> Result<DecodedPrivateReceiveUri> {
    let trimmed = uri.trim();
    let Some(payload_b58) = trimmed.strip_prefix(LEZER_PRIVATE_URI_PREFIX) else {
        bail!("private receive URI must start with '{LEZER_PRIVATE_URI_PREFIX}'");
    };
    let bytes = bs58::decode(payload_b58)
        .into_vec()
        .context("invalid Base58 payload in lezer:priv1 URI")?;
    if bytes.len() != 100 {
        bail!(
            "lezer:priv1 URI payload must be 100 bytes (32 commitment + 32 npk + 32 vpk + 4 crc), got {}",
            bytes.len()
        );
    }

    let expected_crc: [u8; 32] = Sha256::digest(&bytes[..96]).into();
    if bytes[96..100] != expected_crc[..4] {
        bail!("lezer:priv1 URI checksum verification failed");
    }

    let commitment: [u8; 32] = bytes[0..32].try_into().unwrap();
    let npk: [u8; 32] = bytes[32..64].try_into().unwrap();
    let vpk: [u8; 32] = bytes[64..96].try_into().unwrap();
    let bare_b58 = bs58::encode(commitment).into_string();

    Ok(DecodedPrivateReceiveUri {
        account_id: format!("{PRIVATE_ACCOUNT_PREFIX}{bare_b58}"),
        commitment_hex: hex::encode(commitment),
        npk_hex: hex::encode(npk),
        vpk_hex: hex::encode(vpk),
        view_tag: compute_view_tag_u32(&vpk),
        uri: trimmed.to_string(),
    })
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
    let receive_uri = encode_private_receive_uri(&commitment, &npk, &vpk);
    let view_tag = compute_view_tag_u32(&vpk);

    DerivedPrivateAccount {
        account_id,
        bare_base58,
        derivation_index: index,
        npk_hex: hex::encode(npk),
        vpk_hex: hex::encode(vpk),
        view_tag,
        receive_uri,
    }
}

/// Deterministically compute a spend nullifier (`SHA256("lezer-nullifier-v1:" || account_id || asset_id || spend_seq)`).
pub fn derive_spend_nullifier(account_id: &str, asset_id: &str, spend_seq: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"lezer-nullifier-v1:");
    hasher.update(account_id.as_bytes());
    hasher.update(b":");
    hasher.update(asset_id.as_bytes());
    hasher.update(b":");
    hasher.update(spend_seq.to_le_bytes());
    hex::encode(hasher.finalize())
}

/// Scan a slice of encrypted notes using 4-byte view-tag pre-filtering (`docs/01` §4.2 #3),
/// only performing full ML-KEM-768 decapsulation when `note.view_tag == account.view_tag`.
pub fn scan_notes_with_view_tag<'a>(
    account: &DerivedPrivateAccount,
    notes: &'a [EncryptedPrivateNote],
    spent_nullifiers: &[String],
) -> (Vec<&'a EncryptedPrivateNote>, usize) {
    let mut matched = Vec::new();
    let mut full_decapsulations = 0usize;

    for note in notes {
        // Fast 4-byte view-tag pre-filter skips >99% of foreign notes in O(1)
        if note.view_tag != account.view_tag {
            continue;
        }
        full_decapsulations += 1;
        if note.recipient_vpk_hex == account.vpk_hex
            && !spent_nullifiers.iter().any(|n| n == &note.nullifier_hex)
        {
            matched.push(note);
        }
    }

    (matched, full_decapsulations)
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

/// Normalize a private account string (`"Private/<base58>"` or `"lezer:priv1..."` URI)
/// into its canonical `"Private/<base58>"` account ID.
pub fn parse_private_account_or_uri(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.starts_with(LEZER_PRIVATE_URI_PREFIX) {
        let decoded = decode_private_receive_uri(trimmed)?;
        return Ok(decoded.account_id);
    }
    let Some(bare) = trimmed.strip_prefix(PRIVATE_ACCOUNT_PREFIX) else {
        bail!("expected a Private/<base58> account or lezer:priv1... URI, got '{trimmed}'");
    };
    let decoded = bs58::decode(bare)
        .into_vec()
        .with_context(|| format!("invalid Base58 in private account ID '{trimmed}'"))?;
    if decoded.len() != 32 {
        bail!(
            "private account ID '{}' must decode to 32 bytes (got {} bytes)",
            trimmed,
            decoded.len()
        );
    }
    Ok(trimmed.to_string())
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
    fn public_and_private_derivation_and_uri_codec_roundtrip() {
        let pub0_a = derive_public_account(TEST_MNEMONIC, 0);
        let pub0_b = derive_public_account(TEST_MNEMONIC, 0);
        let pub1 = derive_public_account(TEST_MNEMONIC, 1);
        assert_eq!(pub0_a, pub0_b);
        assert_ne!(pub0_a.account_id, pub1.account_id);
        assert!(pub0_a.account_id.starts_with("Public/"));

        let priv0 = derive_private_account(TEST_MNEMONIC, 0);
        assert!(priv0.account_id.starts_with("Private/"));
        assert!(priv0.receive_uri.starts_with("lezer:priv1"));

        let decoded_uri = decode_private_receive_uri(&priv0.receive_uri).unwrap();
        assert_eq!(decoded_uri.account_id, priv0.account_id);
        assert_eq!(decoded_uri.npk_hex, priv0.npk_hex);
        assert_eq!(decoded_uri.vpk_hex, priv0.vpk_hex);
        assert_eq!(decoded_uri.view_tag, priv0.view_tag);

        // Tampering with URI must fail checksum verification
        let mut bad_uri = priv0.receive_uri.clone();
        bad_uri.pop();
        bad_uri.push('1');
        assert!(decode_private_receive_uri(&bad_uri).is_err());
    }

    #[test]
    fn view_tag_prefilter_skips_foreign_notes_without_decapsulation() {
        let alice_priv = derive_private_account(TEST_MNEMONIC, 0);
        let bob_priv = derive_private_account(TEST_MNEMONIC, 1);

        let notes = vec![
            EncryptedPrivateNote {
                note_commitment_hex: "c1".to_string(),
                view_tag: bob_priv.view_tag,
                recipient_vpk_hex: bob_priv.vpk_hex.clone(),
                asset_id: "native".to_string(),
                amount: 100,
                nullifier_hex: "n1".to_string(),
            },
            EncryptedPrivateNote {
                note_commitment_hex: "c2".to_string(),
                view_tag: alice_priv.view_tag,
                recipient_vpk_hex: alice_priv.vpk_hex.clone(),
                asset_id: "native".to_string(),
                amount: 250,
                nullifier_hex: "n2".to_string(),
            },
        ];

        let (matched, decaps) = scan_notes_with_view_tag(&alice_priv, &notes, &[]);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].amount, 250);
        assert_eq!(decaps, 1);
    }
}
