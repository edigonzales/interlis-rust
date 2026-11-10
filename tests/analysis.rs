use interlis::{
    AnalysisReport, AnalysisStatus as Status, CompileRequest, DeclarationKind, LanguageVersion,
    Limits, SourceInput, TypeInfo, analyze,
};
const BASE: &str = r#"INTERLIS 2.3;
MODEL ProbeBase (en) AT "https://example.invalid" VERSION "2026-11-10" =
 DOMAIN Label=TEXT*40; Count=0..100;
END ProbeBase."#;
const DATA: &str = r#"INTERLIS 2.3;
MODEL ProbeData (en) AT "https://example.invalid" VERSION "2026-11-10" =
 IMPORTS ProbeBase;
 TOPIC Items = CLASS Item =
  label:MANDATORY ProbeBase.Label;
  count:ProbeBase.Count;
 END Item; END Items;
END ProbeData."#;
fn request(sources: Vec<(&str, String)>) -> CompileRequest {
    CompileRequest {
        sources: sources
            .into_iter()
            .map(|(name, text)| SourceInput {
                name: name.into(),
                text,
            })
            .collect(),
        limits: Limits::default(),
    }
}
fn pair(version: &str) -> CompileRequest {
    request(vec![
        (
            "memory:///a-not-the-model-name.ili",
            BASE.replacen("2.3", version, 1),
        ),
        ("memory:///z-data.ili", DATA.replacen("2.3", version, 1)),
    ])
}
fn single(body: &str) -> CompileRequest {
    request(vec![(
        "memory:///single.ili",
        format!(
            "INTERLIS 2.4; MODEL M AT \"https://example.invalid\" VERSION \"1\" = {body} END M."
        ),
    )])
}
fn outcome(input: CompileRequest, status: Status, code: Option<&str>) -> AnalysisReport {
    let report = analyze(input).unwrap();
    assert_eq!(report.status(), status, "{:?}", report.diagnostics());
    assert_eq!(report.graph().is_some(), status == Status::CheckedSubset);
    if let Some(code) = code {
        assert!(
            report.diagnostics().iter().any(|d| d.code == code),
            "{:?}",
            report.diagnostics()
        );
    }
    report
}
#[test]
fn two_file_subset_has_owned_metadata_members_types_and_origins_in_both_versions() {
    for (version, language) in [("2.3", LanguageVersion::V23), ("2.4", LanguageVersion::V24)] {
        let report = outcome(pair(version), Status::CheckedSubset, None);
        assert!(report.diagnostics().is_empty());
        let graph = report.graph().unwrap();
        assert_eq!(
            graph.models().iter().map(|m| m.name()).collect::<Vec<_>>(),
            ["ProbeBase", "ProbeData"]
        );
        assert_eq!(graph.models()[1].imports(), ["ProbeBase"]);
        for model in graph.models() {
            assert_eq!(model.language(), language);
            assert_eq!(model.language_tag(), Some("en"));
            assert_eq!(model.issuer(), "https://example.invalid");
            assert_eq!(model.version(), "2026-11-10");
        }
        assert_eq!(
            graph
                .declarations()
                .iter()
                .map(|d| d.qualified_name())
                .collect::<Vec<_>>(),
            [
                "ProbeBase",
                "ProbeBase.Label",
                "ProbeBase.Count",
                "ProbeData",
                "ProbeData.Items",
                "ProbeData.Items.Item",
                "ProbeData.Items.Item.label",
                "ProbeData.Items.Item.count"
            ]
        );
        let class = "ProbeData.Items.Item";
        assert_eq!(
            graph.declaration(class).unwrap().kind(),
            DeclarationKind::Class
        );
        assert_eq!(
            graph
                .members(class)
                .unwrap()
                .iter()
                .map(|d| d.name())
                .collect::<Vec<_>>(),
            ["label", "count"]
        );
        let label = graph.declaration("ProbeData.Items.Item.label").unwrap();
        assert_eq!(label.mandatory(), Some(true));
        assert_eq!(
            graph
                .declaration("ProbeData.Items.Item.count")
                .unwrap()
                .mandatory(),
            Some(false)
        );
        assert_eq!(
            graph.declaration("ProbeBase.Label").unwrap().mandatory(),
            None
        );
        let reference = label.type_reference().unwrap();
        assert_eq!(reference.declared_name(), "ProbeBase.Label");
        assert_eq!(reference.target_name(), "ProbeBase.Label");
        let loc = report.sources().location(reference.source_span()).unwrap();
        let text = report.sources().by_name(&loc.source).unwrap().text();
        assert_eq!(
            &text[loc.start_byte as usize..loc.end_byte as usize],
            "ProbeBase.Label"
        );
        assert!(
            matches!(graph.effective_type("ProbeData.Items.Item.label"),Some(TypeInfo::Text {max_length}) if max_length=="40")
        );
        let Some(TypeInfo::Numeric { lower, upper }) =
            graph.effective_type("ProbeData.Items.Item.count")
        else {
            panic!()
        };
        assert_eq!((lower.original(), upper.original()), ("0", "100"));
        assert_eq!(lower.fractional_digits(), 0);
        assert_eq!(upper.written_exponent(), 0);
        let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
        assert_eq!(json["status"], "checked_subset");
        assert_eq!(json["fullValidation"], false);
        assert_eq!(json["model"]["models"][1]["languageVersion"], version);
        assert_eq!(
            json["model"]["declarations"][6]["effectiveType"]["maxLength"],
            "40"
        );
    }
}
#[test]
fn imports_use_actual_headers_not_source_names_and_do_not_cascade_type_errors() {
    let mut input = pair("2.3");
    input.sources.remove(0);
    let report = outcome(input, Status::Rejected, Some("ILI-IMPORT-NOT-FOUND"));
    assert!(
        !report
            .diagnostics()
            .iter()
            .any(|d| d.code == "ILI-TYPE-NOT-FOUND")
    );
    let report = outcome(
        request(vec![
            ("ProbeBase.ili", BASE.replace("ProbeBase", "Other")),
            ("data.ili", DATA.into()),
        ]),
        Status::Rejected,
        Some("ILI-IMPORT-NOT-FOUND"),
    );
    assert_eq!(report.diagnostics().len(), 1);
    let input = request(vec![
        ("base", BASE.into()),
        (
            "data",
            DATA.replace("ProbeBase.Count", "Wrong.ProbeBase.Count"),
        ),
    ]);
    let report = outcome(input, Status::Rejected, Some("ILI-TYPE-NOT-FOUND"));
    let d = report
        .diagnostics()
        .iter()
        .find(|d| d.code == "ILI-TYPE-NOT-FOUND")
        .unwrap();
    let loc = report
        .sources()
        .location(d.primary.as_ref().unwrap())
        .unwrap();
    let source = report.sources().by_name(&loc.source).unwrap().text();
    assert_eq!(
        &source[loc.start_byte as usize..loc.end_byte as usize],
        "Wrong.ProbeBase.Count"
    );
}
#[test]
fn unknown_domains_unimported_models_and_builtin_boundaries() {
    outcome(
        request(vec![
            ("base", BASE.into()),
            ("data", DATA.replace("ProbeBase.Count", "ProbeBase.Absent")),
        ]),
        Status::Rejected,
        Some("ILI-TYPE-NOT-FOUND"),
    );
    outcome(
        request(vec![
            ("base", BASE.into()),
            ("data", DATA.replace("IMPORTS ProbeBase;", "")),
        ]),
        Status::Rejected,
        Some("ILI-TYPE-NOT-FOUND"),
    );
    let report = outcome(
        single("STRUCTURE S=value:INTERLIS.BOOLEAN; END S;"),
        Status::CheckedSubset,
        None,
    );
    assert!(matches!(
        report.graph().unwrap().effective_type("M.S.value"),
        Some(TypeInfo::Boolean)
    ));
    outcome(
        single("STRUCTURE S=value:INTERLIS.UUIDOID; END S;"),
        Status::Incomplete,
        Some("ILI-UNSUPPORTED"),
    );
    outcome(
        single("STRUCTURE S=END S; STRUCTURE Holder=value:S; END Holder;"),
        Status::Incomplete,
        Some("ILI-UNSUPPORTED"),
    );
    outcome(
        single("DOMAIN B EXTENDS INTERLIS.BOOLEAN=MANDATORY;"),
        Status::Rejected,
        Some("ILI-DERIVATION"),
    );
    outcome(
        request(vec![("x", BASE.replace("ProbeBase", "INTERLIS"))]),
        Status::Rejected,
        Some("ILI-SYNTAX"),
    );
}
#[test]
fn duplicate_models_and_scope_members_keep_original_related_locations() {
    let mut input = pair("2.3");
    input.sources.push(SourceInput {
        name: "extra".into(),
        text: BASE.replace("2026-11-10", "older"),
    });
    let report = outcome(input, Status::Rejected, Some("ILI-DUPLICATE"));
    let d = report
        .diagnostics()
        .iter()
        .find(|d| d.code == "ILI-DUPLICATE")
        .unwrap();
    assert_eq!(d.related.len(), 1);
    assert_ne!(
        report
            .sources()
            .location(d.primary.as_ref().unwrap())
            .unwrap()
            .source,
        report
            .sources()
            .location(&d.related[0].span)
            .unwrap()
            .source
    );
    outcome(
        single("TOPIC T=CLASS C=END C; CLASS C=END C; END T;"),
        Status::Rejected,
        Some("ILI-DUPLICATE"),
    );
    outcome(
        single("STRUCTURE S=value:BOOLEAN; value:BOOLEAN; END S;"),
        Status::Rejected,
        Some("ILI-DUPLICATE"),
    );
    outcome(
        single("DOMAIN T=TEXT*4; TOPIC T=END T;"),
        Status::Rejected,
        Some("ILI-DUPLICATE"),
    );
    // Type and component names are different categories, not one global name ban.
    outcome(
        single("DOMAIN value=TEXT*4; STRUCTURE S=value:value; END S;"),
        Status::CheckedSubset,
        None,
    );
}
#[test]
fn matching_end_names_are_checked_at_all_supported_levels() {
    for body in [
        "TOPIC T=END Wrong;",
        "STRUCTURE S=END Wrong;",
        "TOPIC T=CLASS C=END Wrong; END T;",
    ] {
        let report = outcome(single(body), Status::Rejected, Some("ILI-END-NAME"));
        assert!(!report.diagnostics()[0].related.is_empty());
    }
    let mut input = single("");
    input.sources[0].text = input.sources[0].text.replace("END M.", "END Wrong.");
    outcome(input, Status::Rejected, Some("ILI-END-NAME"));
    outcome(
        single("CLASS C=END C;"),
        Status::Rejected,
        Some("ILI-SCOPE"),
    );
}
#[test]
fn local_topic_scopes_and_empty_queries_do_not_mix_with_attribute_names() {
    let report = outcome(
        single(
            "DOMAIN value=TEXT*4; TOPIC T=DOMAIN Count=0..9; STRUCTURE S=value:value; n:Count; END S; END T; TOPIC Empty=END Empty;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert_eq!(graph.members("M.Empty").unwrap().len(), 0);
    assert!(graph.members("absent").is_none());
    assert!(graph.members("M.value").is_none());
    assert!(
        matches!(graph.effective_type("M.T.S.value"),Some(TypeInfo::Text {max_length}) if max_length=="4")
    );
    assert!(matches!(
        graph.effective_type("M.T.S.n"),
        Some(TypeInfo::Numeric { .. })
    ));
}
#[test]
fn mandatory_derivation_chains_retain_declared_origin_and_effective_flags() {
    let report = outcome(
        single(
            "DOMAIN Base=TEXT*20; First EXTENDS Base=MANDATORY; Last EXTENDS First=MANDATORY; STRUCTURE S=a:Last; b:Base; c:MANDATORY Base; END S;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert_eq!(graph.declaration("M.S.a").unwrap().mandatory(), Some(true));
    assert_eq!(
        graph.declaration("M.S.a").unwrap().declared_mandatory(),
        Some(false)
    );
    assert_eq!(graph.declaration("M.S.b").unwrap().mandatory(), Some(false));
    assert_eq!(graph.declaration("M.S.c").unwrap().mandatory(), Some(true));
    assert_eq!(graph.declaration("M.Last").unwrap().mandatory(), None);
    assert_eq!(
        graph.declaration("M.Last").unwrap().declared_mandatory(),
        Some(true)
    );
    let r = graph
        .declaration("M.Last")
        .unwrap()
        .type_reference()
        .unwrap();
    assert_eq!((r.declared_name(), r.target_name()), ("First", "M.First"));
    assert!(
        matches!(graph.effective_type("M.S.a"),Some(TypeInfo::Text {max_length}) if max_length=="20")
    );
    let report = outcome(
        single("DOMAIN Base=MANDATORY BOOLEAN; STRUCTURE S=a:Base; END S;"),
        Status::CheckedSubset,
        None,
    );
    assert_eq!(
        report
            .graph()
            .unwrap()
            .declaration("M.S.a")
            .unwrap()
            .mandatory(),
        Some(true)
    );
    let input = request(vec![
        ("a", BASE.into()),
        (
            "b",
            DATA.replace(
                "TOPIC Items =",
                "DOMAIN Required EXTENDS ProbeBase.Label=MANDATORY; TOPIC Items =",
            )
            .replace("MANDATORY ProbeBase.Label", "Required"),
        ),
    ]);
    let report = outcome(input, Status::CheckedSubset, None);
    assert_eq!(
        report
            .graph()
            .unwrap()
            .declaration("ProbeData.Items.Item.label")
            .unwrap()
            .mandatory(),
        Some(true)
    );
}
#[test]
fn cycles_and_long_norm_ordered_derivations_use_bounded_iterative_work() {
    let report = outcome(
        single(
            "DOMAIN A EXTENDS B=MANDATORY; B EXTENDS A=MANDATORY; C EXTENDS A=MANDATORY; STRUCTURE S=v:C; END S;",
        ),
        Status::Rejected,
        Some("ILI-DOMAIN-CYCLE"),
    );
    assert_eq!(
        report
            .diagnostics()
            .iter()
            .filter(|d| d.code == "ILI-DOMAIN-CYCLE")
            .count(),
        1
    );
    assert!(
        report
            .diagnostics()
            .iter()
            .any(|d| d.message.contains("A -> B -> A") && d.related.len() == 2)
    );
    let mut body = String::from("DOMAIN D0=BOOLEAN;\n");
    for n in 1..10000 {
        body.push_str(&format!("D{n} EXTENDS D{}=MANDATORY;\n", n - 1));
    }
    body.push_str("STRUCTURE S=v:D9999; END S;");
    let report = outcome(single(&body), Status::CheckedSubset, None);
    let graph = report.graph().unwrap();
    assert!(matches!(
        graph.effective_type("M.S.v"),
        Some(TypeInfo::Boolean)
    ));
    assert_eq!(graph.declaration("M.S.v").unwrap().mandatory(), Some(true));
    assert_eq!(
        graph
            .declaration("M.D9999")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "M.D9998"
    );
    let mut input = single("DOMAIN A=BOOLEAN; B EXTENDS A=MANDATORY;");
    input.limits.max_semantic_steps = 10;
    outcome(input, Status::LimitExceeded, Some("ILI-LIMIT"));
}
#[test]
fn integers_above_js_precision_and_written_decimal_metadata_are_exact() {
    let report = outcome(
        single(
            "DOMAIN Big=9007199254740992..9007199254740993; Fine=0.00..1.00; Coarse=0.0..1.0; Scaled=0.10e-2..0.99e+2; HugeText=TEXT*9007199254740993;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::Numeric { lower, upper }) = graph.effective_type("M.Big") else {
        panic!()
    };
    assert_eq!(lower.coefficient(), "9007199254740992");
    assert_eq!(upper.original(), "9007199254740993");
    let Some(TypeInfo::Numeric { lower: fine, .. }) = graph.effective_type("M.Fine") else {
        panic!()
    };
    let Some(TypeInfo::Numeric { lower: coarse, .. }) = graph.effective_type("M.Coarse") else {
        panic!()
    };
    assert_eq!(
        (fine.fractional_digits(), coarse.fractional_digits()),
        (2, 1)
    );
    assert_eq!((fine.original(), coarse.original()), ("0.00", "0.0"));
    let Some(TypeInfo::Numeric { lower, upper }) = graph.effective_type("M.Scaled") else {
        panic!()
    };
    assert_eq!(
        (lower.original(), lower.written_exponent(), lower.scale()),
        ("0.10e-2", -2, 4)
    );
    assert_eq!(
        (upper.original(), upper.written_exponent(), upper.scale()),
        ("0.99e+2", 2, 0)
    );
    assert!(lower.uses_exponent());
    assert!(
        matches!(graph.effective_type("M.HugeText"),Some(TypeInfo::Text {max_length}) if max_length=="9007199254740993")
    );
    let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let big = json["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["qualifiedName"] == "M.Big")
        .unwrap();
    assert_eq!(
        big["effectiveType"]["upper"]["original"],
        "9007199254740993"
    );
    assert!(big["effectiveType"]["upper"]["coefficient"].is_string());
}
#[test]
fn numeric_order_precision_spelling_and_operational_limits_are_distinct() {
    for (body, code) in [
        (
            "DOMAIN X=9007199254740993..9007199254740992;",
            "ILI-NUMERIC-RANGE",
        ),
        ("DOMAIN X=0.0..1.00;", "ILI-NUMERIC-PRECISION"),
        ("DOMAIN X=1E2..2E2;", "ILI-SYNTAX"),
        ("DOMAIN X=1.0e2..2.0e2;", "ILI-SYNTAX"),
        ("DOMAIN X=TEXT*0;", "ILI-TEXT-LENGTH"),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
    let mut input = single("DOMAIN X=12345..99999;");
    input.limits.max_numeric_digits = 4;
    outcome(input, Status::LimitExceeded, Some("ILI-LIMIT"));
    let mut input = single("DOMAIN X=0.10e-5..0.99e+5;");
    input.limits.max_abs_exponent = 4;
    outcome(input, Status::LimitExceeded, Some("ILI-LIMIT"));
    outcome(
        single("DOMAIN X=0e1000000000..0e1000000000;"),
        Status::LimitExceeded,
        Some("ILI-LIMIT"),
    );
}
#[test]
fn every_supplied_source_is_checked_and_mixed_language_is_incomplete() {
    let mut input = pair("2.3");
    input.sources[1].text = input.sources[1].text.replacen("2.3", "2.4", 1);
    outcome(input, Status::Incomplete, Some("ILI-UNSUPPORTED"));
    let mut input = pair("2.3");
    input.sources.push(SourceInput {name:"unused".into(),text:"INTERLIS 2.3; MODEL Unused AT \"https://example.invalid\" VERSION \"1\" = DOMAIN X=TEXT*0; END Unused.".into()});
    outcome(input, Status::Rejected, Some("ILI-TEXT-LENGTH"));
    outcome(
        request(vec![("empty-model-file", "INTERLIS 2.3;".into())]),
        Status::Rejected,
        Some("ILI-MODEL-MISSING"),
    );
    for body in [
        "TOPIC T=CLASS C=MANDATORY CONSTRAINT x == 1; END C; END T;",
        "VIEW V PROJECTION OF X; = ALL OF X; END V;",
        "STRUCTURE S EXTENDS Base=END S;",
        "DOMAIN N=0..360 CIRCULAR;",
    ] {
        outcome(single(body), Status::Incomplete, Some("ILI-UNSUPPORTED"));
    }
    outcome(pair("2.4"), Status::CheckedSubset, None);
}
#[test]
fn semantic_error_ranges_keep_original_utf8_and_utf16_with_bom_and_crlf() {
    let text = "\u{feff}INTERLIS 2.4;\r\nMODEL M AT \"https://example.invalid/😀\" VERSION \"1\" = STRUCTURE S=value:Missing; END S; END M.";
    let report = outcome(
        request(vec![("memory:///ä😀.ili", text.into())]),
        Status::Rejected,
        Some("ILI-TYPE-NOT-FOUND"),
    );
    let d = report
        .diagnostics()
        .iter()
        .find(|d| d.code == "ILI-TYPE-NOT-FOUND")
        .unwrap();
    let loc = report
        .sources()
        .location(d.primary.as_ref().unwrap())
        .unwrap();
    let offset = text.find("Missing").unwrap();
    assert_eq!(loc.start_byte as usize, offset);
    assert_eq!(loc.end_byte as usize, offset + 7);
    assert_eq!(loc.start_line, 2);
    let prefix = text
        .split("\r\n")
        .nth(1)
        .unwrap()
        .split("Missing")
        .next()
        .unwrap();
    assert_eq!(
        loc.start_column_utf16 as usize,
        prefix.encode_utf16().count() + 1
    );
    assert_eq!(loc.source, "memory:///ä😀.ili");
}

#[test]
fn expanded_json_is_bounded_even_when_the_owned_model_is_small() {
    let mut body = String::from("TOPIC T=CLASS C=");
    for n in 0..30000 {
        body.push_str(&format!("a{n}:BOOLEAN;"));
    }
    body.push_str("END C; END T;");
    let mut input = single(&body);
    input.sources[0].name = "n".repeat(1024);
    input.limits.max_semantic_steps = 3_000_000;
    let report = outcome(input, Status::CheckedSubset, None);
    assert_eq!(report.to_json().unwrap_err().code(), "REPORT_WIRE_SIZE");
    assert!(
        report
            .graph()
            .unwrap()
            .declaration("M.T.C.a29999")
            .is_some()
    );
}

#[test]
fn successful_parallel_runs_and_calls_after_errors_have_no_shared_binding_state() {
    let expected = outcome(pair("2.4"), Status::CheckedSubset, None)
        .to_json()
        .unwrap();
    let handles: Vec<_> = (0..3)
        .map(|_| {
            std::thread::spawn(|| {
                outcome(pair("2.4"), Status::CheckedSubset, None)
                    .to_json()
                    .unwrap()
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), expected);
    }
    outcome(
        single("DOMAIN A EXTENDS B=MANDATORY; B EXTENDS A=MANDATORY;"),
        Status::Rejected,
        Some("ILI-DOMAIN-CYCLE"),
    );
    assert_eq!(
        outcome(pair("2.4"), Status::CheckedSubset, None)
            .to_json()
            .unwrap(),
        expected
    );
}
