//! Compiles the GResource bundle (UI files, stylesheets, icons) into the
//! binary, and the GSettings schema into OUT_DIR so that `cargo run` and
//! `cargo test` work without installing anything.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    glib_build_tools::compile_resources(
        &["data"],
        "data/resources.gresource.xml",
        "finaryx.gresource",
    );

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let status = Command::new("glib-compile-schemas")
        .arg("--strict")
        .arg("--targetdir")
        .arg(&out)
        .arg("data")
        .status()
        .expect("glib-compile-schemas is required to build Finaryx");
    assert!(status.success(), "invalid GSettings schema");
    println!("cargo:rerun-if-changed=data/io.github.docteurscience.Finaryx.gschema.xml");
    println!("cargo:rustc-env=FINARYX_DEV_SCHEMA_DIR={}", out.display());

    for var in ["FINARYX_APP_ID", "FINARYX_LOCALEDIR", "FINARYX_PROFILE"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
}
