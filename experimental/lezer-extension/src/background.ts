/**
 * Manifest V3 Background Service Worker (`docs/07` §3.1 & §3.2).
 *
 * Implements the Two-Gear Signing Router:
 * - Gear 1 (`gear1_pure_wasm_ed25519`): Public transfers & SPEL program calls signed in-browser via `lezer-wasm`.
 * - Gear 2 (`gear2_native_risc0_rapidsnark`): Shielded/Private transfers delegated to `lezerd` over Chromium Native Messaging.
 */

export type SigningGear = 'gear1_pure_wasm_ed25519' | 'gear2_native_risc0_rapidsnark';

export function selectSigningGear(fromAccount: string, toAccount?: string): SigningGear {
  const isPrivateSender = fromAccount.startsWith('Private/');
  const isPrivateRecipient =
    Boolean(toAccount?.startsWith('Private/')) || Boolean(toAccount?.startsWith('lezer:priv1'));
  if (isPrivateSender || isPrivateRecipient) {
    return 'gear2_native_risc0_rapidsnark';
  }
  return 'gear1_pure_wasm_ed25519';
}
