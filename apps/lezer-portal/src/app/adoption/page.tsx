/**
 * Adoption dashboard config. Live numbers come from `lezer verify-adoption`, which reads the
 * testimonial program's index pages directly from the sequencer.
 */
export const ADOPTION_DASHBOARD_CONFIG = {
  indexBuckets: 16,
  maxEntriesPerPage: 64,
  requiredDistinctTestimonials: 150,
  requiredSustainedMonths: 2,
  requiredMonthlyMinimum: 30,
  requiredEcosystemApps: 10,
  ecosystemBlueprints: [
    'Anonymous ZK Chat Identity Registrar (ui_qml)',
    'Token Launchpad & LP-0013 Mint Authority Console (ui_qml)',
    'Private Payroll / Payment Streamer (ui_qml)',
    'Private DAO / Token-Weighted Governance Client (ui_qml)',
    'LEZ Program Explorer & One-Click Source Verifier (ui_qml)',
    'Escrow / OTC Token Swap Board (ui_qml)',
    'On-Chain Markdown / Whistleblower Publisher (ui_qml)',
    'Shielded Tipping / Creator Donation Jar (ui_qml)',
    'Multi-Recipient Token Airdrop Tool (ui_qml)',
    'On-Chain Turn-Based Game Stake Manager (ui_qml)',
  ],
} as const;

export default function AdoptionPage() {
  return ADOPTION_DASHBOARD_CONFIG;
}
