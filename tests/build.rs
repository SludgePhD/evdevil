use std::{env, process::Command};

fn test(args: &[&str]) {
    let cargo = env::var_os("CARGO").expect("`CARGO` isn't set");
    let status = Command::new(cargo)
        .args(&["test", "-p", "evdevil", "--lib"])
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
