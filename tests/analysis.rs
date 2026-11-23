use interlis::{
    AlignmentKind, AnalysisReport, AnalysisStatus as Status, BlackboxKind, CompileRequest,
    DeclarationKind, EnumerationOrder, FixedTextFormat, LanguageVersion, Limits, NumericBounds,
    NumericDirection, NumericRange, SourceInput, TemporalKind, TypeInfo, analyze, analyze_roots,
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

#[test]
fn coordinate_reference_system_requires_the_epsg_lexical_form() {
    let input = single("DOMAIN P=COORD 0..1,0..1 REFSYS \"EPSG:0\";");
    outcome(input, Status::CheckedSubset, None);
    for code in ["", "bogus", "EPSG:", "epsg:2056", "EPSG:-1", "EPSG:1.5"] {
        let value = format!("\"{code}\"");
        let report = outcome(
            single(&format!("DOMAIN P=COORD 0..1,0..1 REFSYS {value};")),
            Status::Rejected,
            Some("ILI-COORD-REFSYS"),
        );
        let diagnostic = report
            .diagnostics()
            .iter()
            .find(|d| d.code == "ILI-COORD-REFSYS")
            .unwrap();
        let sources = report.sources();
        let location = sources
            .location(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        let source = &sources.files()[0].text();
        assert_eq!(
            &source[location.start_byte as usize..location.end_byte as usize],
            value
        );
    }
}

#[test]
fn scalar_types_keep_bounds_families_and_predefined_origins() {
    let body = "DOMAIN Unbounded=TEXT; Long=MTEXT; Bounded=MTEXT*9007199254740993; Identifier=NAME; Link=URI; Horizontal=HALIGNMENT; Vertical=VALIGNMENT; Xml=BLACKBOX XML; Binary=BLACKBOX BINARY; STRUCTURE S=day:INTERLIS.XMLDate; END S;";
    for (version, language) in [("2.3", LanguageVersion::V23), ("2.4", LanguageVersion::V24)] {
        let text = format!("INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        let report = outcome(request(vec![("scalar", text)]), Status::CheckedSubset, None);
        let graph = report.graph().unwrap();
        let ty = |name: &str| graph.effective_type(name).unwrap();
        assert!(matches!(ty("M.Unbounded"), TypeInfo::UnboundedText));
        assert!(matches!(
            ty("M.Long"),
            TypeInfo::MultilineText { max_length: None }
        ));
        assert!(
            matches!(ty("M.Bounded"), TypeInfo::MultilineText { max_length: Some(length) } if length == "9007199254740993")
        );
        assert!(
            matches!(ty("M.Identifier"), TypeInfo::FixedText { max_length, format: FixedTextFormat::Name } if max_length == "255")
        );
        assert!(
            matches!(ty("M.Link"), TypeInfo::FixedText { max_length, format: FixedTextFormat::Uri } if max_length == "1023")
        );
        assert!(matches!(
            ty("M.Horizontal"),
            TypeInfo::Alignment(AlignmentKind::Horizontal)
        ));
        assert!(matches!(
            ty("M.Vertical"),
            TypeInfo::Alignment(AlignmentKind::Vertical)
        ));
        assert!(matches!(ty("M.Xml"), TypeInfo::Blackbox(BlackboxKind::Xml)));
        assert!(matches!(
            ty("M.Binary"),
            TypeInfo::Blackbox(BlackboxKind::Binary)
        ));
        assert!(matches!(
            ty("M.S.day"),
            TypeInfo::Temporal(TemporalKind::Date)
        ));
        for (name, builtin) in [
            ("M.Identifier", "INTERLIS.NAME"),
            ("M.Link", "INTERLIS.URI"),
            ("M.S.day", "INTERLIS.XMLDate"),
        ] {
            let reference = graph.declaration(name).unwrap().type_reference().unwrap();
            assert_eq!(reference.target_name(), builtin);
            assert_eq!(reference.predefined_language(), Some(language));
        }
        let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
        let bounded = wire["model"]["declarations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == "Bounded")
            .unwrap();
        assert_eq!(bounded["effectiveType"]["maxLength"], "9007199254740993");
    }

    let report = outcome(
        single("DOMAIN Day=DATE; Clock=TIMEOFDAY; Stamp=DATETIME;"),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(matches!(
        graph.effective_type("M.Clock"),
        Some(TypeInfo::Temporal(TemporalKind::TimeOfDay))
    ));
    assert!(matches!(
        graph.effective_type("M.Stamp"),
        Some(TypeInfo::Temporal(TemporalKind::DateTime))
    ));
    let day = graph
        .declaration("M.Day")
        .unwrap()
        .type_reference()
        .unwrap();
    assert_eq!(
        (day.declared_name(), day.target_name()),
        ("DATE", "INTERLIS.XMLDate")
    );
    outcome(
        single("DOMAIN Invalid=MTEXT*0;"),
        Status::Rejected,
        Some("ILI-TEXT-LENGTH"),
    );
}

#[test]
fn enumerations_preserve_sibling_order_final_subtrees_and_tree_domains() {
    let body = "DOMAIN Colour=(warm(red,blue:FINAL), cool(blue), neutral) CIRCULAR; AllColours=ALL OF Colour; AllBoolean=ALL OF INTERLIS.BOOLEAN; Empty=(FINAL); STRUCTURE S=value:ALL OF Colour; END S;";
    for version in ["2.3", "2.4"] {
        let text = format!("INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        let report = outcome(request(vec![("enum", text)]), Status::CheckedSubset, None);
        let graph = report.graph().unwrap();
        let Some(TypeInfo::Enumeration {
            order,
            declared_order,
            values,
        }) = graph.effective_type("M.Colour")
        else {
            panic!("expected Colour")
        };
        assert_eq!(
            (*order, *declared_order),
            (
                Some(EnumerationOrder::Circular),
                Some(EnumerationOrder::Circular)
            )
        );
        assert_eq!(
            values
                .elements()
                .iter()
                .map(|e| e.name())
                .collect::<Vec<_>>(),
            ["warm", "cool", "neutral"]
        );
        let warm = values.elements()[0].children().unwrap();
        assert!(warm.is_final());
        assert_eq!(warm.elements()[1].name(), "blue");
        assert!(values.elements()[2].children().is_none());
        let blue = report
            .sources()
            .location(warm.elements()[1].source_span())
            .unwrap();
        assert_eq!(
            &report.sources().files()[0].text()[blue.start_byte as usize..blue.end_byte as usize],
            "blue"
        );
        for root in ["M.AllColours", "M.S.value"] {
            assert!(
                matches!(graph.effective_type(root), Some(TypeInfo::EnumTree { domain }) if domain == "M.Colour")
            );
        }
        assert!(
            matches!(graph.effective_type("M.AllBoolean"), Some(TypeInfo::EnumTree { domain }) if domain == "INTERLIS.BOOLEAN")
        );
        assert!(
            matches!(graph.effective_type("M.Empty"), Some(TypeInfo::Enumeration { values, .. }) if values.is_final() && values.elements().is_empty())
        );
    }
}

#[test]
fn enumeration_definitions_and_tree_domains_reject_illegal_targets() {
    for (body, code) in [
        ("DOMAIN Colour=(red,red);", "ILI-ENUM-DUPLICATE"),
        ("DOMAIN Colour=(a.b,c);", "ILI-ENUM-PATH"),
        ("DOMAIN X=TEXT; Tree=ALL OF X;", "ILI-ENUM-TARGET"),
        (
            "DOMAIN X=(a,b); Tree=ALL OF X; Other=ALL OF Tree;",
            "ILI-ENUM-TARGET",
        ),
        ("DOMAIN A=ALL OF B; B=ALL OF A;", "ILI-DOMAIN-CYCLE"),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
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
        let Some(TypeInfo::Numeric(NumericRange {
            bounds: Some(NumericBounds { lower, upper }),
            ..
        })) = graph.effective_type("ProbeData.Items.Item.count")
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
        Some(TypeInfo::Numeric(_))
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
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { lower, upper }),
        ..
    })) = graph.effective_type("M.Big")
    else {
        panic!()
    };
    assert_eq!(lower.coefficient(), "9007199254740992");
    assert_eq!(upper.original(), "9007199254740993");
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { lower: fine, .. }),
        ..
    })) = graph.effective_type("M.Fine")
    else {
        panic!()
    };
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { lower: coarse, .. }),
        ..
    })) = graph.effective_type("M.Coarse")
    else {
        panic!()
    };
    assert_eq!(
        (fine.fractional_digits(), coarse.fractional_digits()),
        (2, 1)
    );
    assert_eq!((fine.original(), coarse.original()), ("0.00", "0.0"));
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { lower, upper }),
        ..
    })) = graph.effective_type("M.Scaled")
    else {
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
fn every_supplied_source_is_checked_and_mixed_languages_bind_by_model() {
    let mut input = pair("2.3");
    input.sources[1].text = input.sources[1].text.replacen("2.3", "2.4", 1);
    let report = outcome(input, Status::CheckedSubset, None);
    let models = report.graph().unwrap().models();
    assert_eq!(models[0].language(), LanguageVersion::V23);
    assert_eq!(models[1].language(), LanguageVersion::V24);
    let mut input = pair("2.3");
    input.sources.push(SourceInput {name:"unused".into(),text:"INTERLIS 2.3; MODEL Unused AT \"https://example.invalid\" VERSION \"1\" = DOMAIN X=TEXT*0; END Unused.".into()});
    outcome(input, Status::Rejected, Some("ILI-TEXT-LENGTH"));
    let empty = outcome(
        request(vec![("empty-model-file", "INTERLIS 2.3;".into())]),
        Status::CheckedSubset,
        None,
    );
    assert!(empty.graph().unwrap().models().is_empty());
    for body in [
        "TOPIC T=CLASS C=MANDATORY CONSTRAINT x == 1; END C; END T;",
        "TOPIC T = CLASS X=END X; VIEW V PROJECTION OF X; = ALL OF X; END V; END T;",
        "STRUCTURE Base=END Base; STRUCTURE S EXTENDS Base=END S;",
    ] {
        outcome(single(body), Status::Incomplete, Some("ILI-UNSUPPORTED"));
    }
    outcome(pair("2.4"), Status::CheckedSubset, None);
}

#[test]
fn roots_include_transitive_imports_and_ignore_unselected_model_semantics() {
    let sources = || {
        request(vec![
            ("z-main", DATA.replace("2.3", "2.4")),
            ("a-base", BASE.replace("=\n DOMAIN", "= IMPORTS Units;\n DOMAIN")),
            ("b-units", "INTERLIS 2.4; TYPE MODEL Units AT \"x\" VERSION \"1\" = DOMAIN Count=0..10; END Units.".into()),
            ("c-unselected", "INTERLIS 2.4; MODEL Unused AT \"x\" VERSION \"1\" = IMPORTS Missing; END Unused. MODEL UnitsUnused AT \"x\" VERSION \"1\" = END UnitsUnused.".into()),
        ])
    };
    let report = analyze_roots(sources(), &["ProbeData".into()]).unwrap();
    assert_eq!(
        report.status(),
        Status::CheckedSubset,
        "{:?}",
        report.diagnostics()
    );
    let graph = report.graph().unwrap();
    assert_eq!(
        graph.models().iter().map(|m| m.name()).collect::<Vec<_>>(),
        ["ProbeBase", "Units", "ProbeData"]
    );
    assert_eq!(graph.models()[0].imports(), &["Units"]);
    assert_eq!(graph.models()[2].imports(), &["ProbeBase"]);
    assert_eq!(
        graph
            .declaration("ProbeData.Items.Item.count")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "ProbeBase.Count"
    );
    assert!(graph.declaration("Unused").is_none());
    outcome(sources(), Status::Rejected, Some("ILI-IMPORT-NOT-FOUND"));
    let absent = analyze_roots(sources(), &["Absent".into()]).unwrap();
    assert_eq!(absent.status(), Status::Rejected);
    assert_eq!(absent.diagnostics()[0].code, "ILI-ROOT-NOT-FOUND");
    let mut ambiguous = sources();
    ambiguous.sources.push(SourceInput {
        name: "second-base".into(),
        text: BASE.replace("2026-11-10", "2026-11-11"),
    });
    let ambiguous = analyze_roots(ambiguous, &["ProbeData".into()]).unwrap();
    assert_eq!(ambiguous.status(), Status::Rejected);
    let diagnostic = &ambiguous.diagnostics()[0];
    assert_eq!(diagnostic.code, "ILI-IMPORT-AMBIGUOUS");
    assert_eq!(diagnostic.related.len(), 2);
    assert_eq!(
        ambiguous
            .sources()
            .location(diagnostic.primary.as_ref().unwrap())
            .unwrap()
            .source,
        "z-main"
    );
}

#[test]
fn import_cycles_require_a_path_back_to_an_active_model() {
    let sources = |last: &str| {
        request(vec![
            (
                "a",
                "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = IMPORTS B,C; END A.".into(),
            ),
            (
                "b",
                "INTERLIS 2.4; MODEL B AT \"x\" VERSION \"1\" = IMPORTS C; END B.".into(),
            ),
            (
                "c",
                format!("INTERLIS 2.4; MODEL C AT \"x\" VERSION \"1\" = {last} END C."),
            ),
        ])
    };
    assert_eq!(
        analyze_roots(sources(""), &["A".into()]).unwrap().status(),
        Status::CheckedSubset
    );
    for (target, expected_source) in [("C", "c"), ("A", "a")] {
        let cyclic = analyze_roots(sources(&format!("IMPORTS {target};")), &["A".into()]).unwrap();
        assert_eq!(cyclic.status(), Status::Rejected);
        let [d] = cyclic.diagnostics() else {
            panic!("{:?}", cyclic.diagnostics());
        };
        assert_eq!(d.code, "ILI-IMPORT-CYCLE");
        assert_eq!(
            cyclic
                .sources()
                .location(d.primary.as_ref().unwrap())
                .unwrap()
                .source,
            "c"
        );
        assert_eq!(
            cyclic
                .sources()
                .location(&d.related[0].span)
                .unwrap()
                .source,
            expected_source
        );
    }
}

#[test]
fn model_imports_can_refer_to_later_declarations_in_the_same_source() {
    // INTERLIS 2.3 §2.5.1 and 2.4 §3.5.1 specify availability by import.
    // interlis-java resolves the complete source. ili2c rejects this order.
    for version in ["2.3", "2.4"] {
        let text = format!(
            "INTERLIS {version}; MODEL UseFirst AT \"x\" VERSION \"1\" = IMPORTS Later; STRUCTURE S=value:Later.D; END S; END UseFirst. MODEL Later AT \"x\" VERSION \"1\" = DOMAIN D=TEXT*4; END Later."
        );
        let report =
            analyze_roots(request(vec![("model-order", text)]), &["UseFirst".into()]).unwrap();
        assert_eq!(report.status(), Status::CheckedSubset);
        let graph = report.graph().unwrap();
        assert!(
            matches!(graph.effective_type("UseFirst.S.value"), Some(TypeInfo::Text { max_length }) if max_length == "4")
        );
    }
}

#[test]
fn selected_translation_depends_on_exact_original_and_its_imports() {
    let input = |version: &str, imports: &str| {
        request(vec![
            ("a-translation", format!("INTERLIS 2.4; MODEL French AT \"x\" VERSION \"2\" TRANSLATION OF Original [\"{version}\"] = END French.")),
            ("b-original", format!("INTERLIS 2.4; MODEL Original AT \"x\" VERSION \"1\" = {imports} END Original.")),
            ("c-other-version", "INTERLIS 2.4; MODEL Original AT \"x\" VERSION \"3\" = IMPORTS Unused; END Original.".into()),
        ])
    };
    for (version, imports, expected_status, code, source) in [
        (
            "1",
            "",
            Status::Incomplete,
            "ILI-UNSUPPORTED",
            "a-translation",
        ),
        (
            "1",
            "IMPORTS Missing;",
            Status::Rejected,
            "ILI-IMPORT-NOT-FOUND",
            "b-original",
        ),
        (
            "3",
            "",
            Status::Rejected,
            "ILI-IMPORT-NOT-FOUND",
            "c-other-version",
        ),
        (
            "01",
            "",
            Status::Rejected,
            "ILI-TRANSLATION-ORIGINAL-NOT-FOUND",
            "a-translation",
        ),
    ] {
        let report = analyze_roots(input(version, imports), &["French".into()]).unwrap();
        assert_eq!(
            report.status(),
            expected_status,
            "{:?}",
            report.diagnostics()
        );
        assert!(report.graph().is_none());
        let [diagnostic] = report.diagnostics() else {
            panic!("{:?}", report.diagnostics());
        };
        assert_eq!(diagnostic.code, code);
        assert_eq!(
            report
                .sources()
                .location(diagnostic.primary.as_ref().unwrap())
                .unwrap()
                .source,
            source
        );
    }

    let mut ambiguous = input("1", "");
    ambiguous.sources[2].text = ambiguous.sources[1].text.clone();
    let report = analyze_roots(ambiguous, &["French".into()]).unwrap();
    assert_eq!(report.status(), Status::Rejected);
    let diagnostic = &report.diagnostics()[0];
    assert_eq!(diagnostic.code, "ILI-TRANSLATION-ORIGINAL-AMBIGUOUS");
    assert_eq!(diagnostic.related.len(), 2);
}

#[test]
fn translation_cycles_are_rejected_at_the_original_reference() {
    let text = "INTERLIS 2.3; MODEL A AT \"x\" VERSION \"1\" TRANSLATION OF B [\"2\"] = END A. MODEL B AT \"x\" VERSION \"2\" TRANSLATION OF A [\"1\"] = END B.";
    let report = outcome(
        request(vec![("cycle", text.into())]),
        Status::Rejected,
        Some("ILI-TRANSLATION-CYCLE"),
    );
    let [diagnostic] = report.diagnostics() else {
        panic!("{:?}", report.diagnostics());
    };
    assert_eq!(diagnostic.related.len(), 1);
    let location = report
        .sources()
        .location(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!(
        &text[location.start_byte as usize..location.end_byte as usize],
        "A"
    );
}

#[test]
fn unqualified_imports_detect_ambiguity_without_leaking_transitive_visibility() {
    let source = |imports: &str, name: &str| {
        request(vec![
            ("a", BASE.into()),
            ("b", BASE.replace("ProbeBase", "Other")),
            (
                "c",
                format!(
                    "INTERLIS 2.3; MODEL M AT \"x\" VERSION \"1\" = {imports} STRUCTURE S=value:{name}; END S; END M."
                ),
            ),
        ])
    };
    let report = outcome(
        source("IMPORTS UNQUALIFIED ProbeBase, UNQUALIFIED Other;", "Label"),
        Status::Rejected,
        Some("ILI-TYPE-AMBIGUOUS"),
    );
    assert_eq!(report.diagnostics()[0].related.len(), 2);
    outcome(
        source("IMPORTS UNQUALIFIED ProbeBase, Other;", "Label"),
        Status::CheckedSubset,
        None,
    );
    outcome(
        source("IMPORTS ProbeBase;", "Label"),
        Status::Rejected,
        Some("ILI-TYPE-NOT-FOUND"),
    );
    let mut input = source("IMPORTS UNQUALIFIED Other;", "Label");
    input.sources[1].text = "INTERLIS 2.3; MODEL Other AT \"x\" VERSION \"1\" = IMPORTS UNQUALIFIED ProbeBase; END Other.".into();
    outcome(input, Status::Rejected, Some("ILI-TYPE-NOT-FOUND"));
    let mut input = source(
        "IMPORTS UNQUALIFIED ProbeBase, UNQUALIFIED INTERLIS;",
        "XMLDate",
    );
    input.sources[0].text = BASE.replace("Label=TEXT*40", "XMLDate=BOOLEAN");
    let report = outcome(input, Status::CheckedSubset, None);
    let attr = report.graph().unwrap().declaration("M.S.value").unwrap();
    assert_eq!(
        attr.type_reference().unwrap().target_name(),
        "ProbeBase.XMLDate"
    );
}

#[test]
fn runtime_parameters_and_types_have_distinct_namespaces() {
    for body in [
        "PARAMETER Value:BOOLEAN; DOMAIN Value=TEXT*3;",
        "DOMAIN Value=TEXT*3; PARAMETER Value:BOOLEAN;",
    ] {
        let report = outcome(single(body), Status::CheckedSubset, None);
        let graph = report.graph().unwrap();
        assert!(
            matches!(graph.effective_type("M.Value"), Some(TypeInfo::Text { max_length }) if max_length == "3")
        );
        let (parameter, ty) = graph.runtime_parameter("M.Value").unwrap();
        assert_eq!(parameter.kind(), DeclarationKind::RuntimeParameter);
        assert!(matches!(ty, TypeInfo::Boolean));
        let report: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
        let parameter = report["model"]["declarations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["kind"] == "runtime_parameter")
            .unwrap();
        assert_eq!(parameter["effectiveType"]["kind"], "boolean");
    }
    let report = outcome(
        single("PARAMETER Value:BOOLEAN; Value:TEXT*3;"),
        Status::Rejected,
        Some("ILI-DUPLICATE"),
    );
    assert_eq!(report.diagnostics()[0].related.len(), 1);
}

#[test]
fn direct_names_are_checked_even_when_types_are_not_yet_supported() {
    for body in [
        "UNIT Meter [m]; Second [m];",
        "DOMAIN F=BOOLEAN; FUNCTION F():BOOLEAN;",
        "STRUCTURE S= P:BOOLEAN; PARAMETER P:BOOLEAN; END S;",
        "TOPIC T= ASSOCIATION = a -- ANYCLASS; a -- ANYCLASS; END; END T;",
        "TOPIC T= CLASS C=END C; VIEW V PROJECTION OF C; = a := #true; a := #false; END V; END T;",
    ] {
        let report = outcome(single(body), Status::Rejected, Some("ILI-DUPLICATE"));
        let diagnostic = report
            .diagnostics()
            .iter()
            .find(|d| d.code == "ILI-DUPLICATE")
            .unwrap();
        let earlier = report
            .sources()
            .location(&diagnostic.related[0].span)
            .unwrap();
        let later = report
            .sources()
            .location(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert!(earlier.start_byte < later.start_byte, "{body}");
    }
    let report = outcome(
        single("UNIT Meter [m]; DOMAIN Meter=BOOLEAN;"),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert_eq!(
        graph
            .declaration("M.m")
            .unwrap()
            .unit()
            .unwrap()
            .designation,
        "Meter"
    );
    assert!(matches!(
        graph.effective_type("M.Meter"),
        Some(TypeInfo::Boolean)
    ));
}

#[test]
fn direct_references_keep_their_namespace_when_other_semantics_are_pending() {
    let defs = "INTERLIS 2.3; MODEL Definitions AT \"x\" VERSION \"1\" = UNIT Meter[m] EXTENDS INTERLIS.LENGTH; TOPIC T=END T; END Definitions.";
    for (body, status, code) in [
        (
            "IMPORTS UNQUALIFIED Definitions; DOMAIN D=0..2 [m];",
            Status::CheckedSubset,
            None,
        ),
        (
            "IMPORTS Definitions; DOMAIN D=0..2 [Definitions.m];",
            Status::CheckedSubset,
            None,
        ),
        (
            "IMPORTS Definitions; DOMAIN D=0..2 [Definitions.Meter];",
            Status::Rejected,
            Some("ILI-NAME-NOT-FOUND"),
        ),
        (
            "DOMAIN D=0..2 [Definitions.m];",
            Status::Rejected,
            Some("ILI-NAME-NOT-FOUND"),
        ),
        (
            "IMPORTS Definitions; REFSYSTEM BASKET B ~ Definitions.T;",
            Status::Incomplete,
            Some("ILI-UNSUPPORTED"),
        ),
        (
            "REFSYSTEM BASKET B ~ Definitions.T;",
            Status::Rejected,
            Some("ILI-NAME-NOT-FOUND"),
        ),
    ] {
        let text = format!("INTERLIS 2.4; MODEL Use AT \"x\" VERSION \"1\" = {body} END Use.");
        let report = outcome(
            request(vec![("definitions", defs.into()), ("use", text)]),
            status,
            code,
        );
        if let Some(graph) = report.graph() {
            let Some(TypeInfo::Numeric(range)) = graph.effective_type("Use.D") else {
                panic!("expected a bound range");
            };
            assert_eq!(range.unit.as_ref().unwrap().target_name(), "Definitions.m");
        }
    }
    // Both editions permit a class to extend a structure. The inherited type
    // of another member is intentionally left to inheritance analysis.
    outcome(
        single(
            "STRUCTURE Base=END Base; TOPIC Parent=DOMAIN D=TEXT*4; END Parent; TOPIC Child EXTENDS Parent=CLASS C EXTENDS Base=value:D; END C; END Child;",
        ),
        Status::Incomplete,
        Some("ILI-UNSUPPORTED"),
    );
}

#[test]
fn translation_original_is_not_an_implicit_import() {
    for (imports, status, code) in [
        ("", Status::Rejected, "ILI-TYPE-NOT-FOUND"),
        ("IMPORTS Base;", Status::Incomplete, "ILI-UNSUPPORTED"),
    ] {
        let input = request(vec![(
            "one-document",
            format!(
                "INTERLIS 2.4; MODEL French AT \"x\" VERSION \"1\" TRANSLATION OF Base [\"1\"] = {imports} STRUCTURE S=value:Base.D; END S; END French. MODEL Base AT \"x\" VERSION \"1\" = DOMAIN D=TEXT*4; END Base."
            ),
        )]);
        let report = analyze_roots(input, &["French".into()]).unwrap();
        assert_eq!(report.status(), status);
        assert_eq!(report.diagnostics()[0].code, code);
    }
}

#[test]
fn association_names_have_matching_opening_and_closing_presence() {
    for (head, end) in [(" Link", ""), ("", " leftright"), (" Link", " Other")] {
        let body = format!(
            "TOPIC T=CLASS C=END C; ASSOCIATION{head}=left -- C; right -- C; END{end}; END T;"
        );
        let report = outcome(single(&body), Status::Rejected, Some("ILI-END-NAME"));
        let diagnostic = &report.diagnostics()[0];
        let location = report
            .sources()
            .location(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        let source = report.sources().files()[0].text();
        assert_eq!(
            &source[location.start_byte as usize..location.end_byte as usize],
            if end.is_empty() { "END" } else { end.trim() }
        );
    }
}

#[test]
fn class_and_structure_value_restrictions_bind_their_declared_kinds() {
    for language in ["2.3", "2.4"] {
        let text = format!(
            "INTERLIS {language}; MODEL M AT \"x\" VERSION \"1\" = STRUCTURE S=END S; DOMAIN KindOfViewable=CLASS RESTRICTION (S); TOPIC T=CLASS C=END C; VIEW V PROJECTION OF C; = END V; END T; DOMAIN KindOfStructure=STRUCTURE RESTRICTION (M.T.C); FUNCTION f(x:OBJECTS OF M.T.V):BOOLEAN; END M."
        );
        outcome(
            request(vec![("class-values", text)]),
            Status::Incomplete,
            Some("ILI-UNSUPPORTED"),
        );
    }
}

#[test]
fn topic_oid_can_name_a_later_domain_in_the_topic_header() {
    // 2.3 §2.5.2 and 2.4 §3.5.2 specify OID-DomainRef in the topic
    // header. The order rule for attribute type references does not apply.
    let body = "TOPIC T=OID AS M.T.LocalId; DOMAIN LocalId=OID 0..2147483647; END T;";
    outcome(single(body), Status::Incomplete, Some("ILI-UNSUPPORTED"));
    outcome(
        single(&body.replace("M.T.LocalId", "M.T.Missing")),
        Status::Rejected,
        Some("ILI-TYPE-NOT-FOUND"),
    );
}

#[test]
fn same_model_domain_references_require_prior_definition() {
    for body in [
        "STRUCTURE S=value:Later; END S; DOMAIN Later=TEXT*4;",
        "DOMAIN X EXTENDS Later=MANDATORY; Later=BOOLEAN;",
    ] {
        let report = outcome(single(body), Status::Rejected, Some("ILI-DOMAIN-ORDER"));
        let diag = &report.diagnostics()[0];
        let reference = report
            .sources()
            .location(diag.primary.as_ref().unwrap())
            .unwrap();
        let declaration = report.sources().location(&diag.related[0].span).unwrap();
        assert!(reference.start_byte < declaration.start_byte);
    }
    outcome(
        single("DOMAIN Later=TEXT*4; STRUCTURE S=value:Later; END S;"),
        Status::CheckedSubset,
        None,
    );
    let report = outcome(
        single("DOMAIN D=TEXT*4; TOPIC T=STRUCTURE S=value:D; END S; DOMAIN D=TEXT*8; END T;"),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert_eq!(
        graph
            .declaration("M.T.S.value")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "M.D"
    );
    assert!(
        matches!(graph.effective_type("M.T.S.value"), Some(TypeInfo::Text { max_length }) if max_length == "4")
    );
}

#[test]
fn predefined_domains_preserve_language_origin_and_visibility() {
    let src = |version: &str, model: &str| {
        format!(
            "INTERLIS {version}; MODEL {model} AT \"x\" VERSION \"1\" = IMPORTS UNQUALIFIED INTERLIS; STRUCTURE S=ref:INTERLIS.URI; date:INTERLIS_1_DATE; END S; END {model}."
        )
    };
    let report = outcome(
        request(vec![("a", src("2.3", "Old")), ("b", src("2.4", "New"))]),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    for (model, version) in [("Old", LanguageVersion::V23), ("New", LanguageVersion::V24)] {
        for (attr, name, length) in [("ref", "URI", "1023"), ("date", "INTERLIS_1_DATE", "8")] {
            let qualified = format!("{model}.S.{attr}");
            let reference = graph
                .declaration(&qualified)
                .unwrap()
                .type_reference()
                .unwrap();
            assert_eq!(reference.predefined_language(), Some(version));
            assert_eq!(reference.target_name(), format!("INTERLIS.{name}"));
            assert!(
                matches!(graph.effective_type(&qualified), Some(TypeInfo::FixedText { max_length, .. }) if max_length == length)
            );
        }
    }
    for (language, expected, code) in [
        ("2.3", Status::Rejected, Some("ILI-TYPE-NOT-FOUND")),
        ("2.4", Status::CheckedSubset, None),
    ] {
        let text = format!(
            "INTERLIS {language}; MODEL M AT \"x\" VERSION \"1\" = STRUCTURE S=value:INTERLIS.NOOID; END S; END M."
        );
        let report = outcome(request(vec![("a", text)]), expected, code);
        if let Some(graph) = report.graph() {
            assert!(matches!(
                graph.effective_type("M.S.value"),
                Some(TypeInfo::Oid(interlis::OidType::Unstable))
            ));
        }
    }
    let report = outcome(
        single(
            "IMPORTS UNQUALIFIED INTERLIS; TOPIC INTERLIS_1_DATE=END INTERLIS_1_DATE; STRUCTURE S=value:INTERLIS_1_DATE; END S;",
        ),
        Status::CheckedSubset,
        None,
    );
    assert_eq!(
        report
            .graph()
            .unwrap()
            .declaration("M.S.value")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "INTERLIS.INTERLIS_1_DATE"
    );
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

#[test]
fn enumeration_extensions_keep_inherited_order_and_explicit_options() {
    let report = outcome(
        single(
            "DOMAIN Base=(warm(red), cool) ORDERED; Added EXTENDS Base=(warm.red(dark,light), cool(pale:FINAL), neutral); Closed EXTENDS Added=(warm.red(FINAL):FINAL); Fixed (FINAL)=BOOLEAN;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(graph.declaration("M.Fixed").unwrap().is_final());
    let Some(TypeInfo::Enumeration {
        values,
        order,
        declared_order,
    }) = graph.effective_type("M.Closed")
    else {
        panic!("expected checked extension");
    };
    assert_eq!(
        (*order, *declared_order),
        (Some(EnumerationOrder::Ordered), None)
    );
    assert!(values.is_final());
    assert_eq!(
        values
            .elements()
            .iter()
            .map(|e| e.name())
            .collect::<Vec<_>>(),
        ["warm", "cool", "neutral"]
    );
    let red = values.elements()[0].children().unwrap().elements()[0]
        .children()
        .unwrap();
    assert_eq!(
        red.elements().iter().map(|e| e.name()).collect::<Vec<_>>(),
        ["dark", "light"]
    );
    assert!(red.is_final());
    assert!(values.elements()[1].children().unwrap().is_final());
    let Some(TypeInfo::Enumeration { values: base, .. }) = graph.effective_type("M.Base") else {
        panic!("missing base");
    };
    assert!(
        base.elements()[0].children().unwrap().elements()[0]
            .children()
            .is_none()
    );
}

#[test]
fn enum_extensions_reject_final_additions_duplicates_and_unbound_paths() {
    for (body, code) in [
        (
            "DOMAIN Base=(a:FINAL); Bad EXTENDS Base=(b);",
            "ILI-ENUM-FINAL",
        ),
        (
            "DOMAIN Base=(a(x:FINAL)); Bad EXTENDS Base=(a(y));",
            "ILI-ENUM-FINAL",
        ),
        (
            "DOMAIN Base=(a); Bad EXTENDS Base=(a);",
            "ILI-ENUM-DUPLICATE",
        ),
        (
            "DOMAIN Base=(a(x)); Bad EXTENDS Base=(a.y(z));",
            "ILI-ENUM-PATH",
        ),
        (
            "DOMAIN Base=(a); Bad EXTENDS Base=(b(x),b.x(y));",
            "ILI-ENUM-PATH",
        ),
        ("DOMAIN Base=TEXT; Bad EXTENDS Base=(a);", "ILI-ENUM-BASE"),
        (
            "DOMAIN Base=(a) CIRCULAR; Bad EXTENDS Base=(b);",
            "ILI-ENUM-CIRCULAR",
        ),
        (
            "DOMAIN Base=(a); Bad EXTENDS Base=(b) CIRCULAR;",
            "ILI-ENUM-CIRCULAR",
        ),
        (
            "DOMAIN Base (FINAL)=(a); Bad EXTENDS Base=(b);",
            "ILI-DOMAIN-FINAL",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn domain_specializations_preserve_base_provenance_and_narrow_values() {
    let report = outcome(
        single(
            "DOMAIN Base=MANDATORY TEXT; Short EXTENDS Base=TEXT*0010; Smaller EXTENDS Short=TEXT*4; Long=MTEXT; Bounded EXTENDS Long=MTEXT*75; N=-20.00..20.00; Refined EXTENDS N=-19.01..019.99; Id=NAME; SameId EXTENDS Id=NAME; STRUCTURE S=value:Smaller; END S;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(
        matches!(graph.effective_type("M.Smaller"), Some(TypeInfo::Text { max_length }) if max_length == "4")
    );
    assert_eq!(
        graph.declaration("M.S.value").unwrap().mandatory(),
        Some(true)
    );
    let first = graph
        .declaration("M.Smaller")
        .unwrap()
        .base_reference()
        .unwrap();
    assert_eq!(first.target_name(), "M.Short");
    assert!(
        matches!(graph.effective_type("M.Bounded"), Some(TypeInfo::MultilineText { max_length: Some(cap) }) if cap == "75")
    );
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { lower, upper }),
        ..
    })) = graph.effective_type("M.Refined")
    else {
        panic!("refined range");
    };
    assert_eq!((lower.original(), upper.original()), ("-19.01", "019.99"));
    let id = graph.declaration("M.SameId").unwrap();
    assert_eq!(id.type_reference().unwrap().target_name(), "INTERLIS.NAME");
    assert_eq!(id.base_reference().unwrap().target_name(), "M.Id");
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let id = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "SameId")
        .unwrap();
    assert_eq!(id["baseReference"]["targetName"], "M.Id");
    assert_eq!(id["effectiveType"]["format"], "name");
}

#[test]
fn domain_specializations_cannot_widen_or_change_scalar_families() {
    for (base, own, code) in [
        ("TEXT*4", "TEXT*0010", "ILI-DOMAIN-EXTENSION"),
        ("TEXT*10", "TEXT", "ILI-DOMAIN-EXTENSION"),
        ("TEXT*10", "MTEXT*5", "ILI-DOMAIN-EXTENSION"),
        ("NAME", "TEXT*255", "ILI-DOMAIN-EXTENSION"),
        ("-20.00..20.00", "-21.00..19.99", "ILI-DOMAIN-EXTENSION"),
        ("-20.00..20.00", "-19.00..20.01", "ILI-DOMAIN-EXTENSION"),
        ("-20.00..20.00", "-19.000..19.999", "ILI-DOMAIN-PRECISION"),
    ] {
        let report = outcome(
            single(&format!("DOMAIN Base={base}; Child EXTENDS Base={own};")),
            Status::Rejected,
            Some(code),
        );
        let diagnostic = &report.diagnostics()[0];
        assert_eq!(diagnostic.related.len(), 1);
    }
}

#[test]
fn numeric_extension_rounds_23_bounds_and_preserves_24_precision() {
    let input = |version: &str, low: &str, high: &str| {
        request(vec![(
            "ranges",
            format!(
                "INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = DOMAIN Normal=-1.00..7.99; Fine EXTENDS Normal={low}..{high}; END M."
            ),
        )])
    };
    let report = outcome(
        input("2.3", "-1.0049", "7.9949"),
        Status::CheckedSubset,
        None,
    );
    let Some(TypeInfo::Numeric(NumericRange {
        bounds: Some(NumericBounds { upper, .. }),
        ..
    })) = report.graph().unwrap().effective_type("M.Fine")
    else {
        panic!("expected rounded-compatible range");
    };
    assert_eq!(upper.original(), "7.9949");
    outcome(
        input("2.3", "-1.0049", "7.9999"),
        Status::Rejected,
        Some("ILI-DOMAIN-EXTENSION"),
    );
    outcome(
        input("2.3", "-1.0050", "7.9949"),
        Status::Rejected,
        Some("ILI-DOMAIN-EXTENSION"),
    );
    outcome(
        input("2.4", "-1.0000", "7.0000"),
        Status::Rejected,
        Some("ILI-DOMAIN-PRECISION"),
    );
    outcome(input("2.4", "-1.00", "7.00"), Status::CheckedSubset, None);
}

#[test]
fn numeric_ranges_preserve_abstractness_circularity_and_direction() {
    let report = outcome(
        single(
            "DOMAIN Angle (ABSTRACT)=NUMERIC CIRCULAR CLOCKWISE; Degree EXTENDS Angle=0..359; Opposite=0..1 COUNTERCLOCKWISE; PARAMETER Decimal:NUMERIC; STRUCTURE Holder=angle:Degree; END Holder;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(graph.declaration("M.Angle").unwrap().is_abstract());
    assert!(matches!(
        graph.effective_type("M.Angle"),
        Some(TypeInfo::Numeric(NumericRange {
            bounds: None,
            unit: None,
            circular: true,
            direction: Some(NumericDirection::Clockwise)
        }))
    ));
    let Some(TypeInfo::Numeric(range)) = graph.effective_type("M.Holder.angle") else {
        panic!("expected concrete angle");
    };
    assert!(range.circular);
    assert_eq!(range.direction, Some(NumericDirection::Clockwise));
    assert_eq!(range.bounds.as_ref().unwrap().upper.original(), "359");
    assert!(matches!(
        graph.effective_type("M.Opposite"),
        Some(TypeInfo::Numeric(NumericRange {
            direction: Some(NumericDirection::Counterclockwise),
            ..
        }))
    ));
    assert!(matches!(
        graph.runtime_parameter("M.Decimal").unwrap().1,
        TypeInfo::Numeric(NumericRange { bounds: None, .. })
    ));

    for (body, code) in [
        ("DOMAIN A=NUMERIC;", "ILI-DOMAIN-ABSTRACT"),
        (
            "DOMAIN A (ABSTRACT)=TEXT*4; STRUCTURE S=x:A; END S;",
            "ILI-ATTRIBUTE-ABSTRACT",
        ),
        (
            "DOMAIN A (ABSTRACT)=NUMERIC; B EXTENDS A=MANDATORY;",
            "ILI-DOMAIN-ABSTRACT",
        ),
        (
            "DOMAIN A (ABSTRACT)=NUMERIC; STRUCTURE S=x:A; END S;",
            "ILI-ATTRIBUTE-ABSTRACT",
        ),
        ("STRUCTURE S=x:NUMERIC; END S;", "ILI-ATTRIBUTE-ABSTRACT"),
        (
            "DOMAIN A=0..10 CLOCKWISE; B EXTENDS A=1..9 COUNTERCLOCKWISE;",
            "ILI-DOMAIN-DIRECTION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn runtime_parameters_follow_their_model_edition_contract() {
    for (edition, marker, status) in [
        ("2.3", "", Status::Rejected),
        ("2.3", "CONTRACTED ", Status::CheckedSubset),
        ("2.4", "", Status::CheckedSubset),
    ] {
        let text = format!(
            "INTERLIS {edition}; {marker}MODEL M AT \"x\" VERSION \"1\" = PARAMETER R:NUMERIC; END M."
        );
        let report = outcome(
            request(vec![("parameter", text)]),
            status,
            (status == Status::Rejected).then_some("ILI-PARAMETER-CONTRACTED"),
        );
        if let Some(graph) = report.graph() {
            assert_eq!(graph.models()[0].is_contracted(), !marker.is_empty());
            assert!(graph.runtime_parameter("M.R").is_some());
        }
    }
}

#[test]
fn coordinate_types_preserve_axes_rotation_reference_system_and_kind() {
    let report = outcome(
        single(
            "DOMAIN Height=COORD 0..10; Plane EXTENDS Height=COORD 1..9,0..20,ROTATION 2 -> 1 REFSYS \"EPSG:2056\"; Solid EXTENDS Plane=COORD 2..8,2..18,-3..3; Points=MULTICOORD 0..1,0..1; Concrete EXTENDS INTERLIS.LineCoord=COORD 0..1,0..1;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::Coordinate(solid)) = graph.effective_type("M.Solid") else {
        panic!("expected 3D coordinate");
    };
    assert_eq!(solid.axes.len(), 3);
    assert_eq!(solid.axes[0].bounds.as_ref().unwrap().lower.original(), "2");
    assert_eq!(
        solid.axes[2].bounds.as_ref().unwrap().lower.original(),
        "-3"
    );
    assert_eq!(
        solid.rotation,
        Some(interlis::AxisRotation {
            zero_axis: 2,
            quarter_turn_axis: 1
        })
    );
    assert_eq!(solid.reference_system.as_deref(), Some("EPSG:2056"));
    assert!(
        matches!(graph.effective_type("M.Points"), Some(TypeInfo::Coordinate(ty)) if ty.kind == interlis::CoordinateKind::Multi)
    );
    let concrete = graph
        .declaration("M.Concrete")
        .unwrap()
        .base_reference()
        .unwrap();
    assert_eq!(concrete.target_name(), "INTERLIS.LineCoord");
    assert_eq!(concrete.predefined_language(), Some(LanguageVersion::V24));

    let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let solid = json["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "Solid")
        .unwrap();
    assert_eq!(solid["effectiveType"]["kind"], "coordinate");
    assert_eq!(solid["effectiveType"]["axes"][2]["lower"]["original"], "-3");
}

#[test]
fn coordinate_types_reject_missing_axes_changed_metadata_and_bad_rotation() {
    for (body, code) in [
        (
            "DOMAIN P=COORD 0..1,0..1,ROTATION 1 -> 1;",
            "ILI-COORD-ROTATION",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1,ROTATION 3 -> 1;",
            "ILI-COORD-ROTATION",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1; Short EXTENDS P=COORD 0..1;",
            "ILI-COORD-EXTENSION",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1,ROTATION 1 -> 2; Rotated EXTENDS P=COORD 0..1,0..1,ROTATION 2 -> 1;",
            "ILI-COORD-EXTENSION",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1 REFSYS \"EPSG:2056\"; Wrong EXTENDS P=COORD 0..1,0..1 REFSYS \"EPSG:4326\";",
            "ILI-COORD-EXTENSION",
        ),
        ("DOMAIN A=COORD NUMERIC,0..1;", "ILI-DOMAIN-ABSTRACT"),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn units_keep_designation_exact_conversion_and_ordered_composition() {
    use interlis::{UnitConstant, UnitDefinition, UnitOperator, UnitScale};

    let units = r#"
        INTERLIS 2.3; CONTRACTED TYPE MODEL Measures AT "x" VERSION "1" =
          UNIT
            Speed (ABSTRACT) = (INTERLIS.LENGTH / INTERLIS.TIME);
            FeetPerSecond [fps] EXTENDS Speed = (ft / INTERLIS.s);
            Foot [ft] EXTENDS INTERLIS.LENGTH = 0.3048 * PI / LNBASE [INTERLIS.m];
            Fahrenheit [degF] = FUNCTION // (degF+459.67)/1.8 // [INTERLIS.K];
            SameLength = [ft];
        END Measures.
    "#;
    let report = outcome(
        request(vec![("measures.ili", units.into())]),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let unit = |name| graph.declaration(name).unwrap().unit().unwrap();
    let feet = unit("Measures.ft");
    assert_eq!(feet.designation, "Foot");
    assert_eq!(feet.base.as_ref().unwrap().target_name(), "INTERLIS.LENGTH");
    let UnitDefinition::Derived {
        target,
        scale: UnitScale::Factors(factors),
    } = &feet.definition
    else {
        panic!("expected a bound exact conversion");
    };
    assert_eq!(target.target_name(), "INTERLIS.m");
    assert_eq!(target.predefined_language(), Some(LanguageVersion::V23));
    assert!(
        matches!(&factors[0].value, UnitConstant::Decimal(value) if value.original()=="0.3048" && value.coefficient()=="3048" && value.scale()==4)
    );
    assert!(matches!(factors[1].value, UnitConstant::Pi));
    assert!(matches!(factors[2].value, UnitConstant::LnBase));
    assert_eq!(factors[2].operator, UnitOperator::Divide);
    let UnitDefinition::Composed { terms } = &unit("Measures.fps").definition else {
        panic!("expected a bound composition");
    };
    assert_eq!(terms[0].unit.target_name(), "Measures.ft");
    assert_eq!(terms[1].operator, UnitOperator::Divide);
    assert_eq!(terms[1].unit.target_name(), "INTERLIS.s");
    assert!(unit("Measures.Speed").abstract_unit);
    assert!(
        matches!(&unit("Measures.degF").definition, UnitDefinition::Derived{scale:UnitScale::Function(description), ..} if description.contains("459.67"))
    );
    assert!(matches!(
        &unit("Measures.SameLength").definition,
        UnitDefinition::Derived {
            scale: UnitScale::Identity,
            ..
        }
    ));
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let feet = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decl| decl["qualifiedName"] == "Measures.ft")
        .unwrap();
    assert_eq!(
        feet["unit"]["definition"]["scale"]["value"][0]["value"]["value"]["scale"],
        4
    );
}

#[test]
fn units_reject_invalid_dependencies_and_conversions() {
    for (body, code) in [
        ("UNIT B [bb] EXTENDS INTERLIS.m;", "ILI-UNIT-BASE"),
        ("UNIT B = 1 [INTERLIS.LENGTH];", "ILI-UNIT-DERIVED"),
        ("UNIT B (ABSTRACT) = 1 [INTERLIS.m];", "ILI-UNIT-DERIVED"),
        ("UNIT B = 1 / -0.00 [INTERLIS.m];", "ILI-UNIT-ZERO"),
        (
            "UNIT B = (INTERLIS.LENGTH/INTERLIS.s);",
            "ILI-UNIT-COMPONENT",
        ),
        (
            "UNIT B EXTENDS INTERLIS.TIME = 1 [INTERLIS.m];",
            "ILI-UNIT-EXTENSION",
        ),
        (
            "UNIT A (ABSTRACT)=(INTERLIS.LENGTH/INTERLIS.TIME); B EXTENDS A=(INTERLIS.m*INTERLIS.s);",
            "ILI-UNIT-EXTENSION",
        ),
        ("UNIT U = [V]; V = (INTERLIS.m/U);", "ILI-UNIT-CYCLE"),
        ("UNIT U = [Unknown];", "ILI-NAME-NOT-FOUND"),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn expanding_short_unit_references_charges_the_owned_target_names() {
    let model = "M".repeat(240);
    let terms = vec!["U"; 120].join("*");
    let text = format!(
        "INTERLIS 2.4; MODEL {model} AT \"x\" VERSION \"1\" = UNIT U; V=({terms}); END {model}."
    );
    let mut input = request(vec![("long-unit-name", text)]);
    input.limits.max_semantic_steps = 8_000;
    let report = outcome(input, Status::LimitExceeded, Some("ILI-LIMIT"));
    assert!(report.graph().is_none());
}

#[test]
fn numeric_and_coordinate_units_bind_inherit_and_specialize() {
    for (edition, language) in [("2.3", LanguageVersion::V23), ("2.4", LanguageVersion::V24)] {
        let text = format!(
            r#"INTERLIS {edition}; MODEL M AT "x" VERSION "1" =
            UNIT Foot [ft] = 0.3048 [INTERLIS.m];
            DOMAIN
                Length (ABSTRACT) = NUMERIC [INTERLIS.LENGTH];
                Short EXTENDS Length = 0..10 [ft];
                Narrow EXTENDS Short = 1..9;
                Point = COORD 0..10 [INTERLIS.m], 0..10 [ft];
                SmallPoint EXTENDS Point = COORD 2..8, 1..9;
        END M."#
        );
        let report = outcome(
            request(vec![("measures", text)]),
            Status::CheckedSubset,
            None,
        );
        let graph = report.graph().unwrap();
        let Some(TypeInfo::Numeric(narrow)) = graph.effective_type("M.Narrow") else {
            panic!("expected a numeric domain");
        };
        let unit = narrow.unit.as_ref().unwrap();
        assert_eq!(unit.target_name(), "M.ft");
        assert_eq!(unit.declared_name(), "ft");
        let Some(TypeInfo::Coordinate(point)) = graph.effective_type("M.SmallPoint") else {
            panic!("expected a coordinate domain");
        };
        let metre = point.axes[0].unit.as_ref().unwrap();
        assert_eq!(metre.target_name(), "INTERLIS.m");
        assert_eq!(metre.predefined_language(), Some(language));
        assert_eq!(point.axes[1].unit.as_ref().unwrap().target_name(), "M.ft");
    }
}

#[test]
fn measured_ranges_reject_illegal_units_and_dimensions() {
    for (body, code) in [
        (
            "DOMAIN D=0..1 [INTERLIS.LENGTH];",
            "ILI-NUMERIC-UNIT-ABSTRACT",
        ),
        (
            "DOMAIN D (ABSTRACT)=COORD NUMERIC [INTERLIS.LENGTH];",
            "ILI-COORD-UNIT-ABSTRACT",
        ),
        (
            "DOMAIN A (ABSTRACT)=NUMERIC [INTERLIS.LENGTH]; D EXTENDS A=0..1;",
            "ILI-NUMERIC-UNIT-ABSTRACT",
        ),
        (
            "DOMAIN A (ABSTRACT)=NUMERIC [INTERLIS.LENGTH]; D EXTENDS A=0..1 [INTERLIS.s];",
            "ILI-NUMERIC-UNIT-EXTENSION",
        ),
        (
            "DOMAIN A=0..1; D EXTENDS A=0..1 [INTERLIS.m];",
            "ILI-NUMERIC-UNIT-EXTENSION",
        ),
        (
            "UNIT Foot [ft]=0.3048 [INTERLIS.m]; DOMAIN A=0..1 [INTERLIS.m]; D EXTENDS A=0..1 [ft];",
            "ILI-NUMERIC-UNIT-EXTENSION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn oid_types_keep_their_carrier_and_predefined_identity() {
    use interlis::OidType;
    let report = outcome(
        single(
            "DOMAIN Custom EXTENDS INTERLIS.ANYOID=OID TEXT*40; Loose=OID ANY; STRUCTURE S=ascii:INTERLIS.STANDARDOID; uuid:INTERLIS.UUIDOID; number:INTERLIS.I32OID; name:OID NAME; arbitrary:INTERLIS.ANYOID; END S;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let oid = |name| graph.effective_type(name).unwrap();
    assert!(
        matches!(oid("M.Custom"), TypeInfo::Oid(OidType::Typed(carrier)) if matches!(carrier.as_ref(), TypeInfo::Text {max_length} if max_length == "40"))
    );
    assert!(matches!(oid("M.Loose"), TypeInfo::Oid(OidType::Any)));
    assert!(matches!(oid("M.S.ascii"), TypeInfo::Oid(OidType::Standard)));
    assert!(matches!(oid("M.S.uuid"), TypeInfo::Oid(OidType::Uuid)));
    assert!(
        matches!(oid("M.S.name"), TypeInfo::Oid(OidType::Typed(carrier)) if matches!(carrier.as_ref(), TypeInfo::FixedText {format: FixedTextFormat::Name, ..}))
    );
    assert!(matches!(oid("M.S.arbitrary"), TypeInfo::Oid(OidType::Any)));
    let TypeInfo::Oid(OidType::Typed(carrier)) = oid("M.S.number") else {
        panic!("expected an OID with a numeric carrier");
    };
    let TypeInfo::Numeric(range) = carrier.as_ref() else {
        panic!("expected numeric carrier bounds");
    };
    let bounds = range.bounds.as_ref().unwrap();
    assert_eq!(
        (bounds.lower.coefficient(), bounds.upper.coefficient()),
        ("0", "2147483647")
    );
    assert_eq!(
        graph
            .declaration("M.S.number")
            .unwrap()
            .type_reference()
            .unwrap()
            .predefined_language(),
        Some(LanguageVersion::V24)
    );
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let custom = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decl| decl["qualifiedName"] == "M.Custom")
        .unwrap();
    assert_eq!(custom["effectiveType"]["oidKind"], "typed");
    assert_eq!(custom["effectiveType"]["carrier"]["maxLength"], "40");
}

#[test]
fn oid_domain_extensions_cannot_change_a_concrete_or_unstable_oid() {
    for body in [
        "DOMAIN Id EXTENDS INTERLIS.UUIDOID=OID TEXT*36;",
        "DOMAIN Id EXTENDS INTERLIS.NOOID=OID TEXT*36;",
        "DOMAIN Id EXTENDS INTERLIS.ANYOID=OID ANY;",
        "DOMAIN A=OID TEXT*36; B EXTENDS A=MANDATORY;",
        "DOMAIN A=TEXT*36; B EXTENDS A=OID TEXT*36;",
        "DOMAIN A=OID TEXT*36; B EXTENDS A=TEXT*36;",
    ] {
        outcome(single(body), Status::Rejected, Some("ILI-OID-EXTENSION"));
    }
}
