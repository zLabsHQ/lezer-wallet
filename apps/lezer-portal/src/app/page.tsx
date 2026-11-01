/**
 * Lezer portal landing content — Basecamp `lgpd` install instructions & module list.
 * Zero key custody: all signing lives exclusively in native Rust (`lezer-core`).
 */
export const PORTAL_HERO = {
  title: 'Lezer Wallet Suite for Logos Execution Zone (LEZ)',
  submissionId: 'lp0021-lezer',
  lgpdInstallCommand:
    'lgpd repo add lezer https://raw.githubusercontent.com/zLabsHQ/lezer-wallet/master/logos-repo.json && lgpd install lezer_wallet_provider lezer_wallet_ui lezer_testimonial_ui lezer_faucet_ui',
  modules: [
    'lezer_wallet_provider',
    'lezer_wallet_ui',
    'lezer_testimonial_ui',
    'lezer_faucet_ui',
  ],
};

export default function LandingPage(): typeof PORTAL_HERO {
  return PORTAL_HERO;
}
