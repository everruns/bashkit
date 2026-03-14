use std::{env, fs, path::Path, process::Command};

/// Build script: sets up napi bindings and syncs package.json version
/// with the Cargo workspace version.
fn main() {
    println!("cargo:rerun-if-changed=package.json");
    sync_package_json_version();
    napi_build::setup();
}

/// Read the Cargo package version and update package.json if different.
fn sync_package_json_version() {
    let cargo_version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");
    let package_json_path = Path::new("package.json");

    let Ok(contents) = fs::read_to_string(package_json_path) else {
        return;
    };

    let expected = format!("  \"version\": \"{cargo_version}\",");
    let mut result = String::with_capacity(contents.len());
    let mut changed = false;

    for line in contents.lines() {
        if !changed && line.starts_with("  \"version\"") {
            if line == expected {
                return;
            }
            result.push_str(&expected);
            changed = true;
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    if !changed {
        return;
    }

    eprintln!("Updating package.json version to {cargo_version}");
    fs::write(package_json_path, &result).expect("failed to write package.json");

    let status = Command::new("npm")
        .args(["install", "--package-lock-only"])
        .status()
        .expect("failed to run npm");
    assert!(status.success(), "npm install --package-lock-only failed");
}
