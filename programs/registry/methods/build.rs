fn main() {
    println!("cargo:rerun-if-changed=guest/src/bin/registry.rs");
}
