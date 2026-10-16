import { ShamirShare } from './shares';

/**
 * Printable & QR-exportable Recovery Kit (`S_kit`) and WebAuthn Passkey PRF envelope (`S_id`).
 */
export interface RecoveryKitBundle {
  version: 1;
  vaultBackupCid: string;
  storageNodeUrl: string;
  kitShare: ShamirShare;
  createdAtUnix: number;
}

export function encodeRecoveryKitUri(kit: RecoveryKitBundle): string {
  const json = JSON.stringify(kit);
  const b64 = Buffer.from(json, 'utf-8').toString('base64url');
  return `lezer:recovery1:${b64}`;
}

export function decodeRecoveryKitUri(uri: string): RecoveryKitBundle {
  const prefix = 'lezer:recovery1:';
  if (!uri.startsWith(prefix)) {
    throw new Error(`Invalid recovery kit URI prefix in '${uri}'`);
  }
  const raw = Buffer.from(uri.slice(prefix.length), 'base64url').toString('utf-8');
  return JSON.parse(raw) as RecoveryKitBundle;
}
