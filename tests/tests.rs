//! Runs unit tests with different feature flags.
//!
//! Some tests will adapt to the selected async runtime automatically. This test exercises them.

use std::{env, ffi::OsString, process::Command};

fn test(args: &[&str]) {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let status = Command::new(cargo)
        .args(&["test", "-p", "evdevil", "--lib"]) // avoid infinite recursion
        .args(args)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "cargo exited with error code {:?}",
        status.code(),
    );
}

fn main() {
    test(&["--no-default-features"]);
    test(&["--features", "serde"]);
    test(&["--features", "tokio"]);
    test(&["--features", "async-io"]);
}
