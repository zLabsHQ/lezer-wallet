/**
 * LP-0023 registry explorer content. Program ids are network-specific and come from a real
 * deployment (`programs/ARTIFACTS.md`); live entries come from `lezer registry list`, a
 * wallet-less read of the `lezer_registry` program's index pages.
 */
export type RegistryBadge = 'not_deployed' | 'known_but_unverified' | 'verified_source';

export interface PortalProgram {
  name: string;
  imageId: string;
  testnetProgramId: string | null;
  badge: RegistryBadge;
}

/** Reproducible image ids (RISC0 r0.1.91.1 builder); testnet ids stay null until deployed. */
export const LEZER_PROGRAMS: readonly PortalProgram[] = [
  { name: 'lezer_registry', imageId: '66ac37a3840cfff5f69fead3f3404ddc9707cd7e807d21c76e328c4cdc5ffb2c', testnetProgramId: null, badge: 'not_deployed' },
  { name: 'lezer_testimonial', imageId: '912c3b34a8e58def7000f2c64bd7e6fd0eed9854ee922c220f9587828806e74b', testnetProgramId: null, badge: 'not_deployed' },
  { name: 'lezer_token', imageId: '581afed957bfc437e8bfb2cd268bdc5428fefd6a6c41a4d88b078c7fd1bbf066', testnetProgramId: null, badge: 'not_deployed' },
  { name: 'lezer_faucet', imageId: 'c22cd2cc627ca79657124c456d61e4fbdd1d7e27e49571b9d86a82fe37a4ea28', testnetProgramId: null, badge: 'not_deployed' },
];

export default function RegistryPage(): readonly PortalProgram[] {
  return LEZER_PROGRAMS;
}
