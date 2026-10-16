/**
 * 2-of-3 Shamir Secret Sharing over GF(256) (`docs/07` §4.1 & `docs/01` §5.2):
 * Splits a 32-byte Vault Master Key into 3 shares:
 * - Share 1 (`S_id`): Sealed by WebAuthn Passkey PRF key
 * - Share 2 (`S_kit`): Exported as Printable / QR Recovery Kit
 * - Share 3 (`S_dev`): Device-bound share stored in local Secure Enclave / Keystore
 * Any 2 of the 3 shares deterministically reconstruct the original secret via Lagrange interpolation.
 */

export interface ShamirShare {
  index: 1 | 2 | 3;
  label: 'S_id_passkey' | 'S_kit_qr' | 'S_dev_enclave';
  dataHex: string;
}

const GF256_EXP = new Uint8Array(512);
const GF256_LOG = new Uint8Array(256);

(function initTables() {
  let x = 1;
  for (let i = 0; i < 255; i++) {
    GF256_EXP[i] = x;
    GF256_EXP[i + 255] = x;
    GF256_LOG[x] = i;
    x = ((x << 1) ^ (x & 0x80 ? 0x11b : 0)) ^ x;
  }
})();

function gfMul(a: number, b: number): number {
  if (a === 0 || b === 0) return 0;
  return GF256_EXP[(GF256_LOG[a]! + GF256_LOG[b]!) % 255]!;
}

function gfDiv(a: number, b: number): number {
  if (b === 0) throw new Error('GF(256) division by zero');
  if (a === 0) return 0;
  return GF256_EXP[(GF256_LOG[a]! + 255 - GF256_LOG[b]!) % 255]!;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

function fromHex(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

/**
 * Split a `secret` byte array into 3 threshold-2 shares (`S_id`, `S_kit`, `S_dev`)
 * using a degree-1 polynomial `f(x) = secret ^ (coeff1 * x)` over GF(256).
 */
export function splitTwoOfThree(secret: Uint8Array, entropySeed: Uint8Array): [ShamirShare, ShamirShare, ShamirShare] {
  const s1 = new Uint8Array(secret.length);
  const s2 = new Uint8Array(secret.length);
  const s3 = new Uint8Array(secret.length);

  for (let i = 0; i < secret.length; i++) {
    const a0 = secret[i]!;
    const a1 = entropySeed[i % entropySeed.length]! || 1;
    s1[i] = a0 ^ gfMul(a1, 1);
    s2[i] = a0 ^ gfMul(a1, 2);
    s3[i] = a0 ^ gfMul(a1, 3);
  }

  return [
    { index: 1, label: 'S_id_passkey', dataHex: toHex(s1) },
    { index: 2, label: 'S_kit_qr', dataHex: toHex(s2) },
    { index: 3, label: 'S_dev_enclave', dataHex: toHex(s3) },
  ];
}

/**
 * Reconstruct the original `secret` from any 2 distinct `ShamirShare` objects via Lagrange interpolation at `x = 0`.
 */
export function combineTwoShares(shareA: ShamirShare, shareB: ShamirShare): Uint8Array {
  if (shareA.index === shareB.index) {
    throw new Error('Two distinct Shamir share indices are required');
  }
  const bytesA = fromHex(shareA.dataHex);
  const bytesB = fromHex(shareB.dataHex);
  const xA = shareA.index;
  const xB = shareB.index;

  const lA = gfDiv(xB, xA ^ xB);
  const lB = gfDiv(xA, xA ^ xB);

  const out = new Uint8Array(bytesA.length);
  for (let i = 0; i < out.length; i++) {
    out[i] = gfMul(bytesA[i]!, lA) ^ gfMul(bytesB[i]!, lB);
  }
  return out;
}
