//! Build script for `lezer-adapter`.
//!
//! Wires `LBC_ROOT_DIR` (prebuilt `logos-blockchain-circuits` v0.5.3) and
//! `RAPIDSNARK_LIB_DIR` (prebuilt `rapidsnark` v0.0.8) environment variables
//! and handles platform-specific static link requirements.

fn main() {
    println!("cargo:rerun-if-env-changed=LBC_ROOT_DIR");
    println!("cargo:rerun-if-env-changed=RAPIDSNARK_LIB_DIR");
    println!("cargo:rerun-if-env-changed=LEZ_SEQUENCER_URL");

    if let Ok(lbc_dir) = std::env::var("LBC_ROOT_DIR") {
        println!("cargo:rustc-env=LEZER_BUILD_LBC_ROOT_DIR={lbc_dir}");
    }
    if let Ok(rs_dir) = std::env::var("RAPIDSNARK_LIB_DIR") {
        println!("cargo:rustc-env=LEZER_BUILD_RAPIDSNARK_LIB_DIR={rs_dir}");
    }

    // The MinGW-w64 rapidsnark archive links against mman-win32's libmman.a.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=static=mman");
    }
}
