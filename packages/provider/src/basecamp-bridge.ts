import { LezerErrorEnvelope, LezerSdkError } from './errors';
import {
  AccountRef,
  ConnectOptions,
  DappMetadata,
  FaucetStatus,
  TokenBalanceEntry,
  TxIntent,
  TxReceipt,
} from './types';

interface LogosGlobal {
  callModuleAsync?: (
    moduleName: string,
    methodName: string,
    args: unknown[],
    callback: (payload: string) => void,
  ) => void;
}

function getLogosGlobal(): LogosGlobal | undefined {
  const g = globalThis as unknown as { logos?: LogosGlobal };
  return g.logos;
}

export class LezerProvider {
  private sessionToken: string | null = null;
  private readonly moduleName = 'lezer_wallet_provider';

  static isAvailable(): boolean {
    return typeof getLogosGlobal()?.callModuleAsync === 'function';
  }

  private callModule<T>(method: string, args: unknown[]): Promise<T> {
    return new Promise((resolve, reject) => {
      const logos = getLogosGlobal();
      if (!logos?.callModuleAsync) {
        reject(
          new LezerSdkError({
            code: 'LZR-4001',
            name: 'SequencerUnreachable',
            category: 'network_and_sequencer',
            message: 'Logos Basecamp IPC bridge (window.logos.callModuleAsync) is not available',
            retryable: false,
          }),
        );
        return;
      }

      logos.callModuleAsync(this.moduleName, method, args, (raw: string) => {
        try {
          const parsed = JSON.parse(raw) as { error?: LezerErrorEnvelope } & T;
          if (parsed && typeof parsed === 'object' && 'error' in parsed && parsed.error) {
            reject(new LezerSdkError(parsed.error));
            return;
          }
          resolve(parsed);
        } catch (err) {
          reject(err);
        }
      });
    });
  }

  async connect(
    dapp: DappMetadata,
    opts?: ConnectOptions,
  ): Promise<{ sessionToken: string; accounts: AccountRef[] }> {
    const req = await this.callModule<{ request_id: string; status: string }>('connect', [
      JSON.stringify({ ...dapp, ...opts }),
    ]);
    const resolved = await this.callModule<{
      status: string;
      session_token?: string;
      authorized_accounts?: string[];
    }>('get_request_status', [req.request_id]);

    const token = resolved.session_token ?? '';
    this.sessionToken = token;
    const accounts: AccountRef[] = (resolved.authorized_accounts ?? []).map((id) => ({
      accountId: id,
      privacy: id.startsWith('Private/') ? 'private' : 'public',
      label: id,
    }));
    return { sessionToken: token, accounts };
  }

  async disconnect(): Promise<void> {
    if (this.sessionToken) {
      await this.callModule('disconnect', [this.sessionToken]);
      this.sessionToken = null;
    }
  }

  async getAccounts(): Promise<AccountRef[]> {
    const res = await this.callModule<{ accounts: string[] }>('get_accounts', [
      this.sessionToken ?? '',
    ]);
    return res.accounts.map((id) => ({
      accountId: id,
      privacy: id.startsWith('Private/') ? 'private' : 'public',
      label: id,
    }));
  }

  async getBalance(accountId: string, assetId: 'native' | `token:${string}` = 'native'): Promise<string> {
    const res = await this.callModule<{ balance: string }>('get_balance', [
      this.sessionToken ?? '',
      accountId,
      assetId,
    ]);
    return res.balance;
  }

  async getTokenBalances(accountId: string): Promise<TokenBalanceEntry[]> {
    const res = await this.callModule<{ balances: TokenBalanceEntry[] }>('get_token_balances', [
      this.sessionToken ?? '',
      accountId,
    ]);
    return res.balances;
  }

  async proposeTransaction(intent: TxIntent): Promise<TxReceipt> {
    const res = await this.callModule<{ request_id: string }>('propose_transaction', [
      this.sessionToken ?? '',
      JSON.stringify(intent),
    ]);
    const finalStatus = await this.callModule<{ tx_hash?: string }>('get_request_status', [
      res.request_id,
    ]);
    return { txHash: finalStatus.tx_hash ?? '' };
  }

  async faucetInfo(): Promise<FaucetStatus> {
    return this.callModule<FaucetStatus>('faucet_info', []);
  }
}
