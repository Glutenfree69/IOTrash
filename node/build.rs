fn main() {
    embuild::espidf::sysenv::output();

    // lora-phy hard-depends on defmt, whose linker script (defmt.x) only
    // exists when defmt is compiled. The node always builds with LoRa, so the
    // flag is unconditional here (unlike the gateway's feature-gated version).
    println!("cargo:rustc-link-arg=-Tdefmt.x");
}
