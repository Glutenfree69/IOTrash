fn main() {
    embuild::espidf::sysenv::output();

    // lora-phy hard-depends on defmt, whose linker script (defmt.x) only
    // exists when defmt is actually compiled — i.e. when the `lora` feature
    // is enabled. Adding the flag unconditionally (e.g. in .cargo/config.toml)
    // breaks mock builds with "cannot open linker script file defmt.x".
    if std::env::var_os("CARGO_FEATURE_LORA").is_some() {
        println!("cargo:rustc-link-arg=-Tdefmt.x");
    }
}
