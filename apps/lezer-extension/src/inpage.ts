/**
 * Injected Page Provider (`window.lezer`) for Web dApps (`docs/07` §3.1).
 *
 * Forwards whitelisted public RPC requests (`connect`, `getAccounts`, `getBalance`,
 * `proposeTransaction`) via `window.postMessage` (`LEZER_PAGE_REQUEST`) to the
 * extension content script and background service worker.
 */

export interface LezerPageRequest {
  type: 'LEZER_PAGE_REQUEST';
  requestId: string;
  method: 'connect' | 'disconnect' | 'getAccounts' | 'getBalance' | 'proposeTransaction';
  params: unknown;
}

export function createInpageRequest(
  requestId: string,
  method: LezerPageRequest['method'],
  params: unknown,
): LezerPageRequest {
  return {
    type: 'LEZER_PAGE_REQUEST',
    requestId,
    method,
    params,
  };
}
