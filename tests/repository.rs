#![cfg(feature = "native-cli")]

use md5::{Digest, Md5};
use serde_json::Value;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Output},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const ISSUER: &str = "https://example.test/";

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_interlis-compiler"))
}

fn report(command: &mut Command, exit: i32) -> Value {
    let output = command.arg("--json").output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn ili(edition: &str, name: &str, version: &str, body: &str) -> String {
    format!(
        "INTERLIS {edition};\nMODEL {name} AT \"{ISSUER}\" VERSION \"{version}\" =\n{body}\nEND {name}.\n"
    )
}

fn model_entry(name: &str, edition: &str, file: &str, version: &str, extra: &str) -> String {
    format!(
        "<ModelMetadata><Name>{name}</Name><SchemaLanguage>ili2_{edition}</SchemaLanguage><File>{file}</File><Version>{version}</Version>{extra}</ModelMetadata>"
    )
}

fn index(entries: &[String]) -> String {
    format!("<RepositoryIndex>{}</RepositoryIndex>", entries.concat())
}

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn expect_model(schema: &Value, name: &str, version: &str, source: &str) {
    let model = schema["schema"]["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["name"] == name)
        .unwrap();
    assert_eq!(model["version"], version);
    assert!(
        model["source"].as_str().unwrap().ends_with(source),
        "{model}"
    );
}

#[test]
fn check_uses_explicit_and_neighbour_models_and_leaves_request_mode_explicit() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    let main = dir.join("main.ili");
    let text = ili("2.3", "Main", "1", "IMPORTS Base;");
    write(dir, "main.ili", &text);
    write(
        dir,
        "elsewhere.ILI",
        ili("2.4", "Base", "near", "IMPORTS Leaf;"),
    );
    write(dir, "leaf.ili", ili("2.4", "Leaf", "1", ""));
    write(
        dir,
        "nested/duplicate.ili",
        ili("2.3", "Base", "hidden", ""),
    );
    write(
        dir,
        "nested/explicit.ili",
        ili("2.3", "Base", "explicit", ""),
    );
    write(dir, "repo/ilimodels.xml", index(&[]));

    let Output {
        status,
        stdout,
        stderr,
    } = cli().arg("check").arg(&main).output().unwrap();
    assert!(status.success());
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
    let check = report(cli().args(["check", "--offline"]).arg(&main), 0);
    assert_eq!(check["status"], "ok");
    assert_eq!(check["diagnostics"], serde_json::json!([]));
    assert!(check.get("schema").is_none());

    let discovered = report(cli().arg("compile").arg(&main), 0);
    expect_model(&discovered, "Base", "near", "/elsewhere.ILI");
    expect_model(&discovered, "Leaf", "1", "/leaf.ili");

    let explicit = report(
        cli()
            .arg("compile")
            .arg(&main)
            .arg(dir.join("nested/explicit.ili")),
        0,
    );
    expect_model(&explicit, "Base", "explicit", "/explicit.ili");
    let missing = report(
        cli()
            .args(["check", "--repository"])
            .arg(dir.join("repo"))
            .arg(&main),
        1,
    );
    assert_eq!(missing["diagnostics"][0]["code"], "ILI-IMPORT-NOT-FOUND");
    assert_eq!(missing["diagnostics"][0]["source"], "cli:///1/main.ili");
    assert_eq!(missing["diagnostics"][0]["startLine"], 3);

    write(
        dir,
        "request.json",
        serde_json::json!({"sources":[{"name":"original","text":text}]}).to_string(),
    );
    let explicit_request = report(
        cli()
            .args(["compile", "--request"])
            .arg(dir.join("request.json")),
        1,
    );
    assert_eq!(
        explicit_request["diagnostics"][0]["code"],
        "ILI-IMPORT-NOT-FOUND"
    );
    assert_eq!(explicit_request["diagnostics"][0]["source"], "original");
}

#[test]
fn repository_order_editions_version_chains_and_exact_translations() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    write(
        dir,
        "first.xml",
        index(&[
            model_entry("Base", "3", "base23.ili", "old-edition", ""),
            model_entry("Base", "4", "start.ili", "old", ""),
            model_entry(
                "Base",
                "4",
                "selected.ili",
                "new",
                "<precursorVersion>old</precursorVersion>",
            ),
            model_entry(
                "Base",
                "4",
                "missing.ili",
                "duplicate",
                "<precursorVersion>old</precursorVersion>",
            ),
            model_entry(
                "Base",
                "4",
                "missing.ili",
                "hidden",
                "<precursorVersion>new</precursorVersion><browseOnly>true</browseOnly>",
            ),
            model_entry(
                "Base",
                "4",
                "missing.ili",
                "disconnected",
                "<precursorVersion>ignored</precursorVersion>",
            ),
        ]),
    );
    write(
        dir,
        "second.xml",
        index(&[model_entry("Base", "4", "missing.ili", "second-index", "")]),
    );
    write(dir, "start.ili", ili("2.4", "Base", "old", ""));
    let mut selected = ili("2.4", "Base", "new", "IMPORTS Leaf;");
    selected.push_str(
        "MODEL Other AT \"https://example.test/\" VERSION \"1\" = IMPORTS Missing; END Other.\n",
    );
    write(dir, "selected.ili", selected);
    write(dir, "leaf.ili", ili("2.4", "Leaf", "1", ""));
    write(
        dir,
        "leaf.xml",
        index(&[model_entry("Leaf", "4", "leaf.ili", "1", "")]),
    );
    write(dir, "base23.ili", ili("2.3", "Base", "old-edition", ""));
    write(dir, "main.ili", ili("2.3", "Main", "1", "IMPORTS Base;"));
    write(
        dir,
        "translated.ili",
        "INTERLIS 2.4; MODEL Translated AT \"https://example.test/\" VERSION \"1\" TRANSLATION OF Base [\"old\"] = END Translated.",
    );

    let locations = format!(
        "{};{};{}",
        dir.join("first.xml").display(),
        dir.join("second.xml").display(),
        dir.join("leaf.xml").display()
    );
    let query = |name: &str| {
        let mut command = cli();
        command.args(["compile", "--repositories", &locations, "--root", name]);
        command
    };
    let chosen = report(&mut query("Base"), 0);
    expect_model(&chosen, "Base", "new", "/selected.ili");
    expect_model(&chosen, "Leaf", "1", "/leaf.ili");
    assert_eq!(chosen["schema"]["models"].as_array().unwrap().len(), 2);
    assert!(chosen["diagnostics"].as_array().unwrap().iter().any(|d| {
        d["message"]
            .as_str()
            .unwrap()
            .contains("duplicate successor")
    }));

    let imported = report(
        cli()
            .args(["compile", "--repositories", &locations])
            .arg(dir.join("main.ili")),
        0,
    );
    expect_model(&imported, "Base", "old-edition", "/base23.ili");
    let translated = report(
        cli()
            .args(["compile", "--repositories", &locations])
            .arg(dir.join("translated.ili")),
        0,
    );
    expect_model(&translated, "Base", "old", "/start.ili");

    let early = dir.join("early.xml");
    write(
        dir,
        "early.xml",
        index(&[model_entry("Base", "3", "base23.ili", "old-edition", "")]),
    );
    let earlier = report(
        cli().args(["compile", "--repository"]).arg(early).args([
            "--repositories",
            &locations,
            "--root",
            "Base",
        ]),
        0,
    );
    expect_model(&earlier, "Base", "old-edition", "/base23.ili");
}

