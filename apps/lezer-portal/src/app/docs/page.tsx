/**
 * Developer Documentation Page for `@lezer/provider`, `@lezer/registry`, and `lezer` CLI.
 */
export const SDK_DOCUMENTATION_SECTIONS = [
  {
    id: 'quickstart',
    title: '1. Basecamp Module Installation via lgpd',
    command:
      'lgpd repo add lezer https://raw.githubusercontent.com/logos-co/lezer-wallet/main/logos-repo.json && lgpd install lezer_wallet_provider lezer_wallet_ui lezer_faucet_ui lezer_testimonial_ui',
  },
  {
    id: 'provider-sdk',
    title: '2. Connecting a Basecamp ui_qml dApp via @lezer/provider',
    example: `const provider = new LezerProvider();\nconst { sessionToken, accounts } = await provider.connect({ moduleName: 'my_dapp', displayName: 'My LEZ dApp', version: '1.0.0' });`,
  },
  {
    id: 'cli-verifier',
    title: '3. Stateless 16-Shard Evaluator Adoption Audit',
    command:
      'LEZER_NO_VAULT=1 lezer verify-adoption --submission-id lp0021-lezer --sequencer https://testnet.lez.logos.co',
  },
] as const;

export default function DocsPage() {
  return SDK_DOCUMENTATION_SECTIONS;
}
