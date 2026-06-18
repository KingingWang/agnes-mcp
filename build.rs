use std::process::Command;

fn main() {
    // Build timestamp
    println!(
        "cargo:rustc-env=BUILD_TIMESTAMP={}",
        chrono::Utc::now().to_rfc3339()
    );

    // Git commit info
    let mut commit = String::from("unknown");
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        if output.status.success() {
            commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    println!("cargo:rustc-env=GIT_COMMIT={commit}");

    // Rust toolchain version
    let mut version = String::from("unknown");
    if let Ok(output) = Command::new("rustc").args(["--version"]).output() {
        if output.status.success() {
            version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
    }
    println!("cargo:rustc-env=RUST_VERSION={version}");

    // Re-run build.rs when HEAD advances (not just on branch switch).
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
        if let Some(reference) = head.strip_prefix("ref:") {
            let reference = reference.trim();
            if !reference.is_empty() {
                println!("cargo:rerun-if-changed=.git/{reference}");
            }
        }
    }
}
