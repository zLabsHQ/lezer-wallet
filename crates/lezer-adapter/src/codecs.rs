//! Borsh & RISC Zero `u32`-word instruction serializers and deserializers
//! (`crates/lezer-adapter/src/codecs.rs`).
//!
//! Converts between raw byte slices, RISC Zero little-endian `u32` words, and
//! 64-character hex Program IDs (`[u32; 8]`).

use anyhow::{Context, Result, bail};

/// Encode an arbitrary byte slice into padded little-endian `u32` words for RISC Zero guest input.
pub fn bytes_to_risc0_u32_words(bytes: &[u8]) -> Vec<u32> {
    let mut padded = Vec::with_capacity(bytes.len().div_ceil(4) * 4);
    padded.extend_from_slice(bytes);
    while !padded.len().is_multiple_of(4) {
        padded.push(0);
    }
    padded
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("4-byte chunk")))
        .collect()
}

/// Convert a slice of RISC Zero little-endian `u32` words back into raw bytes.
pub fn risc0_u32_words_to_bytes(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len() * 4);
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}

/// Parse a 64-character hex RISC0 Image ID into its `[u32; 8]` wire representation.
pub fn program_id_words_from_hex(hex_str: &str) -> Result<[u32; 8]> {
    let trimmed = hex_str.trim();
    let bytes = hex::decode(trimmed).context("invalid hex string for program_id")?;
    if bytes.len() != 32 {
        bail!(
            "program_id hex must decode to 32 bytes (8 u32 words), got {}",
            bytes.len()
        );
    }
    let mut words = [0u32; 8];
    for (i, chunk) in bytes.chunks_exact(4).enumerate() {
        words[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    Ok(words)
}

/// Render a `[u32; 8]` Program ID into its canonical 64-character lowercase hex string.
pub fn program_id_words_to_hex(words: &[u32; 8]) -> String {
    let mut bytes = [0u8; 32];
    for (i, w) in words.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risc0_words_and_program_id_hex_roundtrip() {
        let original = b"lezer-risc0-word-codec";
        let words = bytes_to_risc0_u32_words(original);
        let decoded = risc0_u32_words_to_bytes(&words);
        assert_eq!(&decoded[..original.len()], original);

        let hex_id = "be6cd0c3c15e6d6236cadf3af0eb63332473f223c0713256f7a5b83e0fac2cf5";
        let parsed = program_id_words_from_hex(hex_id).unwrap();
        assert_eq!(program_id_words_to_hex(&parsed), hex_id);
    }
}
