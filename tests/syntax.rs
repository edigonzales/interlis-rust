use interlis::{
    AnalysisStatus, CompileRequest, Limits, SourceInput, SyntaxStatus, analyze, parse, parse_json,
    parse_request_json,
};
use serde_json::{Value, json};

fn source(name: &str, text: &str) -> SourceInput {
    SourceInput {
        name: name.into(),
        text: text.into(),
    }
}

#[test]
fn syntax_reports_original_document_and_model_order_without_binding() {
    let body = "TYPE MODEL A AT \"x\" VERSION \"é😀\" = IMPORTS Missing; END Wrong. MODEL A AT \"y\" VERSION \"2\" = END A.";
    let text = format!("\u{feff}!! 😀\r\nINTERLIS 2.4;\r\n{body}");
    let request = json!({"sources": [
        {"name": "z-😀", "text": text},
        {"name": "a", "text": "INTERLIS 2.3;"}
    ]})
    .to_string();
    let report = parse_request_json(&request).unwrap();
    assert_eq!(report.status(), SyntaxStatus::Parsed);
    assert_eq!(report.sources().by_name("z-😀").unwrap().text(), text);
    assert!(report.diagnostics().is_empty());
    assert_eq!(parse_json(&request).unwrap(), report.to_json().unwrap());
    let output: Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    assert_eq!(output["validationScope"], "syntax");
    assert_eq!(output["fullValidation"], false);
    assert!(output.get("model").is_none());
    let documents = output["documents"].as_array().unwrap();
    assert_eq!(documents.len(), 2);
    assert_eq!(
        documents[0],
        json!({"source": "a", "languageVersion": "2.3", "models": []})
    );
    let models = documents[1]["models"].as_array().unwrap();
    assert_eq!(documents[1]["source"], "z-😀");
    assert_eq!(documents[1]["languageVersion"], "2.4");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["name"], "A");
    assert_eq!(models[0]["kind"], "type");
    assert_eq!(models[0]["version"], "é😀");
    assert_eq!(models[0]["startByte"], text.find("TYPE MODEL").unwrap());
    assert_eq!(models[0]["startLine"], 3);
    assert_eq!(models[0]["startColumnUtf16"], 1);
    let model_end = body.find(" MODEL A AT \"y\"").unwrap();
    assert_eq!(
        models[0]["endColumnUtf16"],
        body[..model_end].encode_utf16().count() + 1
    );
    assert_eq!(models[1]["name"], "A");
    assert_eq!(models[1]["kind"], "data");
    assert_eq!(models[1]["version"], "2");

    let text = "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = IMPORTS Missing; END A.";
    let model = analyze(CompileRequest {
        sources: vec![source("bad-model", text)],
        limits: Limits::default(),
    })
    .unwrap();
    assert_eq!(model.status(), AnalysisStatus::Rejected);
    assert!(
        model
            .diagnostics()
            .iter()
            .any(|d| d.code == "ILI-IMPORT-NOT-FOUND")
    );
}

#[test]
fn failure_priorities_hide_all_partial_documents() {
    let foreign = "INTERLIS 2.5; ? \"foreign syntax";
    let good = "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = END A.";
    let broken = "INTERLIS 2.4; MODEL B AT \"x\" VERSION \"1\" = DOMAIN Broken = ; END B.";
    for (sources, limits, status, codes) in [
        (
            vec![source("a", good), source("b", foreign)],
            Limits::default(),
            SyntaxStatus::UnsupportedVersion,
            vec!["ILI-UNSUPPORTED"],
        ),
        (
            vec![source("a", foreign), source("b", good), source("c", broken)],
            Limits::default(),
            SyntaxStatus::Invalid,
            vec!["ILI-UNSUPPORTED", "ILI-SYNTAX"],
        ),
        (
            vec![source("a", foreign), source("b", broken)],
            Limits {
                max_diagnostics: 1,
                ..Limits::default()
            },
            SyntaxStatus::LimitExceeded,
            vec!["ILI-LIMIT"],
        ),
    ] {
        let result = parse(CompileRequest { sources, limits }).unwrap();
        assert_eq!(result.status(), status);
        assert_eq!(
            result
                .diagnostics()
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>(),
            codes
        );
        let value: Value = serde_json::from_str(&result.to_json().unwrap()).unwrap();
        assert_eq!(value["documents"], Value::Null);
        assert!(value.get("documents").is_some());
    }

    let rich = include_str!("fixtures/syntax/views-graphics24.ili");
    let request = || CompileRequest {
        sources: vec![source("rich", rich)],
        limits: Limits::default(),
    };
    assert_eq!(parse(request()).unwrap().status(), SyntaxStatus::Parsed);
    let report = analyze(request()).unwrap();
    assert_eq!(report.status(), AnalysisStatus::Rejected);
    assert!(
        report
            .diagnostics()
            .iter()
            .any(|d| d.code == "ILI-NAME-NOT-FOUND")
    );
}

#[test]
fn syntax_requests_and_report_size_fail_at_their_boundaries() {
    for input in [
        "{",
        r#"{"sources":[],"sources":[]}"#,
        r#"{"sources":[],"unknown":0}"#,
    ] {
        assert_eq!(parse_json(input).unwrap_err().code(), "REQUEST_JSON");
    }
    assert_eq!(
        parse_json(&" ".repeat(16 * 1024 * 1024 + 1))
            .unwrap_err()
            .code(),
        "REQUEST_WIRE_SIZE"
    );
    let text = format!(
        "INTERLIS 2.4; {}",
        "MODEL A AT \"x\" VERSION \"1\" = END A. ".repeat(32_000)
    );
    let name = "a".repeat(1024);
    let report = parse(CompileRequest {
        sources: vec![source(&name, &text)],
        limits: Limits {
            max_tokens: 500_000,
            ..Limits::default()
        },
    })
    .unwrap();
    assert_eq!(
        report.status(),
        SyntaxStatus::Parsed,
        "{:?}",
        report.diagnostics()
    );
    assert_eq!(report.to_json().unwrap_err().code(), "REPORT_WIRE_SIZE");
    assert_eq!(report.sources().by_name(&name).unwrap().text(), text);
}
