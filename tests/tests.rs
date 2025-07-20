//! Runs unit tests with different feature flags.
//!
//! Some tests will adapt to the selected async runtime automatically. This test exercises them.

use std::{env, process::Command};

fn test(args: &[&str]) {
    let cargo = env::var_os("CARGO").expect("`CARGO` isn't set");
    let status = Command::new(cargo)
        .args(&["test", "-p", "evdevil", "--lib"]) // avoid infinite recursion
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
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
