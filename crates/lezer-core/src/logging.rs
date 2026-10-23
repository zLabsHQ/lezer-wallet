//! Zero-telemetry structured logging (`~/.lezer/logs/lezer.log`) and `Redacted<T>` wrapper.
//!
//! Enforces compile-time redaction so mnemonics, passwords, Argon2id keys,
//! spending keys, and viewing seeds never appear in log output.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Compile-time secret redaction wrapper (`Redacted<T>`).
///
/// Any secret wrapped in `Redacted<T>` formats as `"[REDACTED]"` in both `Debug` and
/// `Display` implementations and zeroizes its memory on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Redacted<T: Zeroize>(T);

impl<T: Zeroize> Redacted<T> {
    pub fn new(secret: T) -> Self {
        Self(secret)
    }

    pub fn expose_secret(&self) -> &T {
        &self.0
    }

    pub fn into_inner(mut self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut self.0)
    }
}

impl<T: Zeroize> std::fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"[REDACTED]\"")
    }
}

impl<T: Zeroize> std::fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T: Zeroize + Serialize> Serialize for Redacted<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

/// Structured JSON log entry written to `~/.lezer/logs/lezer.log`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredLogEntry {
    pub timestamp_unix: u64,
    pub level: String,
    pub event: String,
    pub details: serde_json::Value,
}

/// Append-only ring-capped local logger (`~/.lezer/logs/lezer.log`, max 10 MiB x 3 files).
/// Never transmits telemetry over the network.
#[derive(Debug, Clone)]
pub struct AuditLogger {
    log_path: PathBuf,
}

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

impl AuditLogger {
    /// Construct a logger writing to `home_dir/logs/lezer.log`.
    pub fn new(home_dir: &Path) -> Self {
        Self {
            log_path: home_dir.join("logs").join("lezer.log"),
        }
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Record a structured JSON log event locally.
    pub fn record(&self, level: &str, event: &str, details: serde_json::Value) {
        if let Some(parent) = self.log_path.parent() {
            let _ = crate::vault::ensure_private_dir(parent);
        }
        self.rotate_if_needed();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        let entry = StructuredLogEntry {
            timestamp_unix: now,
            level: level.to_string(),
            event: event.to_string(),
            details,
        };

        if let Ok(line) = serde_json::to_string(&entry)
            && let Ok(mut file) = {
                let mut opts = OpenOptions::new();
                opts.create(true).append(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt as _;
                    opts.mode(0o600);
                }
                opts.open(&self.log_path)
            }
        {
            let _ = writeln!(file, "{line}");
        }
    }

    fn rotate_if_needed(&self) {
        let Ok(meta) = fs::metadata(&self.log_path) else {
            return;
        };
        if meta.len() < MAX_LOG_BYTES {
            return;
        }
        let rot2 = self.log_path.with_extension("log.2");
        let rot1 = self.log_path.with_extension("log.1");
        let _ = fs::rename(&rot1, &rot2);
        let _ = fs::rename(&self.log_path, &rot1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_debug_display_and_json_never_leak() {
        let secret = Redacted::new("abandon ability able about above absent".to_string());
        assert_eq!(format!("{secret:?}"), "\"[REDACTED]\"");
        assert_eq!(format!("{secret}"), "[REDACTED]");
        let json = serde_json::to_string(&secret).unwrap();
        assert_eq!(json, "\"[REDACTED]\"");
        assert!(!json.contains("abandon"));
    }

    #[test]
    fn audit_logger_writes_redacted_structured_json() {
        let tmp = tempfile::tempdir().unwrap();
        let logger = AuditLogger::new(tmp.path());
        let password = Redacted::new("super-secret-password-123".to_string());

        logger.record(
            "INFO",
            "vault_unlock",
            serde_json::json!({
                "password": password,
                "status": "ok"
            }),
        );

        let contents = fs::read_to_string(logger.log_path()).unwrap();
        assert!(contents.contains("vault_unlock"));
        assert!(contents.contains("[REDACTED]"));
        assert!(!contents.contains("super-secret-password-123"));
    }
}
