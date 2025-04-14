use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_cli_help() {
    let mut cmd = Command::cargo_bin("veda").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: veda <COMMAND>"))
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains("start"))
        .stdout(predicate::str::contains("chat"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("stop"))
        .stdout(predicate::str::contains("Options:"))
        .stdout(predicate::str::contains("--help"))
        .stdout(predicate::str::contains("--version"));
}

#[test]
fn test_cli_start_help() {
    let mut cmd = Command::cargo_bin("veda").unwrap();
    cmd.arg("start")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: veda start"))
        .stdout(predicate::str::contains("Options:"))
        .stdout(predicate::str::contains("--prompt <PROMPT>"))
        .stdout(predicate::str::contains("--port <PORT>"))
        .stdout(predicate::str::contains("--help"));
}

#[test]
fn test_cli_chat_help() {
     let mut cmd = Command::cargo_bin("veda").unwrap();
     cmd.arg("chat")
         .arg("--help")
         .assert()
         .success()
         .stdout(predicate::str::contains("Usage: veda chat")); // Chat has no specific options yet
}

#[test]
fn test_cli_set_help() {
     let mut cmd = Command::cargo_bin("veda").unwrap();
     cmd.arg("set")
         .arg("--help")
         .assert()
         .success()
         .stdout(predicate::str::contains("Usage: veda set <COMMAND>"))
         .stdout(predicate::str::contains("Commands:"))
         .stdout(predicate::str::contains("instances"));
}

#[test]
fn test_cli_set_instances_help() {
     let mut cmd = Command::cargo_bin("veda").unwrap();
     cmd.arg("set")
         .arg("instances")
         .arg("--help")
         .assert()
         .success()
         .stdout(predicate::str::contains("Usage: veda set instances <VALUE>"));
}

#[test]
fn test_cli_stop_help() {
     let mut cmd = Command::cargo_bin("veda").unwrap();
     cmd.arg("stop")
         .arg("--help")
         .assert()
         .success()
         .stdout(predicate::str::contains("Usage: veda stop")); // Stop has no specific options yet
}

#[test]
fn test_cli_no_command() {
    // Running without a command should show help/usage
    let mut cmd = Command::cargo_bin("veda").unwrap();
    cmd.assert()
        .failure() // clap exits with non-zero status when no command is given
        .stderr(predicate::str::contains("Usage: veda <COMMAND>"));
}

// Note: Testing `start` command execution requires more setup (mocking servers, checking logs, etc.)
// and might be better suited for end-to-end tests.
