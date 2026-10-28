import { LezerSdkError } from './errors';
import type { AccountRef, Balance, DappMetadata, TxIntent, TxReceipt } from './types';

/**
 * In-memory stand-in with the same surface as `LezerProvider`, for dApp unit tests and the
 * portal playground. It holds no keys and never touches a chain; receipts are clearly fake
 * (`status.state === 'submitted'`, `detail: 'mock'`).
 */
export class MockLezerProvider {
  private connected = false;
  private allowPrivateRead = false;
  private readonly accounts: AccountRef[] = [
    { accountId: 'Public/MockPublicAccount11111111111111111111111', privacy: 'public' },
    { accountId: 'Private/MockPrivateAccount1111111111111111111111', privacy: 'private' },
  ];

  async connect(dapp: DappMetadata): Promise<AccountRef[]> {
    this.connected = true;
    this.allowPrivateRead = Boolean(dapp.requestPrivateBalanceRead);
    return [...this.accounts];
  }

  async disconnect(): Promise<void> {
    this.connected = false;
  }

  private ensure(): void {
    if (!this.connected) {
      throw new LezerSdkError({
        code: 'LZR-2001',
        name: 'SessionExpiredOrInvalid',
        category: 'session_and_acl',
        message: 'not connected',
        retryable: false,
      });
    }
  }

  async getAccounts(): Promise<AccountRef[]> {
    this.ensure();
    return [...this.accounts];
  }

  async getBalance(accountId: string): Promise<Balance> {
    this.ensure();
    const isPrivate = accountId.startsWith('Private/');
    if (isPrivate && !this.allowPrivateRead) {
      throw new LezerSdkError({
        code: 'LZR-2003',
        name: 'PrivateBalanceForbidden',
        category: 'session_and_acl',
        message: 'private balance read not granted',
        retryable: false,
      });
    }
    return { account_id: accountId, kind: isPrivate ? 'private' : 'public', balance: '1000000000', source: 'live', block_id: 1 };
  }

  async proposeTransaction(intent: TxIntent): Promise<TxReceipt> {
    this.ensure();
    const from =
      intent.kind === 'native_transfer' || intent.kind === 'token_transfer' || intent.kind === 'token_burn'
        ? intent.from
        : intent.kind === 'submit_testimonial'
          ? intent.author
          : intent.kind === 'faucet_claim'
            ? intent.recipient
            : intent.authority;
    const to =
      intent.kind === 'native_transfer' || intent.kind === 'token_transfer' || intent.kind === 'token_mint'
        ? intent.to
        : intent.kind === 'faucet_claim'
          ? intent.recipient
          : intent.kind === 'submit_testimonial'
            ? 'lezer_testimonial'
            : intent.token_id;
    const amount =
      'amount' in intent ? intent.amount : intent.kind === 'faucet_claim' ? '50000000000' : '0';
    return {
      tx_hash: '0'.repeat(64),
      status: { state: 'submitted', detail: 'mock' },
      mode: 'public',
      from,
      to,
      amount,
      build_ms: 0,
      inclusion_ms: null,
    };
  }
}
