import { MockLezerProvider } from '../../../../packages/provider/src/mock-provider';

/**
 * `@lezer/provider` playground backed by `MockLezerProvider` (no keys, no chain).
 */
export async function runPlaygroundDemo() {
  const provider = new MockLezerProvider();
  const accounts = await provider.connect({ displayName: 'Lezer SDK Playground', requestPrivateBalanceRead: true });
  const pubBalance = await provider.getBalance(accounts[0]!.accountId);
  const privBalance = await provider.getBalance(accounts[1]!.accountId);
  const receipt = await provider.proposeTransaction({
    kind: 'submit_testimonial',
    author: accounts[0]!.accountId,
    username: '@playground',
    text: 'Testing @lezer/provider in the playground',
  });
  return { accounts, pubBalance, privBalance, receipt };
}
