/** Wire types of the `lezer_wallet_provider` Basecamp module (mirrors `lezer-core`). */

export type VerificationBadgeState =
  | 'protocol_builtin'
  | 'verified_source'
  | 'known_but_unverified'
  | 'no_registry_entry'
  | 'mismatch_or_error';

export interface SourceVerificationBadge {
  state: VerificationBadgeState;
  label: string;
  program_name: string;
  program_id: string;
}

export interface SecurityWarning {
  code: string;
  severity: 'info' | 'amber_gate' | 'critical_red_override';
  title: string;
  message: string;
}

export type Precheck =
  | { result: 'passed'; available: string }
  | { result: 'insufficient_funds'; available: string; required: string }
  | { result: 'unavailable'; reason: string };

export type PrivacyMode = 'public' | 'shielded' | 'deshielded' | 'private';

export interface TransactionSummary {
  proposal_id: string;
  originating_dapp: string;
  signing_account: string;
  privacy_mode: PrivacyMode;
  requires_zk_proof: boolean;
  instruction_name: string;
  headline: string;
  from: string;
  to: string;
  /** Atomic units, decimal string (u128). */
  amount: string;
  max_fee: string | null;
  precheck: Precheck;
  verification_badge: SourceVerificationBadge;
  warnings: SecurityWarning[];
  requires_unverified_ack: boolean;
  requires_critical_override: boolean;
  summary_digest_hex: string;
}

export interface DappMetadata {
  /** Shown to the user as secondary info; the authenticated origin is the calling module. */
  displayName: string;
  /** Ask the user for permission to read balances of shared private accounts. */
  requestPrivateBalanceRead?: boolean;
}

export interface AccountRef {
  accountId: string;
  privacy: 'public' | 'private';
}

export interface Balance {
  account_id: string;
  kind: 'public' | 'private';
  balance: string;
  source: 'live' | 'cached' | 'local';
  block_id: number | null;
}

export type TxIntent =
  | { kind: 'native_transfer'; from: string; to: string; amount: string }
  | { kind: 'submit_testimonial'; author: string; username: string; text: string }
  | { kind: 'faucet_claim'; recipient: string }
  | { kind: 'token_transfer'; token_id: string; from: string; to: string; amount: string }
  | { kind: 'token_mint'; token_id: string; authority: string; to: string; amount: string }
  | { kind: 'token_burn'; token_id: string; from: string; amount: string }
  | { kind: 'token_set_authority'; token_id: string; authority: string; new_authority: string | null };

export type TxStatus = { state: 'included'; block_id: number } | { state: 'submitted'; detail: string };

export interface TxReceipt {
  tx_hash: string;
  status: TxStatus;
  mode: PrivacyMode;
  from: string;
  to: string;
  amount: string;
  build_ms: number;
  inclusion_ms: number | null;
}

/** `get_request_status` result. */
export type RequestResolution =
  | { status: 'pending'; request_id: string }
  | { status: 'executing'; request_id: string }
  | { status: 'approved_connect'; request_id: string; session_token: string; authorized_accounts: string[] }
  | { status: 'approved_proposal'; request_id: string; tx_hash: string; result: TxReceipt }
  | { status: 'rejected'; request_id: string; code: string; reason: string };

export interface ProviderStatus {
  network_id: string;
  sequencer_url: string;
  lock_state: 'uninitialized' | 'locked' | 'unlocked' | 'deferred_lock_pending';
  busy: boolean;
}
