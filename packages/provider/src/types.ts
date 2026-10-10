export type VerificationBadgeState =
  | 'verified_source'
  | 'known_but_unverified'
  | 'no_registry_entry'
  | 'mismatch_or_error';

export interface SourceVerificationBadge {
  state: VerificationBadgeState;
  label: string;
  program_name: string;
  program_version?: string | null;
  program_id_hex: string;
  repo_url?: string | null;
  commit_hash?: string | null;
  idl_cid?: string | null;
  source_cid?: string | null;
  requires_unverified_acknowledgement: boolean;
  blocked_by_mismatch: boolean;
}

export interface SimulatedBalanceDelta {
  account_id: string;
  asset_id: string;
  symbol: string;
  delta: string;
}

export interface ComputeEstimate {
  estimated_cycles: number;
  segments: number;
  estimated_fee_lez: string;
}

export interface SecurityWarning {
  code: string;
  severity: 'info' | 'amber_gate' | 'critical_red_override';
  title: string;
  message: string;
}

export interface TransactionSummary {
  proposal_id: string;
  originating_dapp: string;
  signing_account: string;
  privacy_mode: string;
  program_id_hex: string;
  program_name: string;
  instruction_name: string;
  headline: string;
  balance_deltas: SimulatedBalanceDelta[];
  authority_changes: string[];
  decoded_args: Record<string, string>;
  verification_badge: SourceVerificationBadge;
  compute_estimate: ComputeEstimate;
  warnings: SecurityWarning[];
  requires_unverified_ack: boolean;
  requires_critical_override: boolean;
  raw_instruction_hex: string;
  summary_digest_hex: string;
}

export interface ApproveDecision {
  proposal_id: string;
  summary_digest_hex: string;
  acknowledged_unverified: boolean;
  critical_override: boolean;
}

export interface DappMetadata {
  moduleName: string;
  displayName: string;
  version: string;
  packageHash?: string;
  icon?: string;
}

export interface ConnectOptions {
  zone?: string;
  requestPrivateBalanceRead?: boolean;
}

export interface AccountRef {
  accountId: string;
  privacy: 'public' | 'private';
  label: string;
  receiveUri?: string;
}

export interface TokenBalanceEntry {
  definitionId: string;
  name: string;
  holdingAccountId: string;
  isAta: boolean;
  privacy: 'public' | 'private';
  balance: string;
  mintAuthority?: string | null;
}

export type TxIntent =
  | {
      kind: 'native_transfer';
      from: string;
      to: string;
      amount: string;
    }
  | {
      kind: 'token_transfer';
      definition_id: string;
      symbol: string;
      from: string;
      to: string;
      amount: string;
      use_ata: boolean;
    }
  | {
      kind: 'token_create';
      name: string;
      total_supply: string;
      creator_account: string;
      mint_authority?: string | null;
    }
  | {
      kind: 'faucet_claim';
      account: string;
      auto_shield_to?: string | null;
    }
  | {
      kind: 'submit_testimonial';
      author: string;
      submission_id: string;
      shard_key: string;
      page_key: string;
      username: string;
      text: string;
      timestamp: number;
    }
  | {
      kind: 'program_call';
      program_id_hex: string;
      instruction_name?: string;
      instruction_data_hex: string;
      accounts: Array<{ account: string; writable: boolean; signer: boolean }>;
    };

export interface TxReceipt {
  txHash: string;
  blockId?: number;
  cyclesUsed?: number;
  feePaid?: string;
}

export interface FaucetStatus {
  faucet_account_id: string;
  prize_amount: string;
  pool_balance: string;
  claims_remaining: string;
  difficulty_bytes: number;
  can_claim: boolean;
  blocked_reason?: string | null;
}
