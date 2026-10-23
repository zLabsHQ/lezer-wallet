import {
  LEZER_REGISTRY_PROGRAM_HEX,
  LEZER_TESTIMONIAL_PROGRAM_HEX,
} from '../../../../../packages/registry/src/pda';

/**
 * LP-0023 Program Registry Explorer & Live 16-Shard Adoption Dashboard metadata.
 */
export const SEEDED_LP0023_PROGRAMS = [
  {
    name: 'lezer_registry',
    programIdHex: LEZER_REGISTRY_PROGRAM_HEX,
    kind: 'deployer',
    badge: 'not_deployed',
    idlCid: null,
    sourceCid: null,
  },
  {
    name: 'lezer_testimonial',
    programIdHex: LEZER_TESTIMONIAL_PROGRAM_HEX,
    kind: 'deployer',
    badge: 'not_deployed',
    idlCid: null,
    sourceCid: null,
  },
] as const;