#[test]
fn site_graph_is_ordered_and_selected_sources_fail_without_fallback() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    for repository in ["start", "parent", "child"] {
        write(dir, &format!("{repository}/ilimodels.xml"), index(&[]));
    }
    write(
        dir,
        "start/ilisite.xml",
        "<Site><parentSite><RepositoryLocation_><value>../parent/</value></RepositoryLocation_></parentSite><subsidiarySite><RepositoryLocation_><value>../child/</value></RepositoryLocation_></subsidiarySite></Site>",
    );
    write(
        dir,
        "parent/ilisite.xml",
        "<Site><parentSite><RepositoryLocation_><value>../start/</value></RepositoryLocation_></parentSite></Site>",
    );
    let selected = model_entry("RootModel", "4", "unrelated-name.ili", "one", "");
    write(dir, "parent/ilimodels.xml", index(&[selected]));
    write(
        dir,
        "child/ilimodels.xml",
        index(&[
            model_entry("RootModel", "4", "child.ili", "child", ""),
            model_entry("OnlyChild", "4", "child.ili", "child", ""),
        ]),
    );
    write(
        dir,
        "child/child.ili",
        format!(
            "{}MODEL OnlyChild AT \"{ISSUER}\" VERSION \"child\" = END OnlyChild.",
            ili("2.4", "RootModel", "child", "")
        ),
    );

    let query = |model: &str| {
        let mut command = cli();
        command
            .args(["compile", "--root", model, "--repository"])
            .arg(dir.join("start"));
        command
    };
    write(
        dir,
        "parent/unrelated-name.ili",
        ili("2.4", "RootModel", "one", ""),
    );
    expect_model(
        &report(&mut query("RootModel"), 0),
        "RootModel",
        "one",
        "/parent/unrelated-name.ili",
    );
    expect_model(
        &report(&mut query("OnlyChild"), 0),
        "OnlyChild",
        "child",
        "/child/child.ili",
    );

    write(
        dir,
        "parent/unrelated-name.ili",
        ili("2.4", "RootModel", "wrong", ""),
    );
    let wrong = report(&mut query("RootModel"), 1);
    assert_eq!(wrong["diagnostics"][0]["code"], "ILI-RES-VERSION-MISMATCH");
    assert!(
        wrong["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("/parent/unrelated-name.ili")
    );
    write(dir, "parent/unrelated-name.ili", [0xff]);
    let invalid = report(&mut query("RootModel"), 1);
    assert_eq!(invalid["diagnostics"][0]["code"], "ILI-RES-INVALID-SOURCE");
    fs::remove_file(dir.join("parent/unrelated-name.ili")).unwrap();
    let unavailable = report(&mut query("RootModel"), 3);
    assert_eq!(unavailable["diagnostics"][0]["code"], "ILI-RES-LOAD");
    let missing = report(&mut query("Absent"), 1);
    assert_eq!(missing["diagnostics"][0]["code"], "ILI-ROOT-NOT-FOUND");
    let offline = report(query("OnlyChild").arg("--offline"), 3);
    assert_eq!(offline["diagnostics"][0]["code"], "ILI-RES-OFFLINE");
}

