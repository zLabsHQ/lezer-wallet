export * from './pda';
export * from './risc0-serde';
export * from './storage';

export interface RegistryEntryRecord {
  program_id_hex: string;
  entry_pda: string;
  shard_key: string;
  page_key: string;
  author: string;
  kind: 'deployer' | 'third_party';
  name: string;
  version: string;
  description: string;
  tags: string[];
  idl_cid: string;
  source_cid: string;
  repo_url: string;
  commit: string;
  build_config: string;
  registered_at: number;
}
