use std::{env, fs, path::PathBuf};

fn main() {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../src/modem");
    let target = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("modem");
    // Preserve each module's directory structure, including nested mod
    // declarations and include! paths, without a second checked-in codec.
    for file in [
        "bw500.rs",
        "bw500/scan.rs",
        "bw500_level4.rs",
        "bw500_waveform.rs",
        "coding.rs",
        "control.rs",
        "data.rs",
        "receiver.rs",
        "setup.rs",
        "waveform.rs",
        "waveform/bw500.rs",
        "wide_allocations.rs",
        "wide_level4.rs",
    ] {
        let input = source.join(file);
        let output = target.join(file);
        println!("cargo:rerun-if-changed={}", input.display());
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::copy(input, output).unwrap();
    }
    fs::write(
        target.join("mod.rs"),
        "pub mod bw500;\nmod bw500_level4;\nmod bw500_waveform;\nmod coding;\n\
         pub mod control;\npub mod data;\npub mod receiver;\nmod setup;\n\
         pub mod waveform;\npub mod wide_level4;\n",
    )
    .unwrap();
}
