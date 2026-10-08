//! Vault lifecycle CLI commands (`lezer init`, `restore`, `unlock`, `lock`, `status`).

use lezer_core::{LezerResult, WalletEngine};
use serde_json::{Value, json};

/// Execute `lezer init` — creates a new 24-word BIP-39 mnemonic vault (`vault.enc`).
pub fn handle_init(engine: &mut WalletEngine, password: &str) -> LezerResult<Value> {
    let res = engine.init_vault(password)?;
    Ok(json!({
        "status": "initialized",
        "vault_path": res.vault_path,
        "mnemonic_phrase": res.mnemonic_phrase,
        "main_public_account": res.main_public_account,
        "shielded_vault_account": res.shielded_vault_account,
        "active_zone": res.active_zone,
    }))
}

/// Execute `lezer restore` — restores a vault from an existing 24-word mnemonic.
pub fn handle_restore(
    engine: &mut WalletEngine,
    mnemonic: &str,
    password: &str,
) -> LezerResult<Value> {
    let res = engine.restore_vault(mnemonic, password)?;
    Ok(json!({
        "status": "restored",
        "vault_path": res.vault_path,
        "main_public_account": res.main_public_account,
        "shielded_vault_account": res.shielded_vault_account,
        "active_zone": res.active_zone,
    }))
}

/// Execute `lezer unlock` — decrypts and verifies `vault.enc` with `password`.
pub fn handle_unlock(engine: &mut WalletEngine, password: &str) -> LezerResult<Value> {
    engine.unlock(password)?;
    let accounts = engine.list_accounts()?;
    Ok(json!({
        "status": "unlocked",
        "lock_state": engine.lock_state(),
        "vault_path": engine.vault_path().display().to_string(),
        "accounts_count": accounts.len(),
        "accounts": accounts,
    }))
}

/// Execute `lezer lock` — locks the vault and zeroizes decrypted in-memory keys.
pub fn handle_lock(engine: &mut WalletEngine) -> LezerResult<Value> {
    let state = engine.lock();
    Ok(json!({
        "status": "locked",
        "lock_state": state,
        "vault_path": engine.vault_path().display().to_string(),
    }))
}

/// Execute `lezer status` — returns the canonical `WalletStateSnapshot`.
pub fn handle_status(engine: &WalletEngine) -> LezerResult<Value> {
    let snapshot = engine.snapshot();
    Ok(serde_json::to_value(snapshot).expect("snapshot serializes"))
}
