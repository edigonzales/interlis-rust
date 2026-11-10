#![cfg(feature = "native-cli")]
use std::{fs, process::Command};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_interlis-compiler"))
}

#[test]
fn real_cli_streams_files_and_request_errors() {
    let root = std::env::temp_dir().join(format!("interlis cli {}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source with spaces.ili");
    fs::write(&source, "INTERLIS 2.3;").unwrap();
    let out = binary()
        .args(["probe", "--json"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(out.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["status"], "incomplete");
    assert_eq!(report["fullValidation"], false);
    assert!(report["model"].is_null());
    let duplicate = binary()
        .arg("probe")
        .arg(&source)
        .arg(root.join("./source with spaces.ili"))
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(duplicate.stdout.is_empty());
    let missing = binary()
        .arg("probe")
        .arg(root.join("missing.ili"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    assert!(missing.stdout.is_empty());
    fs::write(&source, [0xff]).unwrap();
    let invalid = binary().arg("probe").arg(&source).output().unwrap();
    assert_eq!(invalid.status.code(), Some(3));
    assert!(String::from_utf8(invalid.stderr).unwrap().contains("UTF-8"));
    let request = root.join("input.json");
    fs::write(
        &request,
        r#"{"sources":[{"name":"a","text":""}],"limits":{"max_source_bytes":0}}"#,
    )
    .unwrap();
    let bad = binary()
        .args(["probe", "--json", "--request"])
        .arg(&request)
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert!(bad.stdout.is_empty());
    fs::write(
        &request,
        r#"{"sources":[{"name":"a","text":"ab"}],"limits":{"max_source_bytes":1}}"#,
    )
    .unwrap();
    let limited = binary()
        .args(["probe", "--json", "--request"])
        .arg(&request)
        .output()
        .unwrap();
    assert_eq!(limited.status.code(), Some(4));
    let report: serde_json::Value = serde_json::from_slice(&limited.stdout).unwrap();
    assert_eq!(report["status"], "limit_exceeded");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn version_and_help_and_usage() {
    let out = binary().arg("version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        interlis::compiler_version()
    );
    let out = binary().arg("help").output().unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("experimental")
    );
    for args in [
        vec!["check"],
        vec!["probe"],
        vec!["probe", "--request"],
        vec!["version", "extra"],
    ] {
        assert_eq!(binary().args(args).output().unwrap().status.code(), Some(2));
    }
}
