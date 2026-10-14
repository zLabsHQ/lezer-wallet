import { SpelProgramIdl } from './risc0-serde';

/**
 * Logos Storage (`Codex`) IDL fetcher with graceful fallback when storage node is offline.
 */
export async function fetchSpelIdlFromStorage(
  storageNodeUrl: string,
  idlCid: string,
): Promise<SpelProgramIdl | null> {
  const base = storageNodeUrl.replace(/\/+$/, '');
  const url = `${base}/api/storage/v1/data/${encodeURIComponent(idlCid)}/network/stream`;
  try {
    const resp = await fetch(url);
    if (!resp.ok) {
      return null;
    }
    return (await resp.json()) as SpelProgramIdl;
  } catch {
    return null;
  }
}
