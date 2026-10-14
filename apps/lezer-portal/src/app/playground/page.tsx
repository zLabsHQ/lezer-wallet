import { MockLezerProvider } from '../../../../packages/provider/src/mock-provider';

/**
 * Interactive `@lezer/provider` SDK Playground powered by `MockLezerProvider` (zero key custody).
 */
export async function runPlaygroundDemo() {
  const provider = new MockLezerProvider();
  const conn = await provider.connect(
    {
      moduleName: 'lezer_playground_demo',
      displayName: 'Lezer SDK Playground',
      version: '1.0.0',
    },
    { requestPrivateBalanceRead: true },
  );
  const pubBalance = await provider.getBalance(conn.accounts[0]!.accountId);
  const privBalance = await provider.getBalance(conn.accounts[1]!.accountId);
  const tx = await provider.proposeTransaction({
    kind: 'submit_testimonial',
    author: conn.accounts[0]!.accountId,
    submission_id: 'lp0021-lezer',
    shard_key: '0a',
    page_key: '00',
    username: '@playground',
    text: 'Testing @lezer/provider in the interactive playground!',
    timestamp: 1760900000,
  });

  return {
    sessionToken: conn.sessionToken,
    pubBalance,
    privBalance,
    txHash: tx.txHash,
  };
}
