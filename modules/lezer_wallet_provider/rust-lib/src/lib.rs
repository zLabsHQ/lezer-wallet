//! Basecamp core module shim. All logic lives in `lezer-provider`; this file only maps the
//! builder-generated `LezerWalletProviderModule` trait onto it, supplying the caller
//! identity the Logos host authenticated for the in-flight call.

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/generated/provider_gen.rs"));

use lezer_provider::{Caller, Provider};

struct WalletModule {
    provider: Option<Provider>,
}

impl Default for WalletModule {
    fn default() -> Self {
        Self { provider: None }
    }
}

fn caller() -> Caller {
    match logos_rust_sdk::current_caller() {
        logos_rust_sdk::LogosCaller::Module { name, .. } => Caller::Module(name),
        _ => Caller::Unknown,
    }
}

impl WalletModule {
    fn provider(&mut self) -> &Provider {
        self.provider.get_or_insert_with(|| {
            let home = context()
                .map(|c| std::path::PathBuf::from(c.instance_persistence_path))
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(lezer_provider::default_home);
            Provider::new(home, None)
        })
    }

    fn flush_events(&mut self) {
        for e in self.provider().take_events() {
            let payload = e.payload.to_string();
            match e.name {
                "approvalRequested" => emit_approval_requested(&e.request_id, &payload),
                "requestResolved" => emit_request_resolved(&e.request_id, &payload),
                _ => {}
            }
        }
    }

    fn finish(&mut self, out: Vec<u8>) -> String {
        self.flush_events();
        String::from_utf8(out).unwrap_or_default()
    }
}

impl LezerWalletProviderModule for WalletModule {
    fn on_context_ready(&mut self, _ctx: &RustModuleContext) {
        self.provider();
    }

    fn get_status(&mut self) -> String {
        let out = self.provider().get_status();
        self.finish(out)
    }

    fn connect(&mut self, dapp_info: String) -> String {
        let out = self.provider().connect(&caller(), dapp_info.as_bytes());
        self.finish(out)
    }

    fn get_request_status(&mut self, request_id: String) -> String {
        let out = self.provider().get_request_status(&caller(), &request_id);
        self.finish(out)
    }

    fn disconnect(&mut self, session_token: String) -> String {
        let out = self.provider().disconnect(&caller(), &session_token);
        self.finish(out)
    }

    fn get_accounts(&mut self, session_token: String) -> String {
        let out = self.provider().get_accounts(&caller(), &session_token);
        self.finish(out)
    }

    fn get_balance(&mut self, session_token: String, account_id: String) -> String {
        let out = self.provider().get_balance(&caller(), &session_token, &account_id);
        self.finish(out)
    }

    fn get_receive_address(&mut self, session_token: String, account_id: String) -> String {
        let out = self.provider().get_receive_address(&caller(), &session_token, &account_id);
        self.finish(out)
    }

    fn propose_transaction(&mut self, session_token: String, intent_json: String) -> String {
        let out = self.provider().propose_transaction(&caller(), &session_token, intent_json.as_bytes());
        self.finish(out)
    }

    fn wallet_call(&mut self, op_json: String) -> String {
        let out = self.provider().wallet_call(&caller(), op_json.as_bytes());
        self.finish(out)
    }

    fn poll_events(&mut self) -> String {
        let events: Vec<serde_json::Value> = self
            .provider()
            .take_events()
            .into_iter()
            .map(|e| serde_json::json!({ "event": e.name, "request_id": e.request_id, "payload": e.payload }))
            .collect();
        serde_json::to_string(&events).unwrap_or_default()
    }
}

#[no_mangle]
pub extern "Rust" fn logos_module_install() {
    install::<WalletModule>();
}
