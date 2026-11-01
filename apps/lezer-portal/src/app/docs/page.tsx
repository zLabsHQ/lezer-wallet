/**
 * Developer documentation sections for `@lezer/provider`, `LezerProvider.qml`, and the `lezer` CLI.
 * Every command and API shape here matches the shipped code (see DEVELOPERS.md).
 */
export const SDK_DOCUMENTATION_SECTIONS = [
  {
    id: 'quickstart',
    title: '1. Install the Basecamp modules via lgpd',
    command:
      'lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json && lgpd install lezer_wallet_provider lezer_wallet_ui lezer_testimonial_ui lezer_faucet_ui',
  },
  {
    id: 'provider-sdk',
    title: '2. Connect a Basecamp ui_qml dApp via @lezer/provider',
    example: [
      "const provider = new LezerProvider();",
      "const accounts = await provider.connect({ displayName: 'My LEZ dApp' });",
      "const receipt = await provider.proposeTransaction({ kind: 'faucet_claim', recipient: accounts[0].accountId });",
    ].join('\n'),
  },
  {
    id: 'cli-verifier',
    title: '3. Wallet-less adoption audit straight from the sequencer',
    command:
      'lezer --network https://testnet.lez.logos.co verify-adoption --program Public/<lezer_testimonial program id>',
  },
] as const;

export default function DocsPage() {
  return SDK_DOCUMENTATION_SECTIONS;
}
