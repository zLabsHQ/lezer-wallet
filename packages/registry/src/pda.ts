/**
 * Client-side deterministic PDA & shard key derivation matching the SPEL guest programs
 * (`lezer_registry` and `lezer_testimonial`).
 */

/** Program IDs are filled from `programs/ARTIFACTS.md` only after a real deployment. */
export const LEZER_REGISTRY_PROGRAM_HEX: string | null = null;

export const LEZER_TESTIMONIAL_PROGRAM_HEX: string | null = null;

/**
 * Pad a literal ASCII seed string to 32 bytes with trailing zeros (SPEL convention).
 */
export function padSeedTo32Bytes(literal: string): Uint8Array {
  const encoded = new TextEncoder().encode(literal);
  if (encoded.length > 32) {
    throw new Error(`PDA literal seed '${literal}' exceeds 32 bytes`);
  }
  const out = new Uint8Array(32);
  out.set(encoded, 0);
  return out;
}

/**
 * Format a shard index (0..15) or page index (0..255) as canonical 2-digit lowercase hex.
 */
export function toHexByte(value: number): string {
  return (value & 0xff).toString(16).padStart(2, '0');
}
