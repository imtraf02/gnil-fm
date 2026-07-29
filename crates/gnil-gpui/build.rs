#![allow(clippy::disallowed_methods, reason = "build scripts are exempt")]

use std::{path::Path, process};

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gles)");
    check_wgsl_shaders();
}

fn check_wgsl_shaders() {
    let shader_path = Path::new("src/platform/blade/shaders.wgsl");
    println!("cargo:rerun-if-changed={}", shader_path.display());

    let shader_source = std::fs::read_to_string(shader_path).unwrap();
    if let Err(error) = naga::front::wgsl::parse_str(&shader_source) {
        println!("cargo::error=WGSL shader compilation failed:\n{error}");
        process::exit(1);
    }
}