#[test]
fn http_cache_checks_age_integrity_and_offline_without_accessing_the_server() {
    let root = tempfile::tempdir().unwrap();
    let cache = root.path().join("cache");
    let source = ili("2.4", "RootModel", "2025-02-01", "");
    let md5 = format!("{:x}", Md5::digest(source.as_bytes()));
    let listing = index(&[model_entry(
        "RootModel",
        "4",
        "model.ili",
        "2025-02-01",
        &format!("<md5>{md5}</md5>"),
    )]);
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let base = format!("http://{}/", server.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let visited = requests.clone();
    let (stop, stopped) = std::sync::mpsc::channel();
    let body = source.clone();
    let listener = thread::spawn(move || {
        while stopped.try_recv() == Err(std::sync::mpsc::TryRecvError::Empty) {
            let mut stream = match server.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let path = line.split_whitespace().nth(1).unwrap().to_owned();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            visited.lock().unwrap().push(path.clone());
            let (status, header, bytes) = match path.as_str() {
                "/ilimodels.xml" => ("200 OK", "", listing.as_str()),
                "/model.ili" => ("302 Found", "Location: /original.ili\r\n", ""),
                "/original.ili" => ("200 OK", "", body.as_str()),
                _ => ("404 Not Found", "", ""),
            };
            write!(stream, "HTTP/1.1 {status}\r\n{header}Content-Length: {}\r\nConnection: close\r\n\r\n{bytes}", bytes.len()).unwrap();
        }
    });
    let query = |options: &[&str]| {
        let mut cmd = cli();
        cmd.args(["compile", "--root", "RootModel", "--repository", &base])
            .args(options);
        cmd
    };
    let cached = |offline: bool| {
        let mut command = query(if offline { &["--offline"] } else { &[] });
        command.arg("--cache").arg(&cache);
        command
    };
    let initial = report(&mut cached(false), 0);
    expect_model(&initial, "RootModel", "2025-02-01", "/model.ili");
    let three = ["/ilimodels.xml", "/model.ili", "/original.ili"];
    assert_eq!(*requests.lock().unwrap(), three);
    report(&mut cached(false), 0);
    report(&mut cached(true), 0);
    assert_eq!(*requests.lock().unwrap(), three);

    // Change the Java-format metadata dates, keeping the actual cache payloads.
    for file in fs::read_dir(cache.join("metadata-v2")).unwrap() {
        let path = file.unwrap().path();
        let xml = fs::read_to_string(&path).unwrap();
        let (prefix, rest) = xml.split_once("downloaded-at=\"").unwrap();
        let (_, tail) = rest.split_once('"').unwrap();
        let date = (time::OffsetDateTime::now_utc() - time::Duration::days(2))
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        fs::write(path, format!("{prefix}downloaded-at=\"{date}\"{tail}")).unwrap();
    }
    report(&mut cached(false), 0);
    assert_eq!(
        *requests.lock().unwrap(),
        [three.as_slice(), &["/ilimodels.xml"]].concat()
    );

    for file in fs::read_dir(&cache).unwrap() {
        let path = file.unwrap().path();
        if path.is_file() && fs::read(&path).unwrap() == source.as_bytes() {
            fs::write(path, b"corrupt").unwrap();
        }
    }
    let miss = report(&mut cached(true), 3);
    assert_eq!(miss["diagnostics"][0]["code"], "ILI-RES-OFFLINE");
    assert_eq!(requests.lock().unwrap().len(), 4);
    report(&mut cached(false), 0);
    report(&mut query(&["--no-cache"]), 0);
    assert_eq!(requests.lock().unwrap().len(), 9);
    report(&mut query(&["--offline", "--no-cache"]), 3);
    assert_eq!(requests.lock().unwrap().len(), 9);
    stop.send(()).unwrap();
    listener.join().unwrap();
}
