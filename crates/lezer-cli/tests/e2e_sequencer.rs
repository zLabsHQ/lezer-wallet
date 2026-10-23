//! End-to-end tests of the `lezer` binary against a real LEZ v0.3.1 standalone sequencer.
//!
//! Skipped unless `LEZER_E2E_SEQUENCER` is set (e.g. `http://127.0.0.1:3040`). The sequencer
//! must run the upstream debug genesis (`lez/sequencer/service/configs/debug`), whose first
//! supply account key is published in `lez/testnet_initial_state`.
//!
//! Optional:
//! - `LEZER_E2E_PRIVATE=1`         also run shield + deshield (real proofs; minutes, ~4–9 GB RAM)
//! - `LEZER_E2E_TESTIMONIAL_ELF`   path to `lezer_testimonial.bin`: deploy, submit, verify-adoption
//! - `LEZER_E2E_TOKEN_ELF`         path to `lezer_token.bin`: define/mint/transfer/burn/authority

use std::io::Write as _;
use std::process::{Command, Stdio};

use serde_json::Value;

/// Upstream `testnet_initial_state::PRIVATE_KEY_PUB_ACC_A` (public test key, debug genesis only).
const GENESIS_KEY_A: [u8; 32] = [
    16, 162, 106, 154, 236, 125, 52, 184, 35, 100, 238, 174, 69, 197, 41, 77, 187, 10, 118, 75, 0, 11, 148, 238,
    185, 181, 133, 17, 220, 72, 124, 77,
];

struct Wallet {
    home: tempfile::TempDir,
    sequencer: String,
}

impl Wallet {
    fn new(sequencer: &str) -> Self {
        Self { home: tempfile::tempdir().unwrap(), sequencer: sequencer.to_string() }
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<Value, Value> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lezer"))
            .args(["--json", "--network", &self.sequencer])
            .args(args)
            .env("LEZER_HOME", self.home.path())
            .env("LEZER_PASSWORD", "e2e-password-123")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn lezer");
        if let Some(input) = stdin {
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
            panic!("non-JSON output for {args:?}: {}", String::from_utf8_lossy(&out.stdout))
        });
        if out.status.success() { Ok(v) } else { Err(v) }
    }

    fn ok(&self, args: &[&str]) -> Value {
        self.run(args, None).unwrap_or_else(|e| panic!("lezer {args:?} failed: {e}"))
    }

    fn balance(&self, account: &str) -> u128 {
        self.ok(&["balance", account])["balance"].as_str().unwrap().parse().unwrap()
    }
}

fn sequencer() -> Option<String> {
    std::env::var("LEZER_E2E_SEQUENCER").ok().filter(|s| !s.is_empty())
}

fn included(receipt: &Value) -> bool {
    receipt["status"]["state"] == "included"
}

