//! `lezerd` — Chromium / Firefox Native Messaging Host Daemon (`docs/07` §3.1 & §3.2).
//!
//! Speaks the standard 4-byte little-endian length-prefixed JSON wire framing over
//! `stdin` / `stdout` so the Lezer Browser Extension (`apps/lezer-extension`) can delegate
//! Gear 2 private RISC0 + RapidSnark Groth16 proofs (`Semaphore(1)`) to the local machine.

use std::io::{self, Read, Write};

use anyhow::{Context, Result, bail};
use lezer_core::WalletEngine;
use serde_json::{Value, json};

/// Maximum Native Messaging frame size (1 MiB).
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Read a single 4-byte little-endian length-prefixed JSON message from `reader`.
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<Option<Value>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_BYTES {
        bail!("invalid Native Messaging frame length: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    let parsed: Value =
        serde_json::from_slice(&payload).context("malformed JSON in Native Messaging frame")?;
    Ok(Some(parsed))
}

/// Write a single 4-byte little-endian length-prefixed JSON message to `writer`.
pub fn write_native_message<W: Write>(writer: &mut W, message: &Value) -> Result<()> {
    let payload = serde_json::to_vec(message)?;
    let len = u32::try_from(payload.len()).context("frame exceeds u32::MAX")?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Process a single JSON request from the Browser Extension.
pub fn handle_daemon_request(engine: &mut WalletEngine, req: &Value) -> Value {
    let op = req["op"].as_str().unwrap_or("ping");
    match op {
        "ping" => json!({
            "type": "pong",
            "daemon": "lezerd/1.0.0",
            "gear": "gear2_native_risc0_rapidsnark",
        }),
        "status" => serde_json::to_value(engine.snapshot()).unwrap_or_else(|_| json!({})),
        other => json!({
            "error": {
                "code": "LZR-3003",
                "message": format!("unsupported lezerd op '{other}'"),
            }
        }),
    }
}

fn main() -> Result<()> {
    let mut engine = WalletEngine::with_default_adapter()?;
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();

    while let Some(req) = read_native_message(&mut stdin)? {
        let resp = handle_daemon_request(&mut engine, &req);
        write_native_message(&mut stdout, &resp)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn native_messaging_4byte_le_framing_roundtrips() {
        let mut engine = WalletEngine::with_default_adapter().unwrap();
        let req = json!({ "op": "ping" });

        let mut wire = Vec::new();
        write_native_message(&mut wire, &req).unwrap();

        let mut cursor = Cursor::new(wire);
        let decoded = read_native_message(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded["op"], "ping");

        let resp = handle_daemon_request(&mut engine, &decoded);
        assert_eq!(resp["type"], "pong");
        assert_eq!(resp["gear"], "gear2_native_risc0_rapidsnark");
    }
}
