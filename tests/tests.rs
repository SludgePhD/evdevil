//! Runs unit tests with different feature flags.
//!
//! Some tests will adapt to the selected async runtime automatically. This test exercises them.

use std::{env, ffi::OsString, process::Command};

fn test(args: &[&str]) {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let output = Command::new(cargo)
        .args(&["test", "-p", "evdevil", "--lib"]) // avoid infinite recursion
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cargo exited with error code {:?}. stderr:\n{}\nstdout: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
fn no_default_features() {
    test(&["--no-default-features"]);
}

#[test]
fn serde() {
    test(&["--features", "serde"]);
}

#[test]
fn tokio() {
    test(&["--features", "tokio"]);
}

#[test]
fn async_io() {
    test(&["--features", "async-io"]);
}
