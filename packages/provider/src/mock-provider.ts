import { LezerSdkError } from './errors';
import {
  AccountRef,
  ConnectOptions,
  DappMetadata,
  FaucetStatus,
  TokenBalanceEntry,
  TxIntent,
  TxReceipt,
} from './types';

/**
 * Deterministic `MockLezerProvider` for the `apps/lezer-portal` interactive playground
 * and dApp unit testing (zero key custody).
 */
export class MockLezerProvider {
  private sessionToken: string | null = null;
  private allowPrivateRead = false;
  private readonly accounts: AccountRef[] = [
    {
      accountId: 'Public/BH2zpUcwfB6cTKjYp2N8FFQNYSxjighmDfMYveFCC1Wf',
      privacy: 'public',
      label: 'Main Public',
    },
    {
      accountId: 'Private/9xLm2P8vQaShieldedVaultMockAccount111111111',
      privacy: 'private',
      label: 'Shielded Vault',
      receiveUri: 'lezer:priv1MockShieldedReceiveUriV1',
    },
  ];

  async connect(
    _dapp: DappMetadata,
    opts?: ConnectOptions,
  ): Promise<{ sessionToken: string; accounts: AccountRef[] }> {
    this.sessionToken = 'lzr_sess_mock_deterministic_01';
    this.allowPrivateRead = Boolean(opts?.requestPrivateBalanceRead);
    return {
      sessionToken: this.sessionToken,
      accounts: [...this.accounts],
    };
  }

  async disconnect(): Promise<void> {
    this.sessionToken = null;
    this.allowPrivateRead = false;
  }

  async getAccounts(): Promise<AccountRef[]> {
    this.ensureConnected();
    return [...this.accounts];
  }

  async getBalance(accountId: string): Promise<string> {
    this.ensureConnected();
    if (accountId.startsWith('Private/') && !this.allowPrivateRead) {
      throw new LezerSdkError({
        code: 'LZR-2003',
        name: 'PrivateBalanceForbidden',
        category: 'session_and_acl',
        message: `ReadPrivateBalance permission was not granted for ${accountId}`,
        retryable: false,
      });
    }
    return accountId.startsWith('Private/') ? '350' : '1420';
  }

  async getTokenBalances(accountId: string): Promise<TokenBalanceEntry[]> {
    this.ensureConnected();
    return [
      {
        definitionId: 'Public/MintDefLEZERUSD111111111111111111111111',
        name: 'LEZERUSD',
        holdingAccountId: `Public/ATA_${accountId.slice(-8)}`,
        isAta: true,
        privacy: accountId.startsWith('Private/') ? 'private' : 'public',
        balance: '700',
        mintAuthority: null,
      },
    ];
  }

  async proposeTransaction(_intent: TxIntent): Promise<TxReceipt> {
    this.ensureConnected();
    return {
      txHash: 'a1b2c3d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90',
      blockId: 36316,
      cyclesUsed: 155648,
      feePaid: '0',
    };
  }

  async faucetInfo(): Promise<FaucetStatus> {
    return {
      faucet_account_id: 'Public/EfQhKQAkX2FJiwNii2WFQsGndjvF1Mzd7RuVe7QdPLw7',
      prize_amount: '150',
      pool_balance: '1500000',
      claims_remaining: '10000',
      difficulty_bytes: 1,
      can_claim: true,
      blocked_reason: null,
    };
  }

  private ensureConnected(): void {
    if (!this.sessionToken) {
      throw new LezerSdkError({
        code: 'LZR-2001',
        name: 'SessionExpiredOrInvalid',
        category: 'session_and_acl',
        message: 'Provider is not connected; call connect() first',
        retryable: false,
      });
    }
  }
}
