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
    fs::write(
        &source,
        "INTERLIS 2.3; MODEL A AT \"x\" VERSION \"1\" = END A.",
    )
    .unwrap();
    let out = binary()
        .args(["probe", "--json"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["status"], "checked_subset");
    assert_eq!(report["fullValidation"], false);
    assert_eq!(report["model"]["models"][0]["name"], "A");
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

#[test]
fn file_sources_have_portable_names_and_detect_physical_duplicates() {
    let root = std::env::temp_dir().join(format!("interlis identities {}", std::process::id()));
    fs::create_dir_all(root.join("other")).unwrap();
    let name = "probe % ä.ili";
    let source = root.join(name);
    fs::write(
        &source,
        "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = END A.",
    )
    .unwrap();
    let relative = binary()
        .current_dir(&root)
        .args(["probe", "--json", name])
        .output()
        .unwrap();
    let absolute = binary()
        .args(["probe", "--json"])
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(relative.status.code(), Some(0));
    assert_eq!(relative.stdout, absolute.stdout);
    let json = String::from_utf8(relative.stdout).unwrap();
    assert!(!json.contains(root.to_str().unwrap()));
    let report: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        report["model"]["declarations"][0]["source"],
        "cli:///1/probe%20%25%20%C3%A4.ili"
    );
    let hardlink = root.join("hardlink.ili");
    fs::hard_link(&source, &hardlink).unwrap();
    let duplicate = binary()
        .args(["probe", "--json"])
        .arg(&source)
        .arg(&hardlink)
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(duplicate.stdout.is_empty());
    assert!(
        String::from_utf8(duplicate.stderr)
            .unwrap()
            .contains("Duplicate physical")
    );
    #[cfg(unix)]
    {
        let symlink = root.join("symlink.ili");
        std::os::unix::fs::symlink(&source, &symlink).unwrap();
        assert_eq!(
            binary()
                .arg("probe")
                .arg(&source)
                .arg(&symlink)
                .output()
                .unwrap()
                .status
                .code(),
            Some(2)
        );
    }
    // Equal bytes in different files are not a duplicate identity: the core
    // should diagnose their actual duplicate model names, with separate origins.
    let copy = root.join("other").join(name);
    fs::copy(&source, &copy).unwrap();
    let distinct = binary()
        .args(["probe", "--json"])
        .arg(&source)
        .arg(&copy)
        .output()
        .unwrap();
    assert_eq!(distinct.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&distinct.stdout).unwrap();
    assert_eq!(
        report["diagnostics"][0]["source"],
        "cli:///2/probe%20%25%20%C3%A4.ili"
    );
    assert_eq!(
        report["diagnostics"][0]["related"][0]["source"],
        "cli:///1/probe%20%25%20%C3%A4.ili"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cli_outcomes_are_json_reports_and_text_diagnostics_have_logical_locations() {
    let root = std::env::temp_dir().join(format!("interlis outcomes {}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("probe.ili");
    for (text, expected, status) in [
        (
            "INTERLIS 2.3; MODEL A AT \"x\" VERSION \"1\" = END B.",
            1,
            "rejected",
        ),
        (
            "INTERLIS 2.3; MODEL A AT \"x\" VERSION \"1\" = DOMAIN E = (a,b); END A.",
            4,
            "incomplete",
        ),
        (
            "INTERLIS 2.3; MODEL A AT \"x\" VERSION \"1\" = IMPORTS Missing; END A.",
            1,
            "rejected",
        ),
    ] {
        fs::write(&source, text).unwrap();
        let out = binary()
            .args(["probe", "--json"])
            .arg(&source)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(expected));
        assert!(out.stderr.is_empty());
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["status"], status);
        assert!(report["model"].is_null());
        let plain = binary().arg("probe").arg(&source).output().unwrap();
        assert_eq!(plain.status.code(), Some(expected));
        let diagnostics = String::from_utf8(plain.stderr).unwrap();
        assert!(diagnostics.contains("cli:///1/probe.ili:1:"));
        assert!(!diagnostics.contains(root.to_str().unwrap()));
    }
    fs::remove_dir_all(root).unwrap();
}
