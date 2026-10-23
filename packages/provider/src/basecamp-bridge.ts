import { LezerErrorEnvelope, LezerSdkError } from './errors';
import type {
  AccountRef,
  Balance,
  DappMetadata,
  ProviderStatus,
  RequestResolution,
  TxIntent,
  TxReceipt,
} from './types';

/** The Logos host bridge (`logos-view-module-runtime` LogosQmlBridge). */
interface LogosBridge {
  callModuleAsync(
    module: string,
    method: string,
    args: unknown[],
    callback: (payload: string) => void,
    timeoutMs?: number,
  ): void;
}

export interface LezerProviderOptions {
  /** Injected bridge; defaults to `globalThis.logos`. */
  bridge?: LogosBridge;
  /** Poll interval while waiting for the user (ms). */
  pollIntervalMs?: number;
  /** Give up waiting for the user after this long (ms). Default 10 min. */
  approvalTimeoutMs?: number;
  /** A transaction with a zero-knowledge proof can take minutes; default 20 min. */
  executionTimeoutMs?: number;
}

const MODULE = 'lezer_wallet_provider';

function bridgeError(message: string): LezerSdkError {
  return new LezerSdkError({
    code: 'LZR-4001',
    name: 'SequencerUnreachable',
    category: 'network_and_sequencer',
    message,
    retryable: true,
  });
}

/** Decode a bridge payload: JSON of the `tstr` result, which is itself JSON. */
export function decodePayload<T>(payload: string): T {
  let value: unknown = JSON.parse(payload);
  if (typeof value === 'string') value = JSON.parse(value);
  const err = (value as { error?: unknown } | null)?.error;
  if (typeof err === 'string') throw bridgeError(err);
  if (err && typeof err === 'object') throw new LezerSdkError(err as LezerErrorEnvelope);
  return value as T;
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * dApp-side client for the Lezer wallet. The wallet authenticates your module by its
 * host-issued identity; sessions cannot be used by other modules. Every transaction is
 * shown to the user in the wallet's approval sheet before it is signed.
 */
export class LezerProvider {
  private sessionToken: string | null = null;
  private readonly bridge: LogosBridge;
  private readonly pollIntervalMs: number;
  private readonly approvalTimeoutMs: number;
  private readonly executionTimeoutMs: number;

  constructor(opts: LezerProviderOptions = {}) {
    const bridge = opts.bridge ?? (globalThis as { logos?: LogosBridge }).logos;
    if (!bridge?.callModuleAsync) throw bridgeError('Logos bridge (logos.callModuleAsync) is not available');
    this.bridge = bridge;
    this.pollIntervalMs = opts.pollIntervalMs ?? 1000;
    this.approvalTimeoutMs = opts.approvalTimeoutMs ?? 10 * 60_000;
    this.executionTimeoutMs = opts.executionTimeoutMs ?? 20 * 60_000;
  }

  static isAvailable(): boolean {
    return typeof (globalThis as { logos?: LogosBridge }).logos?.callModuleAsync === 'function';
  }

  private call<T>(method: string, args: string[]): Promise<T> {
    return new Promise((resolve, reject) => {
      this.bridge.callModuleAsync(MODULE, method, args, (payload) => {
        try {
          resolve(decodePayload<T>(payload));
        } catch (e) {
          reject(e);
        }
      });
    });
  }

  private requireSession(): string {
    if (!this.sessionToken) {
      throw new LezerSdkError({
        code: 'LZR-2001',
        name: 'SessionExpiredOrInvalid',
        category: 'session_and_acl',
        message: 'not connected — call connect() first',
        retryable: false,
      });
    }
    return this.sessionToken;
  }

  /** Poll a request until it leaves `pending`/`executing`, or time out. */
  private async waitFor(requestId: string, timeoutMs: number): Promise<RequestResolution> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const r = await this.call<RequestResolution>('get_request_status', [requestId]);
      if (r.status !== 'pending' && r.status !== 'executing') return r;
      if (Date.now() > deadline) {
        throw new LezerSdkError({
          code: 'LZR-3002',
          name: 'ProposalExpired',
          category: 'approval_and_simulation',
          message: `no decision for ${requestId} within ${timeoutMs} ms`,
          retryable: true,
        });
      }
      await sleep(this.pollIntervalMs);
    }
  }

  private static rejected(r: Extract<RequestResolution, { status: 'rejected' }>): LezerSdkError {
    return new LezerSdkError({
      code: r.code,
      name: r.code === 'LZR-3001' ? 'UserRejected' : 'RequestRejected',
      category: 'approval_and_simulation',
      message: r.reason,
      retryable: false,
    });
  }

  async status(): Promise<ProviderStatus> {
    return this.call<ProviderStatus>('get_status', []);
  }

  /** Ask the user to connect; resolves once they approve (or rejects if they decline). */
  async connect(dapp: DappMetadata): Promise<AccountRef[]> {
    const req = await this.call<{ request_id: string }>('connect', [JSON.stringify(dapp)]);
    const r = await this.waitFor(req.request_id, this.approvalTimeoutMs);
    if (r.status === 'rejected') throw LezerProvider.rejected(r);
    if (r.status !== 'approved_connect') throw bridgeError(`unexpected resolution ${r.status}`);
    this.sessionToken = r.session_token;
    return r.authorized_accounts.map(toRef);
  }

  async disconnect(): Promise<void> {
    if (!this.sessionToken) return;
    await this.call('disconnect', [this.sessionToken]);
    this.sessionToken = null;
  }

  async getAccounts(): Promise<AccountRef[]> {
    const res = await this.call<{ accounts: string[] }>('get_accounts', [this.requireSession()]);
    return res.accounts.map(toRef);
  }

  /** Private accounts require the user's explicit per-account permission (else LZR-2003). */
  async getBalance(accountId: string): Promise<Balance> {
    return this.call<Balance>('get_balance', [this.requireSession(), accountId]);
  }

  async getReceiveAddress(accountId: string): Promise<string> {
    const r = await this.call<{ receive_address: string }>('get_receive_address', [this.requireSession(), accountId]);
    return r.receive_address;
  }

  /**
   * Propose a transaction. Resolves with the receipt once the user approved it and the
   * sequencer accepted it (private transfers include proof generation: minutes).
   */
  async proposeTransaction(intent: TxIntent): Promise<TxReceipt> {
    const req = await this.call<{ request_id: string }>('propose_transaction', [
      this.requireSession(),
      JSON.stringify(intent),
    ]);
    const r = await this.waitFor(req.request_id, this.approvalTimeoutMs + this.executionTimeoutMs);
    if (r.status === 'rejected') throw LezerProvider.rejected(r);
    if (r.status !== 'approved_proposal') throw bridgeError(`unexpected resolution ${r.status}`);
    return r.result;
  }
}

function toRef(id: string): AccountRef {
  return { accountId: id, privacy: id.startsWith('Private/') ? 'private' : 'public' };
}
