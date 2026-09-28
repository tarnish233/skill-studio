fn main() {
    tauri_build::build();
    #[cfg(target_os = "macos")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_page_tools();
    }
}

#[cfg(target_os = "macos")]
fn build_page_tools() {
    use std::{env, path::PathBuf, process::Command};
    let source = "native/PageTools.swift";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let arch = match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("Unsupported macOS architecture: {other}"),
    };
    let minimum = "12.0"; // Keep in sync with bundle.macOS.minimumSystemVersion.
    swift_rs::SwiftLinker::new(minimum).link();
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-swift-version",
            "5",
            "-parse-as-library",
            "-emit-library",
            "-static",
        ])
        .args([
            "-module-name",
            "SkillStudioPageTools",
            "-target",
            &format!("{arch}-apple-macosx{minimum}"),
        ])
        .arg(if env::var("PROFILE").as_deref() == Ok("release") {
            "-O"
        } else {
            "-Onone"
        })
        .arg("-module-cache-path")
        .arg(output.join("swift-module-cache"))
        .arg(source)
        .arg("-o")
        .arg(output.join("libSkillStudioPageTools.a"))
        .status()
        .expect("Xcode's Swift compiler is required for the macOS toolbar");
    assert!(
        status.success(),
        "Failed to compile native macOS page tools"
    );
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=SkillStudioPageTools");
    println!("cargo:rustc-link-lib=framework=SwiftUI");
    // Swift system runtime is provided by macOS 12+, including bundled releases.
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}
