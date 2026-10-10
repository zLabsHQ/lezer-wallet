export type LezerErrorCategory =
  | 'vault_and_auth'
  | 'session_and_acl'
  | 'approval_and_simulation'
  | 'network_and_sequencer'
  | 'prover_and_crypto'
  | 'registry_and_storage';

export interface LezerErrorEnvelope {
  code: string;
  name: string;
  category: LezerErrorCategory;
  message: string;
  retryable: boolean;
}

export class LezerSdkError extends Error {
  public readonly code: string;
  public readonly category: LezerErrorCategory;
  public readonly retryable: boolean;

  constructor(envelope: LezerErrorEnvelope) {
    super(`${envelope.code}: ${envelope.name} (${envelope.message})`);
    this.name = 'LezerSdkError';
    this.code = envelope.code;
    this.category = envelope.category;
    this.retryable = envelope.retryable;
  }
}
