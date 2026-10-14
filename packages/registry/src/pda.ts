/**
 * Client-side deterministic PDA & shard key derivation matching the SPEL guest programs
 * (`lezer_registry` and `lezer_testimonial`).
 */

export const LEZER_REGISTRY_PROGRAM_HEX =
  'be6cd0c3c15e6d6236cadf3af0eb63332473f223c0713256f7a5b83e0fac2cf5';

export const LEZER_TESTIMONIAL_PROGRAM_HEX =
  '9aa87b6b4c3d2e1f0a9b8c7d6e5f4a3b2c1d0e9f8a7b6c5d4e3f2a1b0c9dfb9f';

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
