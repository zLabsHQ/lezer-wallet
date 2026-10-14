/**
 * Lezer Portal Landing Page — Basecamp `lgpd` install instructions & architecture overview.
 * Zero key custody: all signing lives exclusively in native Rust (`lezer-core`).
 */
export const PORTAL_HERO = {
  title: 'Lezer Wallet Suite for Logos Execution Zone (LEZ)',
  submissionId: 'lp0021-lezer',
  lgpdInstallCommand:
    'lgpd repo add lezer https://raw.githubusercontent.com/logos-co/lezer-wallet/main/logos-repo.json && lgpd install lezer_wallet_provider lezer_wallet_ui lezer_faucet_ui lezer_testimonial_ui',
  modules: [
    'lezer_wallet_provider',
    'lezer_wallet_ui',
    'lezer_faucet_ui',
    'lezer_testimonial_ui',
  ],
};

export default function LandingPage(): typeof PORTAL_HERO {
  return PORTAL_HERO;
}