#[test]
fn e2e_wallet_lifecycle_transfers_and_testimonials() {
    let Some(seq) = sequencer() else {
        eprintln!("skipping: set LEZER_E2E_SEQUENCER to run against a standalone sequencer");
        return;
    };
    let w = Wallet::new(&seq);

    // Network compatibility: our privacy circuit must equal the sequencer's.
    let net = w.ok(&["network"]);
    assert_eq!(net["privacy_circuit_matches"], true, "wallet/sequencer version mismatch: {net}");

    // Create wallet; recovery phrase returned once; vault is encrypted on disk.
    let init = w.ok(&["init"]);
    let phrase = init["recovery_phrase"].as_str().unwrap().to_string();
    assert_eq!(phrase.split_whitespace().count(), 24);
    let vault = std::fs::read_to_string(w.home.path().join("vault.enc")).unwrap();
    assert!(!vault.contains(phrase.split_whitespace().next().unwrap()) || !vault.contains(&phrase));

    // Fund from the debug-genesis account.
    w.run(&["account", "import", "--label", "genesis"], Some(&hex::encode(GENESIS_KEY_A))).expect("import");
    assert!(w.balance("genesis") > 0, "genesis account must be funded");
    let main_before = w.balance("main");

    // Real public transfer, reported only once included.
    let amount: u128 = 2_000_000_000;
    let r = w.ok(&["send", "--from", "genesis", "--to", "main", "--amount", &amount.to_string()]);
    assert!(included(&r), "public transfer not included: {r}");
    assert_eq!(w.balance("main"), main_before + amount);

    // Overspend is rejected honestly (no fake success).
    let err = w.run(&["send", "--from", "main", "--to", "genesis", "--amount", "999999999999999999"], None).unwrap_err();
    assert_eq!(err["error"]["code"], "LZR-3003", "{err}");

    // Restore from the phrase yields the same main account.
    let restored = Wallet::new(&seq);
    let r2 = restored.run(&["restore"], Some(&phrase)).expect("restore");
    let main_id = |v: &Value| {
        v["accounts"].as_array().unwrap().iter().find(|a| a["labels"][0] == "main").unwrap()["account_id"].clone()
    };
    assert_eq!(main_id(&r2), main_id(&init));

    if std::env::var("LEZER_E2E_PRIVATE").as_deref() == Ok("1") {
        let shield = w.ok(&["send", "--from", "main", "--to", "private", "--amount", "500000000"]);
        assert!(included(&shield), "shield not included: {shield}");
        assert_eq!(w.balance("private"), 500_000_000);
        let deshield = w.ok(&["send", "--from", "private", "--to", "main", "--amount", "200000000"]);
        assert!(included(&deshield), "deshield not included: {deshield}");
        assert_eq!(w.balance("private"), 300_000_000);
    }

    if let Ok(elf) = std::env::var("LEZER_E2E_TESTIMONIAL_ELF") {
        let d = w.ok(&["program", "deploy", &elf, "--payer", "genesis"]);
        let program = d["program_id"].as_str().unwrap().to_string();
        let local_image = w.ok(&["program", "image-id", &elf]);
        assert_eq!(d["image_id"], local_image["image_id"]);

        let t = w.ok(&["testimonial", "submit", "--program", &program, "--author", "main", "--username", "@e2e", "--text", "Lezer works end to end"]);
        assert!(included(&t), "testimonial not included: {t}");
        let dup = w.run(&["testimonial", "submit", "--program", &program, "--author", "main", "--text", "again"], None);
        assert!(dup.is_err(), "duplicate testimonial must be rejected");

        // Verifier runs WITHOUT any wallet state.
        let verifier = Wallet::new(&seq);
        let report = verifier.ok(&["verify-adoption", "--program", &program]);
        assert_eq!(report["total_testimonials"], 1, "{report}");
        assert_eq!(report["testimonials"][0]["text"], "Lezer works end to end");
    }

    if let Ok(elf) = std::env::var("LEZER_E2E_TOKEN_ELF") {
        let d = w.ok(&["program", "deploy", &elf, "--payer", "genesis"]);
        let program = d["program_id"].as_str().unwrap().to_string();

        w.ok(&["account", "new", "public", "--label", "tok-def"]);
        let accts = w.ok(&["account", "list"]);
        let def_id = accts
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["labels"].as_array().unwrap().iter().any(|l| l == "tok-def"))
            .unwrap()["account_id"]
            .as_str()
            .unwrap()
            .to_string();

        // define → mint → transfer → burn; all real program calls.
        let t = w.ok(&[
            "token", "define", "--program", &program, "--definition", "tok-def",
            "--name", "E2E Token", "--symbol", "E2E", "--decimals", "6",
            "--supply", "1000000", "--authority", "genesis", "--payer", "genesis",
        ]);
        assert_eq!(t["token_id"], def_id);
        assert_eq!(t["total_supply"], "1000000");

        let tbal = |w: &Wallet, owner: &str| -> u128 {
            w.ok(&["token", "balance", "--program", &program, "--token", &def_id, "--owner", owner])["balance"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap()
        };
        assert!(included(&w.ok(&[
            "token", "mint", "--program", &program, "--token", &def_id,
            "--authority", "genesis", "--to", "main", "--amount", "5000", "--payer", "genesis",
        ])));
        assert_eq!(tbal(&w, "main"), 5_000);
        w.ok(&["account", "new", "public", "--label", "bob"]);
        assert!(included(&w.ok(&[
            "token", "transfer", "--program", &program, "--token", &def_id,
            "--from", "main", "--to", "bob", "--amount", "2000", "--payer", "genesis",
        ])));
        assert_eq!(tbal(&w, "bob"), 2_000);
        assert_eq!(tbal(&w, "main"), 3_000);
        assert!(included(&w.ok(&[
            "token", "burn", "--program", &program, "--token", &def_id,
            "--from", "bob", "--amount", "500", "--payer", "genesis",
        ])));
        assert_eq!(tbal(&w, "bob"), 1_500);

        // Pre-flight rejections: unauthorized mint, overspend, re-definition — all local,
        // no charged revert should reach the chain.
        let err = w.run(&[
            "token", "mint", "--program", &program, "--token", &def_id,
            "--authority", "bob", "--to", "bob", "--amount", "1", "--payer", "genesis",
        ], None).unwrap_err();
        assert_eq!(err["error"]["code"], "LZR-3006", "{err}");
        let err = w.run(&[
            "token", "transfer", "--program", &program, "--token", &def_id,
            "--from", "bob", "--to", "main", "--amount", "99999999", "--payer", "genesis",
        ], None).unwrap_err();
        assert_eq!(err["error"]["code"], "LZR-3003", "{err}");
        let err = w.run(&[
            "token", "define", "--program", &program, "--definition", "tok-def",
            "--name", "Dupe", "--symbol", "DUP", "--supply", "1", "--payer", "genesis",
        ], None).unwrap_err();
        assert_eq!(err["error"]["code"], "LZR-3006", "{err}");

        // Authority rotation: bob mints after genesis delegates; after renounce nobody can.
        assert!(included(&w.ok(&[
            "token", "set-authority", "--program", &program, "--token", &def_id,
            "--authority", "genesis", "--new", "bob", "--payer", "genesis",
        ])));
        assert!(included(&w.ok(&[
            "token", "mint", "--program", &program, "--token", &def_id,
            "--authority", "bob", "--to", "bob", "--amount", "7", "--payer", "genesis",
        ])));
        assert!(included(&w.ok(&[
            "token", "set-authority", "--program", &program, "--token", &def_id,
            "--authority", "bob", "--new", "none", "--payer", "genesis",
        ])));
        let err = w.run(&[
            "token", "mint", "--program", &program, "--token", &def_id,
            "--authority", "bob", "--to", "bob", "--amount", "1", "--payer", "genesis",
        ], None).unwrap_err();
        assert_eq!(err["error"]["code"], "LZR-3006", "{err}");

        let info = w.ok(&["token", "info", "--program", &program, "--token", &def_id]);
        assert_eq!(info["authority"], Value::Null);
        assert_eq!(info["total_supply"], "1004507");
    }

    if let Ok(reg_elf) = std::env::var("LEZER_E2E_REGISTRY_ELF") {
        // Deploy the registry; self-register the testimonial program through a chained call.
        let d = w.ok(&["program", "deploy", &reg_elf, "--payer", "genesis"]);
        let registry = d["program_id"].as_str().unwrap().to_string();

        if std::env::var("LEZER_E2E_TESTIMONIAL_ELF").is_ok() {
            // testimonial was deployed earlier in this test; its program id is on record
            let r = w.ok(&[
                "registry", "register", "--program", &registry, "--subject", "lezer_testimonial",
                "--name", "Lezer Testimonial", "--source-url", "https://example.invalid/lezer",
                "--commit", "deadbeef", "--version", "0.3.1", "--payer", "genesis",
            ]);
            assert!(included(&r), "register_self not included: {r}");

            // Wallet-less directory read.
            let verifier = Wallet::new(&seq);
            let list = verifier.ok(&["registry", "list", "--program", &registry]);
            assert_eq!(list["total_programs"], 1, "{list}");
            assert_eq!(list["entries"][0]["name"], "Lezer Testimonial");
            // The entry is keyed by the *program's* account id — the chained-call sender.
            assert!(list["entries"][0]["program_id"].as_str().unwrap().starts_with("Public/"));
        }
    }
}
