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
                matches!(graph.effective_type(root), Some(TypeInfo::EnumTree { domain, .. }) if domain.as_deref() == Some("M.Colour"))
            );
        }
        assert!(
            matches!(graph.effective_type("M.AllBoolean"), Some(TypeInfo::EnumTree { domain, .. }) if domain.as_deref() == Some("INTERLIS.BOOLEAN"))
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
        "TOPIC T=CLASS C=SET CONSTRAINT INTERLIS.areAreas(ALL, UNDEFINED, >>shape); END C; END T;",
        "TOPIC T = CLASS X=END X; VIEW V PROJECTION OF X; = ALL OF X; END V; END T;",
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
    let report = outcome(
        single(
            "STRUCTURE Base=END Base; TOPIC Parent=DOMAIN D=TEXT*4; END Parent; TOPIC Child EXTENDS Parent=CLASS C EXTENDS Base=value:D; END C; END Child;",
        ),
        Status::CheckedSubset,
        None,
    );
    assert_eq!(
        report
            .graph()
            .unwrap()
            .declaration("M.Child.C.value")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "M.Parent.D"
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
            "INTERLIS {language}; CONTRACTED MODEL M AT \"x\" VERSION \"1\" = STRUCTURE S=END S; DOMAIN KindOfViewable=CLASS RESTRICTION (S); TOPIC T=CLASS C=END C; VIEW V PROJECTION OF C; = END V; END T; DOMAIN KindOfStructure=STRUCTURE RESTRICTION (M.T.C); FUNCTION f(x:OBJECTS OF M.T.V):BOOLEAN; END M."
        );
        outcome(
            request(vec![("class-values", text)]),
            Status::Incomplete,
            Some("ILI-UNSUPPORTED"),
        );
        let text = format!(
            "INTERLIS {language}; MODEL M AT \"x\" VERSION \"1\" = STRUCTURE S=END S;
             DOMAIN AnyObject=STRUCTURE; AnyClass EXTENDS AnyObject=CLASS;
             Metaobjects=CLASS RESTRICTION (INTERLIS.METAOBJECT);
             Scales EXTENDS Metaobjects=CLASS RESTRICTION (INTERLIS.SCALSYSTEM);
             Segments=STRUCTURE RESTRICTION (INTERLIS.LineSegment);
             Lines EXTENDS Segments=STRUCTURE RESTRICTION (INTERLIS.StraightSegment);
             TOPIC T=CLASS C EXTENDS S=END C; CLASS D EXTENDS C=END D;
             DOMAIN ClassValues=CLASS RESTRICTION (C; S);
             Narrow EXTENDS ClassValues=CLASS RESTRICTION (D);
             END T; DOMAIN StructValues=STRUCTURE RESTRICTION (S; M.T.C); END M."
        );
        let report = outcome(
            request(vec![("class-values", text)]),
            Status::CheckedSubset,
            None,
        );
        let graph = report.graph().unwrap();
        let Some(TypeInfo::ClassValue(value)) = graph.effective_type("M.T.ClassValues") else {
            panic!("expected a class value");
        };
        assert_eq!(value.kind, interlis::ClassValueKind::Class);
        assert_eq!(
            value
                .restrictions
                .iter()
                .map(|target| target.target_name())
                .collect::<Vec<_>>(),
            ["M.T.C", "M.S"]
        );
        let Some(TypeInfo::ClassValue(narrow)) = graph.effective_type("M.T.Narrow") else {
            panic!("expected a narrowed class value");
        };
        assert_eq!(narrow.restrictions[0].target_name(), "M.T.D");
        let source = &report.sources().files()[0].text();
        let range = report
            .sources()
            .location(narrow.restrictions[0].source_span())
            .unwrap();
        assert_eq!(
            &source[range.start_byte as usize..range.end_byte as usize],
            "D"
        );
    }
    for extension in [
        "CLASS",
        "CLASS RESTRICTION (Unrelated)",
        "STRUCTURE RESTRICTION (C)",
    ] {
        outcome(
            single(&format!(
                "TOPIC T=CLASS C=END C; CLASS Unrelated=END Unrelated;
             DOMAIN Base=CLASS RESTRICTION (C); Derived EXTENDS Base={extension}; END T;"
            )),
            Status::Rejected,
            Some("ILI-CLASS-VALUE-EXTENSION"),
        );
    }
    outcome(
        single(
            "STRUCTURE S=END S; DOMAIN Base=STRUCTURE RESTRICTION (S); Derived EXTENDS Base=CLASS RESTRICTION (S);",
        ),
        Status::Rejected,
        Some("ILI-CLASS-VALUE-EXTENSION"),
    );
}

#[test]
fn topic_oid_can_name_a_later_domain_in_the_topic_header() {
    // 2.3 §2.5.2 and 2.4 §3.5.2 specify OID-DomainRef in the topic
    // header. The order rule for attribute type references does not apply.
    let body = "TOPIC T=OID AS M.T.LocalId; DOMAIN LocalId=OID 0..2147483647; END T;";
    let report = outcome(single(body), Status::CheckedSubset, None);
    let rule = report
        .graph()
        .unwrap()
        .declaration("M.T")
        .unwrap()
        .oid_rules()
        .unwrap()
        .object
        .as_ref()
        .unwrap();
    assert_eq!(rule.domain.as_ref().unwrap().target_name(), "M.T.LocalId");
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
    outcome(
        single("DOMAIN D=TEXT*4; TOPIC T=STRUCTURE S=value:D; END S; DOMAIN D=TEXT*8; END T;"),
        Status::Rejected,
        Some("ILI-TOPIC-ENCLOSING-NAME"),
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
    let text = "\u{feff}INTERLIS 2.4;\r\nMODEL M AT \"https://example.invalid\" VERSION \"1😀\" = STRUCTURE S=value:Missing; END S; END M.";
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
            reference_system: None,
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

#[test]
fn topic_and_class_oid_rules_preserve_defaults_overrides_and_origin() {
    use interlis::OidKind;
    for edition in ["2.3", "2.4"] {
        let text = format!(
            r#"INTERLIS {edition}; MODEL M AT "x" VERSION "1" =
            TOPIC T =
                BASKET OID AS INTERLIS.STANDARDOID;
                OID AS INTERLIS.I32OID;
                CLASS Default = END Default;
                CLASS Override = OID AS INTERLIS.UUIDOID; END Override;
                CLASS Unstable = NO OID; END Unstable;
            END T;
            TOPIC A (ABSTRACT) = OID AS INTERLIS.ANYOID;
                CLASS Root (ABSTRACT) = END Root;
                CLASS Concrete = OID AS INTERLIS.I32OID; END Concrete;
            END A;
        END M."#
        );
        let report = outcome(
            request(vec![("oids", text.clone())]),
            Status::CheckedSubset,
            None,
        );
        let graph = report.graph().unwrap();
        let rules = |name| graph.declaration(name).unwrap().oid_rules().unwrap();
        let topic = rules("M.T");
        let object = |name| rules(name).object.as_ref().unwrap();
        assert_eq!(
            topic
                .basket
                .as_ref()
                .unwrap()
                .domain
                .as_ref()
                .unwrap()
                .target_name(),
            "INTERLIS.STANDARDOID"
        );
        let oid = object("M.T.Default");
        assert_eq!(oid.kind, OidKind::Concrete);
        assert_eq!(
            oid.domain.as_ref().unwrap().target_name(),
            "INTERLIS.I32OID"
        );
        let span = report.sources().location(oid.source_span()).unwrap();
        assert_eq!(
            &text[span.start_byte as usize..span.end_byte as usize],
            "INTERLIS.I32OID"
        );
        assert_eq!(
            object("M.T.Override")
                .domain
                .as_ref()
                .unwrap()
                .target_name(),
            "INTERLIS.UUIDOID"
        );
        assert_eq!(object("M.T.Unstable").kind, OidKind::Unstable);
        assert!(object("M.T.Unstable").domain.is_none());
        assert_eq!(object("M.A.Root").kind, OidKind::Any);
        assert!(rules("M.T.Default").basket.is_none());
    }
}

#[test]
fn oid_rules_require_an_oid_domain_and_open_rules_require_abstract_owners() {
    for (body, code) in [
        (
            "DOMAIN D=TEXT*16; TOPIC T=OID AS D; END T;",
            "ILI-OID-DOMAIN",
        ),
        (
            "TOPIC T=BASKET OID AS INTERLIS.BOOLEAN; END T;",
            "ILI-OID-DOMAIN",
        ),
        ("TOPIC T=OID AS INTERLIS.ANYOID; END T;", "ILI-OID-ABSTRACT"),
        (
            "TOPIC T=CLASS C=OID AS INTERLIS.ANYOID; END C; END T;",
            "ILI-OID-ABSTRACT",
        ),
        (
            "TOPIC T (ABSTRACT)=OID AS INTERLIS.ANYOID; CLASS C=END C; END T;",
            "ILI-OID-ABSTRACT",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn a_concrete_topic_cannot_leave_an_abstract_class_unspecialized() {
    outcome(
        single("TOPIC T=CLASS C (ABSTRACT)=END C; END T;"),
        Status::Rejected,
        Some("ILI-TOPIC-ABSTRACT"),
    );
    outcome(
        single("TOPIC T (ABSTRACT)=CLASS C (ABSTRACT)=END C; END T;"),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn topics_inherit_names_oids_and_direct_dependencies() {
    let report = outcome(
        single(
            "TOPIC Child EXTENDS Base=DEPENDS ON Extra,Shared; CLASS Item=value:Label; END Item; END Child;
             TOPIC Shared=END Shared; TOPIC Extra=END Extra;
             TOPIC Base=BASKET OID AS INTERLIS.STANDARDOID; OID AS INTERLIS.I32OID; DEPENDS ON Shared;
             DOMAIN Label=TEXT*12; END Base;",
        ),
        // The inherited domain is still subject to the ordering requirement.
        Status::Rejected,
        Some("ILI-DOMAIN-ORDER"),
    );
    assert!(report.graph().is_none());
    let report = outcome(
        single(
            "TOPIC Shared=END Shared; TOPIC Extra=END Extra;
             TOPIC Base=BASKET OID AS INTERLIS.STANDARDOID; OID AS INTERLIS.I32OID; DEPENDS ON Shared;
             DOMAIN Label=TEXT*12; END Base;
             TOPIC Child EXTENDS Base=DEPENDS ON Extra,M.Shared; CLASS Item=value:Label; END Item; END Child;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let child = graph.declaration("M.Child").unwrap();
    assert_eq!(child.base_reference().unwrap().target_name(), "M.Base");
    assert_eq!(
        child
            .topic_dependencies()
            .iter()
            .map(|dep| dep.target_name())
            .collect::<Vec<_>>(),
        ["M.Shared", "M.Extra"]
    );
    let source = report.sources();
    let origin = source
        .location(child.topic_dependencies()[0].source_span())
        .unwrap();
    let input = source.files()[0].text();
    assert_eq!(
        &input[origin.start_byte as usize..origin.end_byte as usize],
        "Shared"
    );
    let direct = child.declared_topic_dependencies();
    assert_eq!(
        direct
            .iter()
            .map(|dep| dep.declared_name())
            .collect::<Vec<_>>(),
        ["Extra", "M.Shared"]
    );
    let written = source.location(direct[1].source_span()).unwrap();
    assert_eq!(
        &input[written.start_byte as usize..written.end_byte as usize],
        "M.Shared"
    );
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let child_json = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decl| decl["qualifiedName"] == "M.Child")
        .unwrap();
    assert_eq!(
        child_json["declaredTopicDependencies"][1]["startByte"],
        written.start_byte
    );
    assert_eq!(
        child
            .oid_rules()
            .unwrap()
            .basket
            .as_ref()
            .unwrap()
            .domain
            .as_ref()
            .unwrap()
            .target_name(),
        "INTERLIS.STANDARDOID"
    );
    assert_eq!(
        graph
            .declaration("M.Child.Item")
            .unwrap()
            .oid_rules()
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .domain
            .as_ref()
            .unwrap()
            .target_name(),
        "INTERLIS.I32OID"
    );
    assert_eq!(
        graph
            .declaration("M.Child.Item.value")
            .unwrap()
            .type_reference()
            .unwrap()
            .target_name(),
        "M.Base.Label"
    );
}

#[test]
fn topic_inheritance_and_dependencies_reject_cycles_and_illegal_redefinitions() {
    for (body, code) in [
        (
            "TOPIC A EXTENDS B=END A; TOPIC B EXTENDS A=END B;",
            "ILI-TOPIC-CYCLE",
        ),
        (
            "TOPIC A=DEPENDS ON B; END A; TOPIC B=DEPENDS ON A; END B;",
            "ILI-TOPIC-DEPENDENCY-CYCLE",
        ),
        (
            "TOPIC A=DEPENDS ON B; END A; TOPIC B EXTENDS A=END B;",
            "ILI-TOPIC-DEPENDENCY-CYCLE",
        ),
        (
            "TOPIC A (FINAL)=END A; TOPIC B EXTENDS A=END B;",
            "ILI-TOPIC-FINAL",
        ),
        (
            "TOPIC A=OID AS INTERLIS.UUIDOID; END A; TOPIC B EXTENDS A=OID AS INTERLIS.I32OID; END B;",
            "ILI-OID-OVERRIDE",
        ),
        (
            "TOPIC A=DOMAIN D=BOOLEAN; END A; TOPIC B EXTENDS A=DOMAIN D=TEXT; END B;",
            "ILI-TOPIC-INHERITED-NAME",
        ),
        (
            "TOPIC A (ABSTRACT)=CLASS C (ABSTRACT)=END C; END A; TOPIC B EXTENDS A=END B;",
            "ILI-TOPIC-ABSTRACT",
        ),
        (
            "DOMAIN D=TEXT; TOPIC A=END A; TOPIC B EXTENDS A=DOMAIN D=BOOLEAN; END B;",
            "ILI-TOPIC-ENCLOSING-NAME",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn classes_and_structures_expose_effective_attributes_in_declaration_order() {
    let report = outcome(
        single(
            "STRUCTURE Base=z:BOOLEAN; a:TEXT*12; END Base;
             STRUCTURE Child EXTENDS Base=child:BOOLEAN; END Child;
             TOPIC T (ABSTRACT)=CLASS Root (ABSTRACT) EXTENDS Child=root:BOOLEAN; END Root; END T;
             TOPIC Sub EXTENDS T=CLASS Root (EXTENDED)=tail:BOOLEAN; END Root;
                CLASS Special EXTENDS Root=extra:BOOLEAN; END Special; END Sub;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert_eq!(
        graph
            .declaration("M.Sub.Root")
            .unwrap()
            .base_reference()
            .unwrap()
            .target_name(),
        "M.T.Root"
    );
    assert_eq!(
        graph
            .effective_attributes("M.Sub.Special")
            .unwrap()
            .map(|d| d.qualified_name())
            .collect::<Vec<_>>(),
        [
            "M.Base.z",
            "M.Base.a",
            "M.Child.child",
            "M.T.Root.root",
            "M.Sub.Root.tail",
            "M.Sub.Special.extra"
        ]
    );
    assert_eq!(graph.members("M.Sub.Special").unwrap()[0].name(), "extra");
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let special = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decl| decl["qualifiedName"] == "M.Sub.Special")
        .unwrap();
    assert_eq!(special["effectiveAttributes"][0], "M.Base.z");
    assert_eq!(special["effectiveAttributes"][5], "M.Sub.Special.extra");
}

#[test]
fn viewable_inheritance_rejects_incompatible_bases_and_attribute_shadowing() {
    for (body, code) in [
        (
            "STRUCTURE A EXTENDS B=END A; STRUCTURE B EXTENDS A=END B;",
            "ILI-CLASS-CYCLE",
        ),
        (
            "STRUCTURE A (FINAL)=END A; STRUCTURE B EXTENDS A=END B;",
            "ILI-CLASS-FINAL",
        ),
        (
            "TOPIC T=CLASS A=END A; STRUCTURE B EXTENDS A=END B; END T;",
            "ILI-NAME-NOT-FOUND",
        ),
        ("STRUCTURE A (EXTENDED)=END A;", "ILI-CLASS-EXTENDED"),
        (
            "STRUCTURE A=z:BOOLEAN; END A; STRUCTURE B EXTENDS A=z:BOOLEAN; END B;",
            "ILI-ATTRIBUTE-OVERRIDE",
        ),
        (
            "TOPIC T=CLASS A=END A; CLASS B EXTENDS A=END B; END T; TOPIC S EXTENDS T=CLASS A (EXTENDED)=END A; END S;",
            "ILI-CLASS-EXTENDED-CONFLICT",
        ),
        (
            "TOPIC A=CLASS C=END C; END A; TOPIC B EXTENDS A=CLASS Spec EXTENDS C=END Spec; END B; TOPIC Between EXTENDS B=END Between; TOPIC D EXTENDS Between=CLASS C (EXTENDED)=END C; END D;",
            "ILI-CLASS-EXTENDED-CONFLICT",
        ),
        (
            "TOPIC T=CLASS A=END A; END T; TOPIC S EXTENDS T=CLASS A EXTENDS M.T.A=END A; END S;",
            "ILI-TOPIC-INHERITED-NAME",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn class_oid_specialization_uses_the_declaring_language_version() {
    let body = "TOPIC T=CLASS A=END A; CLASS B EXTENDS A=OID AS INTERLIS.UUIDOID; END B; END T;";
    for (edition, status, code) in [
        ("2.3", Status::CheckedSubset, None),
        ("2.4", Status::Rejected, Some("ILI-OID-OVERRIDE")),
    ] {
        let text = format!("INTERLIS {edition}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        outcome(request(vec![("oid-overrides", text)]), status, code);
    }
    let report = outcome(
        single(
            "TOPIC T=CLASS A (ABSTRACT)=NO OID; END A; CLASS B (ABSTRACT) EXTENDS A=OID AS INTERLIS.ANYOID; END B; CLASS C EXTENDS B=OID AS INTERLIS.UUIDOID; END C; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    assert_eq!(
        report
            .graph()
            .unwrap()
            .declaration("M.T.C")
            .unwrap()
            .oid_rules()
            .unwrap()
            .object
            .as_ref()
            .unwrap()
            .kind,
        interlis::OidKind::Concrete
    );
}

#[test]
fn attribute_specialization_replaces_its_inherited_slot_and_keeps_mandatory() {
    let report = outcome(
        single(
            "DOMAIN Options=(a,b); More EXTENDS Options=(c);
             STRUCTURE Parent (ABSTRACT)=
                z (ABSTRACT):TEXT*30; count:MANDATORY 0..100 [INTERLIS.m]; kind:Options; active:BOOLEAN;
             END Parent;
             STRUCTURE Child EXTENDS Parent=
                active (EXTENDED):BOOLEAN; z (EXTENDED):TEXT*12; count (EXTENDED):MANDATORY 5..8; kind (EXTENDED):More; extra:BOOLEAN;
             END Child;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let attributes = graph
        .effective_attributes("M.Child")
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(
        attributes
            .iter()
            .map(|declaration| declaration.name())
            .collect::<Vec<_>>(),
        ["z", "count", "kind", "active", "extra"]
    );
    assert_eq!(
        attributes[1].base_reference().unwrap().target_name(),
        "M.Parent.count"
    );
    assert_eq!(attributes[1].mandatory(), Some(true));
    assert_eq!(attributes[1].declared_mandatory(), Some(true));
    let Some(TypeInfo::Numeric(range)) = graph.effective_type("M.Child.count") else {
        panic!("expected specialized numeric range");
    };
    assert_eq!(range.bounds.as_ref().unwrap().lower.original(), "5");
    assert_eq!(range.unit.as_ref().unwrap().target_name(), "INTERLIS.m");
    assert!(
        matches!(graph.effective_type("M.Child.kind"), Some(TypeInfo::Enumeration {values, ..}) if values.elements().last().unwrap().name() == "c")
    );
}

#[test]
fn attributes_reject_illegal_overrides_and_incomplete_concretization() {
    for (body, code) in [
        (
            "STRUCTURE P=value:MANDATORY TEXT*4; END P; STRUCTURE S EXTENDS P=value (EXTENDED):TEXT*4; END S;",
            "ILI-ATTRIBUTE-CARDINALITY",
        ),
        ("STRUCTURE S=value:MANDATORY; END S;", "ILI-ATTRIBUTE-TYPE"),
        (
            "STRUCTURE S=value (EXTENDED):BOOLEAN; END S;",
            "ILI-ATTRIBUTE-OVERRIDE",
        ),
        (
            "STRUCTURE P=value (FINAL):TEXT; END P; STRUCTURE S EXTENDS P=value (EXTENDED):TEXT*4; END S;",
            "ILI-ATTRIBUTE-OVERRIDE",
        ),
        (
            "STRUCTURE P=value:TEXT*4; END P; STRUCTURE S EXTENDS P=value (EXTENDED):TEXT*10; END S;",
            "ILI-DOMAIN-EXTENSION",
        ),
        (
            "STRUCTURE P (ABSTRACT)=value (ABSTRACT):TEXT*4; END P; STRUCTURE S EXTENDS P=END S;",
            "ILI-CLASS-ABSTRACT-ATTRIBUTE",
        ),
        (
            "DOMAIN Unitless=0..5; STRUCTURE P=value:0..10 [INTERLIS.m]; END P; STRUCTURE S EXTENDS P=value (EXTENDED):Unitless; END S;",
            "ILI-NUMERIC-UNIT-EXTENSION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn inherited_predefined_oid_keeps_its_edition_in_a_mixed_closure() {
    let base = "INTERLIS 2.3; MODEL Base AT \"x\" VERSION \"1\" = TOPIC T=CLASS Root=OID AS INTERLIS.I32OID; END Root; END T; END Base.";
    for (local, status, code) in [
        ("", Status::CheckedSubset, None),
        (
            "OID AS INTERLIS.I32OID;",
            Status::Rejected,
            Some("ILI-OID-OVERRIDE"),
        ),
    ] {
        let child = format!(
            "INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" = IMPORTS Base; TOPIC T=CLASS Child EXTENDS Base.T.Root={local} END Child; END T; END M."
        );
        let report = outcome(
            request(vec![("base", base.into()), ("child", child)]),
            status,
            code,
        );
        if let Some(graph) = report.graph() {
            let domain = graph
                .declaration("M.T.Child")
                .unwrap()
                .oid_rules()
                .unwrap()
                .object
                .as_ref()
                .unwrap()
                .domain
                .as_ref()
                .unwrap();
            assert_eq!(domain.target_name(), "INTERLIS.I32OID");
            assert_eq!(domain.predefined_language(), Some(LanguageVersion::V23));
        }
    }
}

#[test]
fn collection_values_keep_exact_cardinality_and_restricted_structure_order() {
    use interlis::CollectionKind;
    let report = outcome(
        single(
            "STRUCTURE Part (ABSTRACT)=END Part;
             STRUCTURE Small EXTENDS Part=END Small; STRUCTURE Large EXTENDS Part=END Large;
             STRUCTURE Base=one:MANDATORY Part; many:BAG {1..*} OF Part; END Base;
             STRUCTURE Child EXTENDS Base=many (EXTENDED):LIST {2..9007199254740993} OF Part RESTRICTION (Large; Small); END Child;
             STRUCTURE Narrower EXTENDS Child=many (EXTENDED):LIST OF Small; END Narrower;
             STRUCTURE Scalars=names:LIST OF TEXT*12; END Scalars;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::Collection(collection)) = graph.effective_type("M.Child.many") else {
        panic!("expected collection");
    };
    assert_eq!(collection.kind, CollectionKind::List);
    assert_eq!(collection.cardinality.lower, "2");
    assert_eq!(
        collection.cardinality.upper.as_deref(),
        Some("9007199254740993")
    );
    let Some(TypeInfo::Collection(omitted)) = graph.effective_type("M.Narrower.many") else {
        panic!("expected collection");
    };
    assert_eq!(omitted.cardinality, collection.cardinality);
    assert_eq!(
        graph.declaration("M.Child.many").unwrap().mandatory(),
        Some(true)
    );
    let Some(TypeInfo::Structure(parts)) = graph.collection_element_type("M.Child.many") else {
        panic!("expected restricted structure");
    };
    assert_eq!(parts.target.as_ref().unwrap().target_name(), "M.Part");
    assert_eq!(
        parts
            .restrictions
            .iter()
            .map(|link| link.target_name())
            .collect::<Vec<_>>(),
        ["M.Large", "M.Small"]
    );
    assert!(
        matches!(graph.collection_element_type("M.Scalars.names"), Some(TypeInfo::Text {max_length}) if max_length == "12")
    );
    let wire: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
    let many = wire["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|decl| decl["qualifiedName"] == "M.Child.many")
        .unwrap();
    assert_eq!(
        many["effectiveType"]["cardinality"]["upper"],
        "9007199254740993"
    );
    assert_eq!(
        many["effectiveType"]["element"]["restrictions"][0]["targetName"],
        "M.Large"
    );
}

#[test]
fn collection_extensions_check_order_cardinality_and_structure_targets() {
    for (body, code) in [
        (
            "STRUCTURE S=values:BAG {5..2} OF TEXT; END S;",
            "ILI-CARDINALITY-RANGE",
        ),
        (
            "STRUCTURE P=values:LIST {1..5} OF TEXT; END P; STRUCTURE C EXTENDS P=values (EXTENDED):BAG {1..5} OF TEXT; END C;",
            "ILI-ATTRIBUTE-CARDINALITY",
        ),
        (
            "STRUCTURE P=values:BAG {2..5} OF TEXT; END P; STRUCTURE C EXTENDS P=values (EXTENDED):BAG {1..9} OF TEXT; END C;",
            "ILI-ATTRIBUTE-CARDINALITY",
        ),
        (
            "STRUCTURE A=END A; STRUCTURE B=END B; STRUCTURE C=value:A RESTRICTION (B); END C;",
            "ILI-STRUCTURE-RESTRICTION",
        ),
        (
            "STRUCTURE C=value:MANDATORY ANYSTRUCTURE; END C;",
            "ILI-ATTRIBUTE-ABSTRACT",
        ),
        (
            "DOMAIN Broad=0..10; Small=1..2; STRUCTURE P=value:Broad; END P; STRUCTURE C EXTENDS P=value (EXTENDED):Small; END C;",
            "ILI-DOMAIN-EXTENSION",
        ),
        (
            "DOMAIN Shared=TEXT; One EXTENDS Shared=MANDATORY; Two EXTENDS Shared=MANDATORY; STRUCTURE P=value:One; END P; STRUCTURE C EXTENDS P=value (EXTENDED):Two; END C;",
            "ILI-DOMAIN-EXTENSION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn reference_attributes_bind_concrete_restrictions_and_topic_dependencies() {
    let report = outcome(
        single(
            "TOPIC Targets=CLASS Base (ABSTRACT)=END Base; CLASS Chosen EXTENDS Base=NO OID; END Chosen; END Targets;
             TOPIC Uses=DEPENDS ON Targets;
                 STRUCTURE Link=object:REFERENCE TO (EXTERNAL) M.Targets.Base RESTRICTION (M.Targets.Chosen); END Link;
                 CLASS Item=wrapped:Link; direct:REFERENCE TO (EXTERNAL) M.Targets.Chosen; END Item;
             END Uses;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::Reference(reference)) = graph.effective_type("M.Uses.Link.object") else {
        panic!("expected a bound reference");
    };
    assert!(reference.external);
    assert_eq!(
        reference.target.target.as_ref().unwrap().target_name(),
        "M.Targets.Base"
    );
    assert_eq!(
        reference.target.restrictions[0].target_name(),
        "M.Targets.Chosen"
    );
}

#[test]
fn reference_attributes_check_direct_and_contained_scope() {
    for (body, code) in [
        (
            "TOPIC Target=CLASS Obj=END Obj; END Target; TOPIC Uses=DEPENDS ON Target; STRUCTURE S=link:REFERENCE TO M.Target.Obj; END S; END Uses;",
            "ILI-REFERENCE-EXTERNAL",
        ),
        (
            "TOPIC Target=CLASS Obj=END Obj; END Target; TOPIC Uses=STRUCTURE S=link:REFERENCE TO (EXTERNAL) M.Target.Obj; END S; END Uses;",
            "ILI-REFERENCE-DEPENDS-ON",
        ),
        (
            "TOPIC Target=CLASS Obj=END Obj; END Target; STRUCTURE Wrapper=link:REFERENCE TO M.Target.Obj; END Wrapper; TOPIC Uses=DEPENDS ON Target; CLASS C=wrapper:Wrapper; END C; END Uses;",
            "ILI-REFERENCE-EXTERNAL",
        ),
        (
            "TOPIC T=CLASS A=END A; CLASS B=END B; STRUCTURE S=link:REFERENCE TO A RESTRICTION (B); END S; END T;",
            "ILI-REFERENCE-RESTRICTION",
        ),
        (
            "TOPIC T (ABSTRACT)=CLASS A (ABSTRACT)=END A; STRUCTURE S=link:REFERENCE TO A; END S; END T;",
            "ILI-REFERENCE-ABSTRACT",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
    for body in [
        "TOPIC T=CLASS A=END A; CLASS Owner=link:REFERENCE TO A; END Owner; END T;",
        "TOPIC T=CLASS A=END A; STRUCTURE S=link:REFERENCE TO A; END S; STRUCTURE Wrapper=member:S; END Wrapper; CLASS Owner EXTENDS Wrapper=END Owner; END T;",
    ] {
        let text = format!("INTERLIS 2.3; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        outcome(
            request(vec![("reference-edition", text)]),
            Status::Rejected,
            Some("ILI-REFERENCE-ATTRIBUTE-SCOPE"),
        );
    }
}

#[test]
fn association_roles_preserve_effective_order_targets_strength_and_cardinality() {
    use interlis::RoleStrength;
    let report = outcome(
        single(
            "TOPIC T=
                CLASS Part=END Part; CLASS Specific EXTENDS Part=END Specific; CLASS Whole=END Whole;
                ASSOCIATION Link (OID)=
                    parts (ORDERED) -- {1..*} Part;
                    whole -<#> Whole;
                    label:TEXT*40;
                    CARDINALITY={0..2};
                END Link;
                ASSOCIATION Narrow EXTENDS Link=
                    parts (EXTENDED,ORDERED) -- Specific;
                    label (EXTENDED):TEXT*10;
                END Narrow;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let association = graph
        .declaration("M.T.Narrow")
        .unwrap()
        .association()
        .unwrap();
    assert!(association.identified);
    assert_eq!(association.cardinality.upper.as_deref(), Some("2"));
    let roles = graph
        .effective_roles("M.T.Narrow")
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(
        roles
            .iter()
            .map(|role| role.qualified_name())
            .collect::<Vec<_>>(),
        ["M.T.Narrow.parts", "M.T.Link.whole"]
    );
    assert_eq!(
        roles[0].base_reference().unwrap().target_name(),
        "M.T.Link.parts"
    );
    let parts = roles[0].role().unwrap();
    assert!(parts.ordered);
    assert_eq!(parts.cardinality.lower, "1");
    assert_eq!(
        parts.targets[0].target.as_ref().unwrap().target_name(),
        "M.T.Specific"
    );
    let whole = roles[1].role().unwrap();
    assert_eq!(whole.strength, RoleStrength::Composition);
    assert_eq!(whole.cardinality.upper.as_deref(), Some("1"));
    assert_eq!(
        graph
            .effective_attributes("M.T.Narrow")
            .unwrap()
            .next()
            .unwrap()
            .qualified_name(),
        "M.T.Narrow.label"
    );
}

#[test]
fn associations_reject_invalid_roles_and_access_collisions() {
    let prefix = "TOPIC T=CLASS A=END A; CLASS B=END B; CLASS C EXTENDS A=END C;";
    for (declarations, code) in [
        ("ASSOCIATION Link=END Link;", "ILI-ASSOCIATION-ROLES"),
        (
            "ASSOCIATION Link=x -<#> {2} A; y -- B; END Link;",
            "ILI-ROLE-COMPOSITION-CARDINALITY",
        ),
        (
            "ASSOCIATION Link=x -<> A; y -<> B; END Link;",
            "ILI-ASSOCIATION-AGGREGATION",
        ),
        (
            "ASSOCIATION Link=x -- A OR C; y -- B; END Link;",
            "ILI-ROLE-ALTERNATIVES",
        ),
        (
            "ASSOCIATION Link=x -- A; y -- B; CARDINALITY={0..2}; END Link;",
            "ILI-ASSOCIATION-OID",
        ),
        (
            "ASSOCIATION Link=x -- A; y -- B; END Link; ASSOCIATION Other=x -- Link; y -- B; END Other;",
            "ILI-ROLE-ASSOCIATION-OID",
        ),
        (
            "ASSOCIATION Link=x -- A; y -- B; END Link; ASSOCIATION Other=x -- C; y -- B; END Other;",
            "ILI-ROLE-NAMESPACE",
        ),
        (
            "ASSOCIATION Link=x -- A; y -- B; END Link; ASSOCIATION Other EXTENDS Link=z (EXTENDED) -- B; END Other;",
            "ILI-ASSOCIATION-ROLE",
        ),
        (
            "ASSOCIATION Link=x -- {1..2} A; y -- B; END Link; ASSOCIATION Other EXTENDS Link=x (EXTENDED) -- {0..3} C; END Other;",
            "ILI-ROLE-EXTENSION",
        ),
        (
            "ASSOCIATION Link=x -- A; y -- B; END Link; ASSOCIATION Other EXTENDS Link=x (EXTENDED,HIDING) -- C; END Other; ASSOCIATION Third EXTENDS Link=x (EXTENDED) -- C; END Third;",
            "ILI-ROLE-HIDING",
        ),
    ] {
        outcome(
            single(&format!("{prefix}{declarations} END T;")),
            Status::Rejected,
            Some(code),
        );
    }
}

#[test]
fn inherited_roles_are_checked_in_the_receiving_association() {
    for (body, code) in [
        (
            "TOPIC T=CLASS C=END C; ASSOCIATION Base (OID)=parent -- Derived; child -- C; END Base;
               ASSOCIATION Derived EXTENDS Base=END Derived; END T;",
            "ILI-ROLE-ASSOCIATION-TARGET",
        ),
        (
            "TOPIC T=CLASS A=END A; CLASS B=END B; ASSOCIATION Base=x -- A; y -- B; END Base; END T;
             TOPIC U=ASSOCIATION Derived EXTENDS M.T.Base=END Derived; END U;",
            "ILI-REFERENCE-EXTERNAL",
        ),
        (
            "TOPIC T=CLASS A=END A; CLASS B=END B; CLASS C EXTENDS A=END C;
                ASSOCIATION Base=x -- A; y -- B; END Base;
                ASSOCIATION Left EXTENDS Base=END Left;
                ASSOCIATION Right EXTENDS Base=END Right;
                ASSOCIATION Hide EXTENDS Left=x (EXTENDED,HIDING) -- C; END Hide;
                ASSOCIATION Show EXTENDS Right=x (EXTENDED) -- C; END Show;
             END T;",
            "ILI-ROLE-HIDING",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn geometry_schema_retains_inherited_forms_vertex_and_symbolic_tolerance() {
    use interlis::{GeometryKind, LineForm, OverlapTolerance};
    let report = outcome(
        single(
            "DOMAIN P=COORD 0.000..100.000,0.000..100.000;
             Inside EXTENDS P=COORD 1.000..99.000,1.000..99.000;
             Surface= SURFACE WITH (STRAIGHTS,ARCS) VERTEX P;
             Patch EXTENDS Surface=AREA WITH (STRAIGHTS) VERTEX Inside;
             Line=POLYLINE WITH (STRAIGHTS,ARCS) VERTEX P WITHOUT OVERLAPS > 0.001;
             DirectedLine EXTENDS Line=DIRECTED POLYLINE WITH (ARCS);",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::Geometry(area)) = graph.effective_type("M.Patch") else {
        panic!("expected area descriptor");
    };
    assert_eq!(area.geometry_kind, GeometryKind::Area);
    assert_eq!(area.vertex.as_ref().unwrap().target_name(), "M.Inside");
    assert!(matches!(&area.line_forms[..], [LineForm::Straights]));
    assert!(
        matches!(&area.tolerance, Some(OverlapTolerance::TrackWidth(Some(origin))) if origin.target_name() == "M.P")
    );
    let Some(TypeInfo::Geometry(line)) = graph.effective_type("M.DirectedLine") else {
        panic!("expected line descriptor");
    };
    assert!(line.directed);
    assert!(line.without_overlaps);
    assert!(
        matches!(&line.tolerance, Some(OverlapTolerance::Explicit(value)) if value.original() == "0.001")
    );
}

#[test]
fn geometry_vertex_and_tolerance_checks_follow_the_model_edition() {
    let body = "DOMAIN P=COORD 0.000..100.000,0.000..100.000; L=SURFACE WITH (STRAIGHTS) VERTEX P WITHOUT OVERLAPS > 0.0001;";
    for (edition, status, code) in [
        ("2.3", Status::CheckedSubset, None),
        (
            "2.4",
            Status::Rejected,
            Some("ILI-GEOMETRY-TOLERANCE-PRECISION"),
        ),
    ] {
        let text = format!("INTERLIS {edition}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        outcome(request(vec![("geometry-precision", text)]), status, code);
    }
    for (body, code) in [
        (
            "DOMAIN P=BOOLEAN; G=POLYLINE WITH (STRAIGHTS) VERTEX P;",
            "ILI-GEOMETRY-VERTEX",
        ),
        (
            "DOMAIN P=MULTICOORD 0..1,0..1; G=POLYLINE WITH (STRAIGHTS) VERTEX P;",
            "ILI-GEOMETRY-VERTEX",
        ),
        (
            "DOMAIN D=POLYLINE WITH (STRAIGHTS) VERTEX INTERLIS.LineCoord;",
            "ILI-GEOMETRY-ABSTRACT",
        ),
        (
            "DOMAIN P=COORD 0.0..1.0,0.00..1.00; G=POLYLINE WITH (STRAIGHTS) VERTEX P;",
            "ILI-GEOMETRY-VERTEX-PRECISION",
        ),
        (
            "DOMAIN P=COORD 0.0..1.0 [INTERLIS.s],0.0..1.0 [INTERLIS.s]; G=POLYLINE WITH (STRAIGHTS) VERTEX P;",
            "ILI-GEOMETRY-VERTEX-UNIT",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1; G=POLYLINE WITH (STRAIGHTS) VERTEX P WITHOUT OVERLAPS > 0;",
            "ILI-GEOMETRY-TOLERANCE",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1; G=POLYLINE WITH (STRAIGHTS) VERTEX P; Other EXTENDS G=POLYLINE WITH (ARCS);",
            "ILI-GEOMETRY-EXTENSION",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1; Q=COORD 0..2,0..2; G=POLYLINE WITH (STRAIGHTS) VERTEX P; Other EXTENDS G=POLYLINE VERTEX Q;",
            "ILI-GEOMETRY-VERTEX-EXTENSION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn numeric_reference_systems_bind_axes_and_exact_convertible_units() {
    for edition in ["2.3", "2.4"] {
        let body = "UNIT Foot [ft]=0.3048 [INTERLIS.m]; Centimeter [cm]=1/100 [INTERLIS.m];
            MeterPerSecond [ms]=(INTERLIS.m/INTERLIS.s); FeetPerHour [fph]=(ft/INTERLIS.h);
            DOMAIN Pos=COORD 0..100 [INTERLIS.m],0..100 [INTERLIS.m],ROTATION 1 -> 2;
                Scale=0..100 [cm]; Height=0..10 [ft] <Scale>;
                East=0..10 [cm] <Pos[1]>; Restricted EXTENDS East=1..9;
                Velocity=0..100 [ms]; Rate=0..10 [fph] <Velocity>;
                Bearing=0..360 [INTERLIS.rad] <Pos>;";
        let text = format!("INTERLIS {edition}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.");
        let report = outcome(
            request(vec![("reference-systems", text)]),
            Status::CheckedSubset,
            None,
        );
        let graph = report.graph().unwrap();
        let system = |name| {
            let Some(TypeInfo::Numeric(range)) = graph.effective_type(name) else {
                panic!("expected numeric type");
            };
            range.reference_system.as_ref().unwrap()
        };
        assert_eq!(system("M.Height").domain.target_name(), "M.Scale");
        assert_eq!(system("M.Restricted").domain.target_name(), "M.Pos");
        assert_eq!(system("M.Restricted").axis, Some(1));
        assert_eq!(system("M.Bearing").axis, None);
        assert_eq!(system("M.Rate").domain.target_name(), "M.Velocity");
    }
}

#[test]
fn numeric_reference_systems_reject_invalid_axes_units_and_cycles() {
    for (body, code) in [
        (
            "DOMAIN P=COORD 0..1,0..1; N=0..1 <P[0]>;",
            "ILI-REFSYSTEM-AXIS",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1; N=0..1 <P[3]>;",
            "ILI-REFSYSTEM-DOMAIN",
        ),
        ("DOMAIN P=0..1; N=0..1 <P[1]>;", "ILI-REFSYSTEM-DOMAIN"),
        (
            "DOMAIN P=COORD 0..1,0..1; N=0..1 <P>;",
            "ILI-REFSYSTEM-DOMAIN",
        ),
        (
            "DOMAIN P=0..1 [INTERLIS.m]; N=0..1 [INTERLIS.s] <P>;",
            "ILI-REFSYSTEM-UNIT",
        ),
        (
            "DOMAIN P=0..1 [INTERLIS.m]; N=0..1 <P>;",
            "ILI-REFSYSTEM-UNIT",
        ),
        (
            "DOMAIN P=COORD 0..1,0..1,ROTATION 1 -> 2; N=0..1 [INTERLIS.m] <P>;",
            "ILI-REFSYSTEM-DIRECTION-UNIT",
        ),
        ("DOMAIN P=0..1 <N>; N=0..1 <P>;", "ILI-REFSYSTEM-CYCLE"),
        (
            "DOMAIN P=COORD 0..1 <N>,0..1; N=0..1 <P[1]>;",
            "ILI-REFSYSTEM-CYCLE",
        ),
        (
            "DOMAIN P=0..1; Q=0..1; N=0..1 <P>; Changed EXTENDS N=0..1 <Q>;",
            "ILI-REFSYSTEM-EXTENSION",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn geometric_extensions_keep_the_origin_and_rules_of_inherited_tolerances() {
    use interlis::OverlapTolerance;
    let old = "INTERLIS 2.3; MODEL Old AT \"x\" VERSION \"1\" = DOMAIN P=COORD 0.000..1.000,0.000..1.000; S=SURFACE WITH (STRAIGHTS) VERTEX P WITHOUT OVERLAPS > 0.0001; END Old.";
    let new = "INTERLIS 2.4; MODEL New AT \"x\" VERSION \"1\" = IMPORTS Old; DOMAIN Refined EXTENDS Old.S=SURFACE; END New.";
    let report = outcome(
        request(vec![("new", new.into()), ("old", old.into())]),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(
        matches!(graph.effective_type("New.Refined"), Some(TypeInfo::Geometry(geometry)) if matches!(&geometry.tolerance, Some(OverlapTolerance::Explicit(value)) if value.original() == "0.0001"))
    );

    let report = outcome(
        single("DOMAIN P=COORD 0.000..10.000,0.000..10.000; Q EXTENDS P=COORD 1.000..9.000,1.000..9.000;
            G=SURFACE WITH (STRAIGHTS) VERTEX P; X EXTENDS G=AREA VERTEX Q WITHOUT OVERLAPS;
            Smaller=SURFACE WITH (STRAIGHTS) VERTEX Q;
            STRUCTURE Base=geom:SURFACE WITH (STRAIGHTS) VERTEX P; END Base;
            STRUCTURE Child EXTENDS Base=geom (EXTENDED):Smaller; END Child;"),
        Status::CheckedSubset,
        None,
    );
    assert!(
        matches!(report.graph().unwrap().effective_type("M.X"), Some(TypeInfo::Geometry(geometry)) if matches!(&geometry.tolerance, Some(OverlapTolerance::TrackWidth(Some(domain))) if domain.target_name()=="M.P"))
    );
}

#[test]
fn generic_contexts_follow_imports_and_narrow_in_extending_topics() {
    let base = r#"
        INTERLIS 2.4; MODEL Base AT "x" VERSION "1" =
          DOMAIN
            Generic (GENERIC)=COORD NUMERIC,NUMERIC;
            West=COORD 0..10,0..10 REFSYS "EPSG:21781";
            East=COORD 20..30,20..30 REFSYS "EPSG:2056";
          CONTEXT standard=Generic=West OR East;
          TOPIC T=DEFERRED GENERICS Generic;
            CLASS Point=position:Generic; END Point;
          END T;
        END Base."#;
    let refined = r#"
        INTERLIS 2.4; MODEL Refined AT "x" VERSION "1" =
          IMPORTS Base;
          DOMAIN Local EXTENDS Base.East=COORD 22..29,22..29;
          CONTEXT local=Base.Generic=Local;
          TOPIC T EXTENDS Base.T=END T;
        END Refined."#;
    let report = outcome(
        request(vec![("base", base.into()), ("refined", refined.into())]),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    assert!(graph.declaration("Base.Generic").unwrap().is_generic());
    let mapping = &graph
        .declaration("Base.standard")
        .unwrap()
        .context_mappings()[0];
    assert_eq!(mapping.generic.target_name(), "Base.Generic");
    assert_eq!(mapping.alternatives[0].target_name(), "Base.West");
    let old = &graph.declaration("Base.T").unwrap().generic_bindings()[0];
    assert!(old.deferred);
    assert_eq!(old.concrete_domains, ["Base.West", "Base.East"]);
    assert_eq!(old.contexts, ["Base.standard"]);
    assert_eq!(
        graph
            .declaration("Base.T")
            .unwrap()
            .declared_deferred_generics()[0]
            .declared_name(),
        "Generic"
    );
    let current = &graph.declaration("Refined.T").unwrap().generic_bindings()[0];
    assert!(!current.deferred);
    assert_eq!(current.generic_domain, "Base.Generic");
    assert_eq!(current.concrete_domains, ["Refined.Local"]);
    assert_eq!(current.contexts, ["Refined.local"]);
}

#[test]
fn generic_contexts_check_shape_availability_and_deferred_use() {
    for (body, code) in [
        ("DOMAIN G (GENERIC)=TEXT;", "ILI-GENERIC-DOMAIN"),
        (
            "DOMAIN G (GENERIC)=COORD NUMERIC; P EXTENDS G=COORD 0..1,0..1;",
            "ILI-GENERIC-DIMENSIONS",
        ),
        (
            "DOMAIN G=COORD 0..1; P=COORD 0..1; CONTEXT C=G=P;",
            "ILI-CONTEXT-GENERIC",
        ),
        (
            "DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; P=COORD 0..1; CONTEXT C=G=P;",
            "ILI-CONTEXT-DIMENSIONS",
        ),
        (
            "DOMAIN G (GENERIC)=COORD 0..1,0..1; P=COORD 0..2,0..1; CONTEXT C=G=P;",
            "ILI-CONTEXT-RANGE",
        ),
        (
            "DOMAIN G (GENERIC)=COORD NUMERIC; TOPIC T=CLASS C=x:G; END C; END T;",
            "ILI-GENERIC-CONTEXT-MISSING",
        ),
        (
            "DOMAIN G (GENERIC)=COORD NUMERIC; A=COORD 0..1; B=COORD 2..3; CONTEXT C=G=A OR B; TOPIC T=CLASS C=x:G; END C; END T;",
            "ILI-GENERIC-DEFERRED-REQUIRED",
        ),
        (
            "DOMAIN G (GENERIC)=COORD NUMERIC; A=COORD 0..1; CONTEXT C=G=A; TOPIC T=DEFERRED GENERICS G; END T;",
            "ILI-GENERIC-DEFERRED-UNUSED",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn model_header_preserves_declared_metadata_and_registered_charset() {
    let report = outcome(
        request(vec![("headers", r#"INTERLIS 2.4;
            CONTRACTED TYPE MODEL Metadata (de) NOINCREMENTALTRANSFER AT "../issuer%20name" VERSION "v1" // version detail // =
                CHARSET "csUTF8"; XMLNS "urn:namespace:metadata";
                DOMAIN D=TEXT*5;
            END Metadata."#.into())]),
        Status::CheckedSubset,
        None,
    );
    let model = &report.graph().unwrap().models()[0];
    assert_eq!(model.kind(), interlis::ModelKind::Type);
    assert!(model.no_incremental_transfer());
    assert_eq!(model.issuer(), "../issuer%20name");
    assert_eq!(model.declared_charset(), Some("csUTF8"));
    assert_eq!(model.charset(), Some("UTF-8"));
    assert_eq!(model.xml_namespace(), Some("urn:namespace:metadata"));
    assert_eq!(model.effective_xml_namespace(), "urn:namespace:metadata");
    let charset = report
        .sources()
        .location(model.charset_span().unwrap())
        .unwrap();
    assert_eq!(charset.start_line, 3);
    assert!(model.explanation().unwrap().contains("version detail"));
}

#[test]
fn invalid_model_header_values_are_rejected_at_the_original_literal() {
    for value in [
        "https://example.org/a b",
        "https://example.org/µ",
        "http://[:::1]/",
        "test%S0",
        "9scheme:opaque",
    ] {
        let text = format!("INTERLIS 2.4; MODEL M AT \"{value}\" VERSION \"1\" = END M.");
        let report = outcome(
            request(vec![("issuer", text.clone())]),
            Status::Rejected,
            Some("ILI-MODEL-ISSUER"),
        );
        let location = report
            .sources()
            .location(report.diagnostics()[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!(
            &text[location.start_byte as usize..location.end_byte as usize],
            format!("\"{value}\"")
        );
    }
    for (options, code) in [
        ("CHARSET \"utf8\";", "ILI-MODEL-CHARSET"),
        ("XMLNS \"\";", "ILI-MODEL-XMLNS"),
        ("XMLNS \"https://example.org:bad/path\";", "ILI-MODEL-XMLNS"),
        ("XMLNS \"urn:example?x=[y]\";", "ILI-MODEL-XMLNS"),
        (
            "XMLNS \"http://www.w3.org/2000/xmlns/\";",
            "ILI-MODEL-XMLNS",
        ),
    ] {
        outcome(single(options), Status::Rejected, Some(code));
    }
    for issuer in [
        "https://[2001:db8::1]/issuer?q=t%20v",
        "pkg://left@@right/path",
        "//#local",
    ] {
        let report = outcome(
            request(vec![(
                "issuer",
                format!("INTERLIS 2.4; MODEL M AT \"{issuer}\" VERSION \"1\" = END M."),
            )]),
            Status::CheckedSubset,
            None,
        );
        assert_eq!(
            report.graph().unwrap().models()[0].effective_xml_namespace(),
            "http://www.interlis.ch/xtf/2.4/M"
        );
    }
    outcome(
        single("XMLNS \"future://[v1.a]/path\";"),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn formatted_temporal_ranges_bind_domains_and_keep_exact_written_bounds() {
    let report = outcome(
        single(
            r#"DOMAIN Calendar = DATE;
           Years = FORMAT Calendar "2010-02-31".."2030-01-02";
           Narrow EXTENDS Years = "2015-01-01".."2025-12-31";
           Clock = FORMAT INTERLIS.XMLTime "-00:-00:-00.000".."23:59:59.999";
           Stamp EXTENDS INTERLIS.XMLDateTime = "2020-01-01T00:00:00.000".."2020-01-02T12:30:00.001";
           TOPIC T = CLASS Appointment = time : Clock; day: INTERLIS.XMLDate; starts : Stamp; END Appointment;
           CLASS Precise EXTENDS Appointment = day (EXTENDED):INTERLIS.XMLDateTime; END Precise; END T;"#,
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let Some(TypeInfo::TemporalRange(narrow)) = graph.effective_type("M.Narrow") else {
        panic!("expected a formatted date range");
    };
    assert_eq!(narrow.base.target_name(), "M.Years");
    assert_eq!(narrow.lower.components, [2015, 1, 1]);
    assert_eq!(narrow.upper.components, [2025, 12, 31]);
    assert_eq!(narrow.upper.original, "2025-12-31");
    let Some(TypeInfo::TemporalRange(stamp)) = graph.effective_type("M.T.Appointment.starts")
    else {
        panic!("expected a formatted timestamp range");
    };
    assert_eq!(stamp.kind, TemporalKind::DateTime);
    assert_eq!(stamp.upper.components, [2020, 1, 2, 12, 30, 0, 1]);
    assert!(matches!(
        graph.effective_type("M.T.Precise.day"),
        Some(TypeInfo::Temporal(TemporalKind::DateTime))
    ));
    let location = report
        .sources()
        .location(stamp.upper.source_span())
        .unwrap();
    let source = report.sources().files()[0].text();
    assert_eq!(
        &source[location.start_byte as usize..location.end_byte as usize],
        "\"2020-01-02T12:30:00.001\""
    );
}

#[test]
fn formatted_temporal_ranges_check_edition_component_precision_and_inheritance() {
    for (body, code) in [
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLDate "2010-1-1".."2020-12-31";"#,
            "ILI-FORMAT-BOUNDS",
        ),
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLDate "2010-01-32".."2020-12-31";"#,
            "ILI-FORMAT-BOUNDS",
        ),
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLTime "10:00:00.00".."20:00:00.000";"#,
            "ILI-FORMAT-BOUNDS",
        ),
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLTime "-00:-00:-00.001".."20:00:00.000";"#,
            "ILI-FORMAT-BOUNDS",
        ),
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLTime "10:00:00.010".."10:00:00.001";"#,
            "ILI-FORMAT-BOUNDS",
        ),
        (
            r#"DOMAIN D=FORMAT INTERLIS.BOOLEAN "1".."2";"#,
            "ILI-FORMAT-BASE",
        ),
        (r#"DOMAIN D="1".."2";"#, "ILI-FORMAT-BASE"),
        (
            r#"DOMAIN D=FORMAT INTERLIS.XMLDate "2010-01-01".."2020-12-31";
                 Wide EXTENDS D="2009-01-01".."2020-12-31";"#,
            "ILI-FORMAT-BOUNDS",
        ),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
    let report = outcome(
        request(vec![
            (
                "legacy",
                r#"INTERLIS 2.3; MODEL Legacy AT "x" VERSION "1" =
            DOMAIN Calendar EXTENDS INTERLIS.XMLDate = "1900-1-1".."2050-12-31"; END Legacy."#
                    .into(),
            ),
            (
                "consumer",
                r#"INTERLIS 2.4; MODEL Consumer AT "x" VERSION "1" =
             IMPORTS Legacy; DOMAIN Alias EXTENDS Legacy.Calendar=MANDATORY;
             Narrow=FORMAT Alias "2010-1-1".."2020-12-31"; END Consumer."#
                    .into(),
            ),
        ]),
        Status::CheckedSubset,
        None,
    );
    let Some(TypeInfo::TemporalRange(narrow)) =
        report.graph().unwrap().effective_type("Consumer.Narrow")
    else {
        panic!("expected a bound imported format");
    };
    assert_eq!(narrow.language, interlis::LanguageVersion::V23);
    for (base, extension) in [("2.3", "2.4"), ("2.4", "2.3")] {
        outcome(
            request(vec![
                (
                    "base",
                    format!(
                        r#"INTERLIS {base}; MODEL Base AT "x" VERSION "1" =
                    DOMAIN Calendar=FORMAT INTERLIS.XMLDate "2010-01-01".."2030-12-31"; END Base."#
                    ),
                ),
                (
                    "extension",
                    format!(
                        r#"INTERLIS {extension}; MODEL Extension AT "x" VERSION "1" =
                    IMPORTS Base; DOMAIN Narrow EXTENDS Base.Calendar =
                        FORMAT INTERLIS.XMLDate "2020-01-01".."2025-12-31"; END Extension."#
                    ),
                ),
            ]),
            Status::Rejected,
            Some("ILI-FORMAT-EXTENSION"),
        );
    }
}

#[test]
fn numeric_subdivisions_use_effective_attribute_order_and_exact_unit_periods() {
    let report = outcome(
        single(
            "UNIT Hour=2*30 [INTERLIS.min]; Minute=3600/60 [INTERLIS.s];
             STRUCTURE Clock =
                hours: 0..23 [Hour];
                CONTINUOUS SUBDIVISION minutes: 0..59 CIRCULAR [Minute];
                CONTINUOUS SUBDIVISION seconds: 0.000..59.999 CIRCULAR [INTERLIS.s];
             END Clock;
             STRUCTURE Shift EXTENDS Clock =
                hours (EXTENDED): 6..18 [Hour]; marker: BOOLEAN;
             END Shift;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let members = graph
        .effective_attributes("M.Shift")
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(
        members
            .iter()
            .map(|member| member.qualified_name())
            .collect::<Vec<_>>(),
        [
            "M.Shift.hours",
            "M.Clock.minutes",
            "M.Clock.seconds",
            "M.Shift.marker"
        ]
    );
    let subdivision = members[2].subdivision().unwrap();
    assert!(subdivision.continuous);
    let location = report
        .sources()
        .location(subdivision.source_span())
        .unwrap();
    let text = report.sources().files()[0].text();
    assert_eq!(
        &text[location.start_byte as usize..location.end_byte as usize],
        "CONTINUOUS SUBDIVISION"
    );
}

#[test]
fn numeric_subdivisions_reject_wrong_types_units_and_periods() {
    for (body, code) in [
        ("SUBDIVISION parts:0..9;", "ILI-SUBDIVISION-PREDECESSOR"),
        (
            "fraction:0.0..9.0; SUBDIVISION parts:0..9;",
            "ILI-SUBDIVISION-INTEGER",
        ),
        (
            "whole:0..9; SUBDIVISION parts:-1..9;",
            "ILI-SUBDIVISION-RANGE",
        ),
        (
            "whole:0..9; SUBDIVISION parts:BOOLEAN;",
            "ILI-SUBDIVISION-TYPE",
        ),
        (
            "whole:BOOLEAN; SUBDIVISION parts:0..1;",
            "ILI-SUBDIVISION-PREDECESSOR-TYPE",
        ),
        (
            "hours:0..23 [INTERLIS.h]; CONTINUOUS SUBDIVISION minutes:0..59 [INTERLIS.min];",
            "ILI-SUBDIVISION-CIRCULAR",
        ),
        (
            "hours:0..23 [INTERLIS.h]; CONTINUOUS SUBDIVISION minutes:0..58 CIRCULAR [INTERLIS.min];",
            "ILI-SUBDIVISION-FACTOR",
        ),
        (
            "hours:0..23 [INTERLIS.h]; CONTINUOUS SUBDIVISION length:0..59 CIRCULAR [INTERLIS.m];",
            "ILI-SUBDIVISION-FACTOR",
        ),
        (
            "minutes:0..59 [INTERLIS.min]; CONTINUOUS SUBDIVISION seconds:0.000..59.998 CIRCULAR [INTERLIS.s];",
            "ILI-SUBDIVISION-FACTOR",
        ),
    ] {
        outcome(
            single(&format!("STRUCTURE Clock={body} END Clock;")),
            Status::Rejected,
            Some(code),
        );
    }
    let override_report = outcome(
        request(vec![("inherited-subdivision", r#"INTERLIS 2.3; MODEL M AT "x" VERSION "1" =
             STRUCTURE Clock=hours:0..23 [INTERLIS.h]; SUBDIVISION minutes:0..59 [INTERLIS.min]; END Clock;
             STRUCTURE Fraction EXTENDS Clock=hours (EXTENDED):0.0..23.0 [INTERLIS.h]; END Fraction;
             END M."#.into())]),
        Status::Rejected,
        Some("ILI-SUBDIVISION-INTEGER"),
    );
    let location = override_report
        .sources()
        .location(override_report.diagnostics()[0].primary.as_ref().unwrap())
        .unwrap();
    assert_eq!(location.start_line, 3);
    for body in [
        "UNIT Opaque=FUNCTION // dependency on an external measurement // [INTERLIS.s];
             STRUCTURE Clock=coarse:0..23 [Opaque];
                CONTINUOUS SUBDIVISION fine:0..59 CIRCULAR [INTERLIS.s]; END Clock;",
        "UNIT First; Second;
             STRUCTURE Clock=coarse:0..23 [First];
                CONTINUOUS SUBDIVISION fine:0..59 CIRCULAR [Second]; END Clock;",
        "UNIT First;
             STRUCTURE Clock=coarse:0..23 [First];
                CONTINUOUS SUBDIVISION fine:0..59 CIRCULAR [INTERLIS.s]; END Clock;",
        "UNIT Count EXTENDS INTERLIS.DIMENSIONLESS; Quotient=(INTERLIS.m/INTERLIS.m);
             STRUCTURE Clock=coarse:0..23 [Count];
                CONTINUOUS SUBDIVISION fine:0..59 CIRCULAR [Quotient]; END Clock;",
    ] {
        outcome(single(body), Status::CheckedSubset, None);
    }
    for units in [
        "UNIT Opaque; Coarse=(Opaque*INTERLIS.h); Fine=(Opaque*INTERLIS.m);",
        "UNIT First (ABSTRACT); Second (ABSTRACT); Coarse EXTENDS First; Fine EXTENDS Second;",
    ] {
        let body = format!(
            "{units} STRUCTURE Clock=coarse:0..23 [Coarse];
                CONTINUOUS SUBDIVISION fine:0..59 CIRCULAR [Fine]; END Clock;"
        );
        outcome(
            single(&body),
            Status::Rejected,
            Some("ILI-SUBDIVISION-FACTOR"),
        );
    }
}

#[test]
fn equivalent_unit_factors_cancel_before_large_products_are_evaluated() {
    use std::fmt::Write;
    let mut body = String::from("UNIT First0=1.005 [INTERLIS.s]; Second0=1.0050 [INTERLIS.s];");
    for index in 1..=21 {
        let prior = index - 1;
        write!(
            body,
            " First{index}=(First{prior}*First{prior}); Second{index}=(Second{prior}*Second{prior});"
        )
        .unwrap();
    }
    body.push_str(
        " STRUCTURE Clock=coarse:0..23 [First21];
            CONTINUOUS SUBDIVISION fine:0..0 CIRCULAR [Second21]; END Clock;",
    );
    outcome(single(&body), Status::CheckedSubset, None);
}

#[test]
fn scalar_constraints_bind_inherited_attributes_and_domain_values() {
    use interlis::{BinaryOperator, BoundExpressionKind, ConstraintRule, ExpressionType};
    let report = outcome(
        single(
            "DOMAIN Count=0..100 CONSTRAINTS nonnegative: THIS >= 0;
             TOPIC T=
                CLASS Base=count:Count; flag:BOOLEAN; label:TEXT*20; END Base;
                CLASS Child EXTENDS Base =
                    MANDATORY CONSTRAINT condition: flag AND (THIS->count >= 2);
                    CONSTRAINT completeness: >= 90.5 % DEFINED(label);
                END Child;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let domain = &graph.declaration("M.Count").unwrap().constraints()[0];
    assert!(matches!(domain.rule, ConstraintRule::Domain));
    assert_eq!(
        graph
            .expression(domain.expression.unwrap())
            .unwrap()
            .value_type,
        ExpressionType::Boolean
    );
    let constraints = graph.declaration("M.T.Child").unwrap().constraints();
    assert_eq!(constraints[0].name.as_deref(), Some("condition"));
    let lhs = match graph
        .expression(constraints[0].expression.unwrap())
        .unwrap()
        .kind
    {
        BoundExpressionKind::Binary {
            operator: BinaryOperator::And,
            left,
            ..
        } => left,
        _ => panic!("expected a Boolean conjunction"),
    };
    let lhs = graph.expression(lhs).unwrap();
    let BoundExpressionKind::Path { steps } = &lhs.kind else {
        panic!("expected an inherited attribute");
    };
    assert_eq!(steps[0].reference.target_name(), "M.T.Base.flag");
    let location = report.sources().location(lhs.source_span()).unwrap();
    let original = report.sources().files()[0].text();
    assert_eq!(
        &original[location.start_byte as usize..location.end_byte as usize],
        "flag"
    );
    let ConstraintRule::Plausibility { ref percentage, .. } = constraints[1].rule else {
        panic!("expected a plausibility constraint");
    };
    assert_eq!(percentage.original(), "90.5");
}

#[test]
fn scalar_constraints_reject_wrong_operands_and_missing_attribute_names() {
    for (constraint, code) in [
        ("MANDATORY CONSTRAINT count;", "ILI-CONSTRAINT-BOOLEAN"),
        (
            "MANDATORY CONSTRAINT count AND flag;",
            "ILI-EXPRESSION-OPERANDS",
        ),
        (
            "MANDATORY CONSTRAINT label < \"other\";",
            "ILI-EXPRESSION-OPERANDS",
        ),
        (
            "MANDATORY CONSTRAINT count == flag;",
            "ILI-EXPRESSION-OPERANDS",
        ),
        ("MANDATORY CONSTRAINT missing;", "ILI-PATH-NAME"),
        ("CONSTRAINT >= 100.1% flag;", "ILI-CONSTRAINT-PERCENTAGE"),
    ] {
        outcome(
            single(&format!(
                "TOPIC T=CLASS C=count:0..10; flag:BOOLEAN; label:TEXT*4; {constraint} END C; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        single(
            "TOPIC T=CLASS C=x:0..100 [INTERLIS.m]; y:0..100 [INTERLIS.s]; flag:BOOLEAN;
            MANDATORY CONSTRAINT x < y AND flag >= #false; END C; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn constraint_errors_keep_name_candidates_and_check_independent_conditions() {
    let report = outcome(
        request(vec![
            ("left", "INTERLIS 2.4; MODEL Left AT \"x\" VERSION \"1\" = PARAMETER flag:BOOLEAN; END Left.".into()),
            ("right", "INTERLIS 2.4; MODEL Right AT \"x\" VERSION \"1\" = PARAMETER flag:BOOLEAN; END Right.".into()),
            ("main", "INTERLIS 2.4; MODEL Main AT \"x\" VERSION \"1\" = IMPORTS UNQUALIFIED Left, UNQUALIFIED Right;
             TOPIC T=CLASS Obj=MANDATORY CONSTRAINT PARAMETER flag; END Obj; END T; END Main.".into()),
        ]),
        Status::Rejected,
        Some("ILI-EXPRESSION-NAME-AMBIGUOUS"),
    );
    assert_eq!(report.diagnostics()[0].related.len(), 2);
    let ignored_role = "INTERLIS 2.4; MODEL Unrelated AT \"x\" VERSION \"1\" =
        TOPIC T=CLASS Obj=END Obj; ASSOCIATION Link=left -- Obj; missing -- Obj; END Link; END T;
        END Unrelated.";
    let input = request(vec![
        (
            "main",
            "INTERLIS 2.4; MODEL Main AT \"x\" VERSION \"1\" =
            TOPIC T=CLASS Obj=MANDATORY CONSTRAINT missing != UNDEFINED; END Obj; END T; END Main."
                .into(),
        ),
        ("unrelated", ignored_role.into()),
    ]);
    outcome(input, Status::Rejected, Some("ILI-PATH-NAME"));
    outcome(
        single(
            "TOPIC T=CLASS Obj=MANDATORY CONSTRAINT THIS == THIS;
                MANDATORY CONSTRAINT \"a\" < \"z\"; END Obj; END T;",
        ),
        Status::Rejected,
        Some("ILI-EXPRESSION-OPERANDS"),
    );
}

#[test]
fn function_signatures_and_calls_bind_parameters_in_order() {
    let report = outcome(
        request(vec![
            (
                "library",
                r#"INTERLIS 2.3; CONTRACTED TYPE MODEL Library AT "issuer" VERSION "1" =
                DOMAIN Small=0..10;
                FUNCTION permits(value:Small; label:TEXT*5):BOOLEAN // policy //;
                END Library."#
                    .into(),
            ),
            (
                "client",
                r#"INTERLIS 2.4; MODEL Client AT "issuer" VERSION "1" =
                IMPORTS Library; TOPIC T=CLASS Obj=
                    count:0..8; label:TEXT*3;
                    MANDATORY CONSTRAINT permitted: Library.permits(count,label);
                    MANDATORY CONSTRAINT nonempty: INTERLIS.len(INTERLIS.trim(label)) > 0;
                END Obj; END T; END Client."#
                    .into(),
            ),
        ]),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let function = graph
        .declaration("Library.permits")
        .unwrap()
        .function()
        .unwrap();
    assert!(function.explanation.as_deref().unwrap().contains("policy"));
    assert_eq!(
        graph
            .function_arguments("Library.permits")
            .unwrap()
            .map(|arg| arg.name())
            .collect::<Vec<_>>(),
        ["value", "label"],
    );
    let call = graph.declaration("Client.T.Obj").unwrap().constraints()[0]
        .expression
        .unwrap();
    let interlis::BoundExpressionKind::Call {
        function,
        arguments,
    } = &graph.expression(call).unwrap().kind
    else {
        panic!("expected a bound function call");
    };
    assert_eq!(function.target_name(), "Library.permits");
    assert_eq!(arguments.len(), 2);
    assert!(matches!(
        graph.expression_declared_type(call),
        Some(TypeInfo::Boolean)
    ));
}

#[test]
fn function_arguments_and_23_contracts_have_specific_diagnostics() {
    for (call, code) in [
        ("check()", "ILI-FUNCTION-ARITY"),
        ("check(#red,5)", "ILI-FUNCTION-ARITY"),
        ("check(#true)", "ILI-FUNCTION-ARGUMENT"),
        ("check(UNDEFINED)", "ILI-FUNCTION-ARGUMENT"),
        ("check(11)", "ILI-FUNCTION-ARGUMENT"),
        ("check(outside)", "ILI-FUNCTION-ARGUMENT"),
        ("Missing(0)", "ILI-FUNCTION-NAME"),
        ("INTERLIS.len(note) > 0", "ILI-FUNCTION-ARGUMENT"),
    ] {
        outcome(
            single(&format!(
                "FUNCTION check(value:MANDATORY 0..10):BOOLEAN;
                 TOPIC T=CLASS Obj=outside:0..11; note:MTEXT*4;
                 MANDATORY CONSTRAINT {call}; END Obj; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        request(vec![(
            "legacy",
            "INTERLIS 2.3; MODEL Legacy AT \"x\" VERSION \"1\" =
            FUNCTION check(value:BOOLEAN):BOOLEAN; END Legacy."
                .into(),
        )]),
        Status::Rejected,
        Some("ILI-FUNCTION-CONTRACTED"),
    );
    outcome(
        single(
            "FUNCTION check(value:0..10 [INTERLIS.m]):BOOLEAN;
                TOPIC T=CLASS Obj=MANDATORY CONSTRAINT check(0); END Obj; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn function_text_arguments_use_canonical_length_and_undefined_values() {
    for edition in ["2.3", "2.4"] {
        for (source, status) in [
            ("é", Status::CheckedSubset),
            ("e\u{0301}", Status::CheckedSubset),
            ("가", Status::Rejected),
            ("", Status::Rejected),
            ("\u{0301}", Status::Rejected),
        ] {
            let text = format!(
                r#"INTERLIS {edition}; CONTRACTED MODEL M AT "issuer" VERSION "1" =
                   FUNCTION check(value:MANDATORY TEXT*1):BOOLEAN;
                   TOPIC T=CLASS Obj=MANDATORY CONSTRAINT check("{source}"); END Obj; END T; END M."#
            );
            let report = outcome(
                request(vec![("text", text)]),
                status,
                (status == Status::Rejected).then_some("ILI-FUNCTION-ARGUMENT"),
            );
            if let Some(graph) = report.graph() {
                let call = graph.declaration("M.T.Obj").unwrap().constraints()[0]
                    .expression
                    .unwrap();
                let interlis::BoundExpressionKind::Call { arguments, .. } =
                    &graph.expression(call).unwrap().kind
                else {
                    panic!("expected a function call");
                };
                assert!(matches!(
                    &graph.expression(arguments[0]).unwrap().kind,
                    interlis::BoundExpressionKind::Text { value, canonical_length: 1 } if value == source
                ));
            }
        }
    }
    outcome(
        single(
            r#"FUNCTION check(value:MANDATORY NAME):BOOLEAN;
                   TOPIC T=CLASS Obj=MANDATORY CONSTRAINT check(""); END Obj; END T;"#,
        ),
        Status::Rejected,
        Some("ILI-FUNCTION-ARGUMENT"),
    );
}

#[test]
fn object_paths_bind_structure_reference_list_and_coordinate_steps() {
    use interlis::{BoundExpressionKind, PathSelection};
    let report = outcome(
        single(
            "TOPIC T=
               STRUCTURE Part=ready:BOOLEAN; END Part;
               CLASS Target=label:TEXT*4; END Target;
               CLASS Item=parts:LIST {0..3} OF Part; single:Part;
                   position:COORD 0.0..9.0,0.00..8.00;
                   pointer:REFERENCE TO Target;
                   MANDATORY CONSTRAINT parts[FIRST]->ready AND single->ready;
                   MANDATORY CONSTRAINT pointer->label == \"test\";
                   MANDATORY CONSTRAINT position[2] > 0;
               END Item;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let rules = graph.declaration("M.T.Item").unwrap().constraints();
    let first_operand = |id| match graph.expression(id).unwrap().kind {
        BoundExpressionKind::Binary { left, .. } => left,
        _ => panic!("expected a binary expression"),
    };
    let item = graph
        .expression(first_operand(rules[0].expression.unwrap()))
        .unwrap();
    let BoundExpressionKind::Path { steps } = &item.kind else {
        panic!("expected a path");
    };
    assert_eq!(steps[0].reference.target_name(), "M.T.Item.parts");
    assert!(matches!(steps[0].selection, Some(PathSelection::First)));
    assert_eq!(steps[1].reference.target_name(), "M.T.Part.ready");
    let span = report
        .sources()
        .location(steps[0].selection_span().unwrap())
        .unwrap();
    let source = report.sources().files()[0].text();
    assert_eq!(
        &source[span.start_byte as usize..span.end_byte as usize],
        "FIRST"
    );
    let item = graph
        .expression(first_operand(rules[1].expression.unwrap()))
        .unwrap();
    let BoundExpressionKind::Path { steps } = &item.kind else {
        panic!("expected a path");
    };
    assert_eq!(steps[1].reference.target_name(), "M.T.Target.label");
    let Some(TypeInfo::Numeric(axis)) =
        graph.expression_declared_type(first_operand(rules[2].expression.unwrap()))
    else {
        panic!("expected the selected coordinate axis");
    };
    assert_eq!(axis.bounds.as_ref().unwrap().upper.original(), "8.00");
}

#[test]
fn object_path_selectors_and_traversal_enforce_actual_shapes() {
    for (path, code) in [
        ("parts[0]->ready", "ILI-PATH-INDEX"),
        ("parts[4]->ready", "ILI-PATH-INDEX"),
        ("bag[FIRST]->ready", "ILI-PATH-SELECTOR"),
        ("parts->ready", "ILI-PATH-MULTIPLICITY"),
        ("point[3] > 0", "ILI-PATH-AXIS"),
        ("label->ready", "ILI-PATH-TRAVERSAL"),
        ("parts[LAST]->absent", "ILI-PATH-NAME"),
    ] {
        outcome(
            single(&format!(
                "TOPIC T=STRUCTURE Piece=ready:BOOLEAN; END Piece;
                 CLASS Item=parts:LIST {{0..3}} OF Piece; bag:BAG {{0..3}} OF Piece;
                 point:COORD 0..1,0..1; label:TEXT*5;
                 MANDATORY CONSTRAINT {path}; END Item; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
}

#[test]
fn object_paths_resolve_reference_targets_in_the_extending_topic() {
    let report = outcome(
        single(
            "TOPIC Parent=OID AS INTERLIS.I32OID;
                CLASS A=active:BOOLEAN; END A;
                STRUCTURE Link=pointer:REFERENCE TO A; END Link;
                CLASS Item=link:Link; END Item;
             END Parent;
             TOPIC Child EXTENDS Parent=
                CLASS A (EXTENDED)=extra:BOOLEAN; END A;
                CLASS Item (EXTENDED)=
                    MANDATORY CONSTRAINT link->pointer->extra;
                END Item;
             END Child;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let expression = graph.declaration("M.Child.Item").unwrap().constraints()[0]
        .expression
        .unwrap();
    let interlis::BoundExpressionKind::Path { steps } = &graph.expression(expression).unwrap().kind
    else {
        panic!("expected an object path");
    };
    assert_eq!(
        steps.last().unwrap().reference.target_name(),
        "M.Child.A.extra"
    );
}

#[test]
fn same_topic_constraint_blocks_augment_the_target_in_source_order() {
    let report = outcome(
        single(
            "TOPIC T=
            CLASS Item=flag:BOOLEAN; MANDATORY CONSTRAINT original: flag; END Item;
            CONSTRAINTS OF Item=MANDATORY CONSTRAINT appended: NOT(flag); END;
            END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let constraints = report
        .graph()
        .unwrap()
        .declaration("M.T.Item")
        .unwrap()
        .constraints();
    assert_eq!(
        constraints
            .iter()
            .map(|constraint| constraint.name.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["original", "appended"]
    );
    let location = report
        .sources()
        .location(constraints[1].source_span())
        .unwrap();
    assert_eq!(location.start_line, 3);
    outcome(
        single(
            "TOPIC T=CLASS Item=flag:BOOLEAN; MANDATORY CONSTRAINT repeated:flag; END Item;
            CONSTRAINTS OF Item=MANDATORY CONSTRAINT repeated:flag; END; END T;",
        ),
        Status::Rejected,
        Some("ILI-DUPLICATE"),
    );
    outcome(
        single(
            "TOPIC Base=CLASS Item=flag:BOOLEAN; END Item; END Base;
            TOPIC Other=CONSTRAINTS OF M.Base.Item=MANDATORY CONSTRAINT flag; END; END Other;",
        ),
        Status::Rejected,
        Some("ILI-CONSTRAINT-TARGET-TOPIC"),
    );
    outcome(
        single("TOPIC Base=CLASS Item=flag:BOOLEAN; END Item; END Base;
            TOPIC Derived EXTENDS Base=CONSTRAINTS OF Item=MANDATORY CONSTRAINT flag; END; END Derived;"),
        Status::Incomplete, Some("ILI-UNSUPPORTED"),
    );
}

#[test]
fn unique_constraints_keep_scope_predicates_prefixes_and_key_order() {
    use interlis::{BoundExpressionKind, ConstraintRule, ExpressionType};
    let report = outcome(
        single(
            "TOPIC T=
               STRUCTURE Part=code:TEXT*4; label:TEXT*5; END Part;
               STRUCTURE Group=parts:LIST {0..3} OF Part; END Group;
               CLASS Item=enabled:BOOLEAN; single:Part; groups:BAG OF Group;
                   UNIQUE (BASKET) basketKey: WHERE enabled: single->label;
                   UNIQUE localKey: (LOCAL) groups->parts: code,label;
               END Item;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let constraints = graph.declaration("M.T.Item").unwrap().constraints();
    let ConstraintRule::Unique {
        basket,
        local_prefix,
        keys,
    } = &constraints[0].rule
    else {
        panic!("expected a basket key");
    };
    assert!(*basket && local_prefix.is_none());
    assert_eq!(keys.len(), 1);
    assert_eq!(
        graph
            .expression(constraints[0].expression.unwrap())
            .unwrap()
            .value_type,
        ExpressionType::Boolean
    );
    let ConstraintRule::Unique {
        basket,
        local_prefix,
        keys,
    } = &constraints[1].rule
    else {
        panic!("expected a local key");
    };
    assert!(!*basket && constraints[1].expression.is_none());
    let prefix = graph.expression(local_prefix.unwrap()).unwrap();
    let BoundExpressionKind::Path { steps } = &prefix.kind else {
        panic!("expected a path to the local structures");
    };
    assert_eq!(
        steps
            .iter()
            .map(|step| step.reference.target_name())
            .collect::<Vec<_>>(),
        ["M.T.Item.groups", "M.T.Group.parts"]
    );
    let location = report.sources().location(prefix.source_span()).unwrap();
    let text = report.sources().files()[0].text();
    assert_eq!(
        &text[location.start_byte as usize..location.end_byte as usize],
        "groups->parts"
    );
    assert_eq!(
        keys.iter()
            .map(|&key| match &graph.expression(key).unwrap().kind {
                BoundExpressionKind::Path { steps } => steps[0].reference.target_name(),
                _ => panic!("expected a key field"),
            })
            .collect::<Vec<_>>(),
        ["M.T.Part.code", "M.T.Part.label"]
    );
}

#[test]
fn unique_checks_distinguish_invalid_scopes_filters_and_paths() {
    for (rule, code) in [
        ("UNIQUE part;", "ILI-UNIQUE-KEY"),
        ("UNIQUE parts;", "ILI-UNIQUE-KEY"),
        ("UNIQUE parts->code;", "ILI-PATH-MULTIPLICITY"),
        ("UNIQUE (BASKET) (LOCAL) parts:code;", "ILI-UNIQUE-SCOPE"),
        ("UNIQUE (LOCAL) label:code;", "ILI-UNIQUE-PREFIX"),
        ("UNIQUE WHERE label: label;", "ILI-CONSTRAINT-BOOLEAN"),
    ] {
        outcome(
            single(&format!(
                "TOPIC T=STRUCTURE Piece=code:TEXT*4; END Piece;
                 CLASS C=part:Piece; parts:BAG OF Piece; label:TEXT*4;
                 {rule} END C; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        single("STRUCTURE S=code:TEXT*4; UNIQUE code; END S;"),
        Status::Rejected,
        Some("ILI-UNIQUE-SCOPE"),
    );
}

#[test]
fn local_uniqueness_can_select_23_geometry_line_attributes() {
    let report = outcome(
        request(vec![("line-attributes", r#"INTERLIS 2.3; MODEL M AT "issuer" VERSION "1" =
            TOPIC T=
                DOMAIN Point=COORD 0.00..10.00,0.00..10.00;
                STRUCTURE Boundary=key:TEXT*8; END Boundary;
                CLASS C=border:SURFACE WITH (STRAIGHTS) VERTEX Point WITHOUT OVERLAPS > 0.01 LINE ATTRIBUTES Boundary;
                    UNIQUE (LOCAL) border:key;
                END C;
            END T; END M."#.into())]),
        Status::CheckedSubset, None,
    );
    let graph = report.graph().unwrap();
    let rule = &graph.declaration("M.T.C").unwrap().constraints()[0].rule;
    let interlis::ConstraintRule::Unique { keys, .. } = rule else {
        panic!("expected a local unique key");
    };
    let interlis::BoundExpressionKind::Path { steps } = &graph.expression(keys[0]).unwrap().kind
    else {
        panic!("expected a key path");
    };
    assert_eq!(steps[0].reference.target_name(), "M.T.Boundary.key");
}

#[test]
fn role_paths_distinguish_link_endpoints_from_collections_of_related_objects() {
    let declarations = "CLASS Parent=ready:BOOLEAN; END Parent;
         CLASS Child=active:BOOLEAN; END Child;
         ASSOCIATION Link=parent -- {0..1} Parent; children -- {0..*} Child;
            UNIQUE children->active;
            MANDATORY CONSTRAINT parent->ready AND children->active;
         END Link;";
    let report = outcome(
        single(&format!(
            "TOPIC T={declarations} CONSTRAINTS OF Child=
                MANDATORY CONSTRAINT parent->ready; END; END T;"
        )),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let predicate = graph.declaration("M.T.Child").unwrap().constraints()[0]
        .expression
        .unwrap();
    let interlis::BoundExpressionKind::Path { steps } = &graph.expression(predicate).unwrap().kind
    else {
        panic!("expected a path across the association");
    };
    assert_eq!(steps[0].reference.target_name(), "M.T.Link.parent");
    assert_eq!(steps[1].reference.target_name(), "M.T.Parent.ready");
    outcome(
        single(&format!(
            "TOPIC T={declarations} CONSTRAINTS OF Parent=
                MANDATORY CONSTRAINT children->active; END; END T;"
        )),
        Status::Rejected,
        Some("ILI-PATH-MULTIPLICITY"),
    );
    outcome(
        single(&format!(
            "TOPIC T={declarations} CONSTRAINTS OF Parent=UNIQUE children; END; END T;"
        )),
        Status::Rejected,
        Some("ILI-UNIQUE-KEY"),
    );
}

#[test]
fn role_paths_check_declaration_order_multiplicity_and_following_external_access() {
    let base = "TOPIC T=OID AS INTERLIS.I32OID;
        CLASS A=END A; CLASS B=active:BOOLEAN; END B; CLASS C=END C;";
    outcome(
        single(&format!(
            "{base} ASSOCIATION Triple=origin -- {{0..1}} A;
                 counterpart -- {{0..1}} B; auxiliary -- {{0..1}} C; END Triple;
             CONSTRAINTS OF A=MANDATORY CONSTRAINT counterpart->active; END; END T;"
        )),
        Status::Rejected,
        Some("ILI-PATH-MULTIPLICITY"),
    );
    outcome(
        single(&format!(
            "{base} ASSOCIATION Pair (OID)=origin -- {{0..1}} A; counterpart -- {{0..1}} C; END Pair;
             ASSOCIATION Link=item -- {{0..1}} Pair; receivers -- {{0..*}} B; END Link;
             CONSTRAINTS OF Pair=MANDATORY CONSTRAINT receivers->active; END; END T;"
        )),
        Status::Rejected, Some("ILI-PATH-MULTIPLICITY"),
    );
    outcome(
        single(
            "TOPIC T=CLASS A=MANDATORY CONSTRAINT counterpart->active; END A;
            CLASS B=active:BOOLEAN; END B;
            ASSOCIATION Pair=origin -- {0..1} A; counterpart -- {0..1} B; END Pair;
            END T;",
        ),
        Status::Rejected,
        Some("ILI-PATH-ROLE-ORDER"),
    );
    let relation = "TOPIC T=OID AS INTERLIS.I32OID;
        CLASS Remote=END Remote; CLASS Local=active:BOOLEAN; END Local;
        STRUCTURE Payload=target:REFERENCE TO Remote; END Payload;
        CLASS Holder=data:Payload; END Holder;
        ASSOCIATION Link=origin (EXTERNAL) -- {0..1} Remote; endpoint -- {0..1} Local; END Link;";
    outcome(
        single(&format!(
            "{relation} CONSTRAINTS OF Holder=MANDATORY CONSTRAINT data->target->endpoint->active; END; END T;"
        )),
        Status::CheckedSubset,
        None,
    );
    outcome(
        single(&format!(
            "{relation} CONSTRAINTS OF Remote=MANDATORY CONSTRAINT endpoint->active; END; END T;"
        )),
        Status::Rejected,
        Some("ILI-PATH-NAME"),
    );
    outcome(
        single(
            "TOPIC T=STRUCTURE Details=END Details; CLASS Holder=detail:Details; END Holder;
             CLASS Other=active:BOOLEAN; END Other;
             ASSOCIATION Link=origin -- {0..1} ANYCLASS; endpoint -- {0..1} Other; END Link;
             CONSTRAINTS OF Holder=MANDATORY CONSTRAINT detail->endpoint->active; END; END T;",
        ),
        Status::Rejected,
        Some("ILI-PATH-NAME"),
    );
}

#[test]
fn existence_constraints_bind_each_required_viewable_and_attribute_path() {
    let report = outcome(
        single(
            "DOMAIN Point=COORD 0.00..10.00, 0.00..10.00;
             TOPIC Values=CLASS Item=code:TEXT*20; vertex:Point; END Item; END Values;
             TOPIC Links=DEPENDS ON Values;
                 CLASS Item=code:TEXT*5; line:POLYLINE WITH (STRAIGHTS) VERTEX Point;
                     EXISTENCE CONSTRAINT codeExists:code REQUIRED IN M.Values.Item:code OR Item:code;
                     EXISTENCE CONSTRAINT pointExists:line REQUIRED IN M.Values.Item:vertex;
                 END Item;
             END Links;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let rules = graph.declaration("M.Links.Item").unwrap().constraints();
    let interlis::ConstraintRule::Existence {
        source_path,
        required,
    } = &rules[0].rule
    else {
        panic!("expected an existence constraint");
    };
    assert!(rules[0].expression.is_none());
    assert_eq!(
        required
            .iter()
            .map(|target| target.viewable.target_name())
            .collect::<Vec<_>>(),
        ["M.Values.Item", "M.Links.Item"]
    );
    let steps = |id| match &graph.expression(id).unwrap().kind {
        interlis::BoundExpressionKind::Path { steps } => steps,
        _ => panic!("expected a bound attribute path"),
    };
    assert_eq!(
        steps(*source_path)[0].reference.target_name(),
        "M.Links.Item.code"
    );
    assert_eq!(
        steps(required[0].path)[0].reference.target_name(),
        "M.Values.Item.code"
    );
}

#[test]
fn existence_wire_preserves_paths_and_referenced_viewable_ranges() {
    let text = "INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" =
        TOPIC T=CLASS Target=x:TEXT; END Target;
        CLASS Source=y:TEXT; EXISTENCE CONSTRAINT y REQUIRED IN Target:x; END Source;
        END T; END M.";
    let request = serde_json::json!({"sources":[{"name":"model.ili","text":text}]});
    let output = interlis::analyze_json(&request.to_string()).unwrap();
    let report: serde_json::Value = serde_json::from_str(&output).unwrap();
    let source = report["model"]["declarations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|declaration| declaration["qualifiedName"] == "M.T.Source")
        .unwrap();
    let constraint = &source["constraints"][0];
    assert_eq!(constraint["source"], "model.ili");
    let path = constraint["sourcePath"].as_u64().unwrap() as usize;
    assert_eq!(
        report["model"]["expressions"][path]["steps"][0]["reference"]["targetName"],
        "M.T.Source.y"
    );
    assert_eq!(constraint["required"][0]["viewable"]["source"], "model.ili");
}

#[test]
fn existence_vertex_type_can_narrow_target_and_standard_oid_matches_text_oid() {
    for (vertex, target, status) in [
        ("Narrow", "Wide", Status::CheckedSubset),
        ("Wide", "Narrow", Status::Rejected),
    ] {
        outcome(
            single(&format!(
                "DOMAIN Wide=COORD 0..100,0..100;
                 Narrow EXTENDS Wide=COORD 10..50,10..50; TextId=OID TEXT*16;
                 TOPIC T=CLASS Lookup=point:{target}; code:TextId; END Lookup;
                 CLASS Source=line:POLYLINE WITH (STRAIGHTS) VERTEX {vertex};
                     code:INTERLIS.STANDARDOID;
                     EXISTENCE CONSTRAINT code REQUIRED IN Lookup:code;
                     EXISTENCE CONSTRAINT line REQUIRED IN Lookup:point;
                 END Source; END T;"
            )),
            status,
            (status == Status::Rejected).then_some("ILI-EXISTENCE-TYPE"),
        );
    }
    outcome(
        single(
            r#"DOMAIN Bare=COORD 0..100,0..100;
               Fine EXTENDS Bare=COORD 10..50,10..50 REFSYS "EPSG:2056";
               TOPIC T=CLASS Lookup=point:Bare; END Lookup;
               CLASS Source=line:POLYLINE WITH (STRAIGHTS) VERTEX Fine;
                   EXISTENCE CONSTRAINT line REQUIRED IN Lookup:point;
               END Source; END T;"#,
        ),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn set_constraints_bind_the_filter_and_targeted_object_sets() {
    let report = outcome(
        single(
            "FUNCTION has(objects:OBJECTS OF M.T.Base RESTRICTION (M.T.Child)):BOOLEAN;
             FUNCTION isChild(obj:OBJECT OF M.T.Child):BOOLEAN;
             TOPIC T=CLASS Base=active:BOOLEAN; END Base;
                CLASS Child EXTENDS Base=
                    MANDATORY CONSTRAINT isChild(THIS);
                    SET CONSTRAINT(BASKET) childSet: WHERE active: has(ALL) AND has(ALL(Child));
                END Child;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let rules = graph.declaration("M.T.Child").unwrap().constraints();
    let interlis::ConstraintRule::Set {
        basket: true,
        filter: Some(filter),
    } = rules[1].rule
    else {
        panic!("expected a basket set constraint with a filter");
    };
    let interlis::BoundExpressionKind::Path { steps } = &graph.expression(filter).unwrap().kind
    else {
        panic!("expected a bound filter");
    };
    assert_eq!(steps[0].reference.target_name(), "M.T.Base.active");
    let interlis::BoundExpressionKind::Binary { left, right, .. } =
        graph.expression(rules[1].expression.unwrap()).unwrap().kind
    else {
        panic!("expected two calls");
    };
    for (id, expected_explicit) in [(left, false), (right, true)] {
        let interlis::BoundExpressionKind::Call { arguments, .. } =
            &graph.expression(id).unwrap().kind
        else {
            panic!("expected a call");
        };
        assert!(matches!(graph.expression(arguments[0]).unwrap().kind,
            interlis::BoundExpressionKind::ObjectSet { explicit } if explicit == expected_explicit));
        let Some(TypeInfo::ObjectSelection(objects)) = graph.expression_declared_type(arguments[0])
        else {
            panic!("expected object selection");
        };
        assert!(objects.plural);
        assert_eq!(
            objects.targets[0].target.as_ref().unwrap().target_name(),
            "M.T.Child"
        );
    }
}

#[test]
fn set_conditions_check_scope_edition_and_all_candidate_object_types() {
    let body = "TOPIC T=CLASS Base=active:BOOLEAN; END Base;
        CLASS Child EXTENDS Base=CONSTRAINT END Child; END T;";
    for (constraint, code) in [
        (
            "SET CONSTRAINT INTERLIS.objectCount(ALL) > 0 AND active;",
            "ILI-SET-OBJECT-VALUE",
        ),
        ("SET CONSTRAINT WHERE active: #true;", "ILI-SET-OBJECTS"),
        (
            "MANDATORY CONSTRAINT INTERLIS.objectCount(ALL) > 0;",
            "ILI-CONSTRAINT-OBJECTS",
        ),
        ("SET CONSTRAINT has(ALL(Base));", "ILI-FUNCTION-ARGUMENT"),
        (
            "SET CONSTRAINT isChild(ALL) AND has(ALL);",
            "ILI-FUNCTION-ARGUMENT",
        ),
        (
            "SET CONSTRAINT INTERLIS.objectCount(ALL(M.T.Missing)) > 0;",
            "ILI-NAME-NOT-FOUND",
        ),
        (
            "SET CONSTRAINT WHERE 1: INTERLIS.objectCount(ALL) > 0;",
            "ILI-CONSTRAINT-BOOLEAN",
        ),
    ] {
        outcome(
            single(&format!(
                "FUNCTION has(objects:OBJECTS OF M.T.Child):BOOLEAN;
                 FUNCTION isChild(obj:OBJECT OF M.T.Child):BOOLEAN;
                 {}",
                body.replace("=CONSTRAINT", &format!("={constraint}"))
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        request(vec![(
            "legacy",
            "INTERLIS 2.3; MODEL M AT \"x\" VERSION \"1\" =
             TOPIC T=CLASS Child=END Child; CLASS Parent=END Parent;
             ASSOCIATION ParentOf=children -- Child; parent -- {1} Parent; END ParentOf;
             CONSTRAINTS OF Parent=MANDATORY CONSTRAINT INTERLIS.objectCount(children)>0;
                 SET CONSTRAINT #true;
             END; END T; END M."
                .into(),
        )]),
        Status::CheckedSubset,
        None,
    );
    outcome(
        single(
            "TOPIC T=STRUCTURE Child=END Child;
             CLASS Parent=children:LIST OF Child;
                MANDATORY CONSTRAINT INTERLIS.elementCount(children)>0;
             END Parent; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    outcome(
        single(
            "TOPIC T=CLASS Parent=children:LIST OF TEXT*5;
                MANDATORY CONSTRAINT INTERLIS.elementCount(children)>0;
             END Parent; END T;",
        ),
        Status::Rejected,
        Some("ILI-FUNCTION-ARGUMENT"),
    );
}

#[test]
fn object_formals_use_contextual_reference_targets_and_role_chains_keep_multiplicity() {
    outcome(
        single(
            "FUNCTION accepts(value:OBJECT OF M.Child.Item):BOOLEAN;
             TOPIC Parent=OID AS INTERLIS.I32OID;
               CLASS Item=END Item;
               STRUCTURE Wrapper=pointer:REFERENCE TO Item; END Wrapper;
             END Parent;
             TOPIC Child EXTENDS Parent=
               CLASS Item (EXTENDED)=extra:BOOLEAN; END Item;
               CLASS Tester=wrap:Wrapper; MANDATORY CONSTRAINT accepts(wrap->pointer); END Tester;
             END Child;",
        ),
        Status::CheckedSubset,
        None,
    );
    let report = outcome(
        single(
            "TOPIC T=CLASS A=END A; CLASS B=END B; CLASS C=END C;
               ASSOCIATION AB=origin -- {1} A; bs -- {0..*} B; END AB;
               ASSOCIATION BC=origin -- {1} B; cs -- {0..*} C; END BC;
               CONSTRAINTS OF A=SET CONSTRAINT WHERE INTERLIS.objectCount(bs->cs)>0:
                 INTERLIS.objectCount(ALL)>0; END;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let expressions = report.graph().unwrap().expressions();
    assert!(expressions.iter().any(|expression| {
        matches!(&expression.kind, interlis::BoundExpressionKind::Path { steps }
            if steps.len() == 2 && steps[1].reference.target_name() == "M.T.BC.cs")
            && expression.value_type == interlis::ExpressionType::ObjectSet
    }));
}

#[test]
fn enumeration_literals_bind_to_leaf_tree_and_formal_types() {
    let report = outcome(
        single(
            "DOMAIN Colour=(green, group(red)) ORDERED;
             Extended EXTENDS Colour=(group(blue)) ORDERED;
             Tree=ALL OF Extended;
             FUNCTION accepts(value:Extended):BOOLEAN;
             TOPIC T=CLASS C=colour:Colour; extension:Extended; tree:Tree;
                 MANDATORY CONSTRAINT colour <= extension;
                 MANDATORY CONSTRAINT tree < (#group);
                 MANDATORY CONSTRAINT accepts(#group.blue);
             END C; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let constraints = graph.declaration("M.T.C").unwrap().constraints();
    let interlis::BoundExpressionKind::Binary { right, .. } = graph
        .expression(constraints[1].expression.unwrap())
        .unwrap()
        .kind
    else {
        panic!("expected enumeration comparison");
    };
    assert!(matches!(graph.expression_declared_type(right),
        Some(TypeInfo::EnumTree { domain, .. }) if domain.as_deref() == Some("M.Extended")));
    let interlis::BoundExpressionKind::Group { expression } = graph.expression(right).unwrap().kind
    else {
        panic!("expected group");
    };
    assert!(matches!(&graph.expression(expression).unwrap().kind,
        interlis::BoundExpressionKind::Enumeration { path } if path == &["group"]));
}

#[test]
fn enumeration_comparisons_distinguish_bad_values_and_unrelated_or_unordered_types() {
    for (condition, code) in [
        ("leaf == #group", "ILI-ENUM-CONSTANT"),
        ("leaf == #group.absent", "ILI-ENUM-CONSTANT"),
        ("leaf == separate", "ILI-EXPRESSION-OPERANDS"),
        ("unordered <= #a", "ILI-EXPRESSION-OPERANDS"),
        ("circular > #a", "ILI-EXPRESSION-OPERANDS"),
        ("flag == #yes", "ILI-ENUM-CONSTANT"),
    ] {
        outcome(
            single(&format!(
                "DOMAIN Values=(a, group(b)) ORDERED;
                 TOPIC T=CLASS C=leaf:Values; separate:(a,group(b)) ORDERED;
                     unordered:(a,b); circular:(a,b) CIRCULAR; flag:BOOLEAN;
                     MANDATORY CONSTRAINT {condition};
                 END C; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        single(
            "TOPIC T=CLASS Lookup=v:(a, group(b)); END Lookup;
             CLASS Source=v:(a, group(b)); EXISTENCE CONSTRAINT v REQUIRED IN Lookup:v;
             END Source; END T;",
        ),
        Status::CheckedSubset,
        None,
    );
}

#[test]
fn predefined_enum_leaves_fit_their_tree_formals_and_existence_targets() {
    for version in ["2.3", "2.4"] {
        outcome(
            request(vec![(
                "memory:///model.ili",
                format!(
                    "INTERLIS {version}; CONTRACTED MODEL M AT \"x\" VERSION \"1\" =
                     DOMAIN Flags=ALL OF INTERLIS.BOOLEAN; Alignments=ALL OF INTERLIS.HALIGNMENT;
                     FUNCTION accepts(flag:Flags; alignment:Alignments):BOOLEAN;
                     TOPIC T=CLASS Lookup=flag:Flags; alignment:Alignments; END Lookup;
                        CLASS Source=flag:BOOLEAN; alignment:HALIGNMENT;
                            MANDATORY CONSTRAINT accepts(flag, alignment);
                            EXISTENCE CONSTRAINT flag REQUIRED IN Lookup:flag;
                            EXISTENCE CONSTRAINT alignment REQUIRED IN Lookup:alignment;
                        END Source;
                     END T; END M."
                ),
            )]),
            Status::CheckedSubset,
            None,
        );
    }
}

#[test]
fn enum_functions_bind_polymorphic_arguments_and_check_relation_of_known_types() {
    let definitions = "DOMAIN Value=(group(a,b), c) ORDERED; Nodes=ALL OF Value;
        FUNCTION accepts(leaf:ENUMVAL; node:ENUMTREEVAL):BOOLEAN;
        TOPIC T=CLASS Item=leaf:Value; tree:Nodes; unrelated:(group(a,b),c) ORDERED;";
    let report = outcome(
        single(&format!(
            "{definitions}
                MANDATORY CONSTRAINT accepts(leaf,tree);
                MANDATORY CONSTRAINT INTERLIS.isEnumSubVal(leaf,(#group));
                MANDATORY CONSTRAINT INTERLIS.inEnumRange(#group.a,tree,#c);
                MANDATORY CONSTRAINT INTERLIS.isEnumSubVal(#group.a,#group);
            END Item; END T;"
        )),
        Status::CheckedSubset,
        None,
    );
    let graph = report.graph().unwrap();
    let args = graph
        .function_arguments("M.accepts")
        .unwrap()
        .collect::<Vec<_>>();
    assert!(matches!(
        graph.effective_type(args[0].qualified_name()),
        Some(TypeInfo::EnumArgument { tree_values: false })
    ));
    assert!(matches!(
        graph.effective_type(args[1].qualified_name()),
        Some(TypeInfo::EnumArgument { tree_values: true })
    ));
    let constraints = graph.declaration("M.T.Item").unwrap().constraints();
    let interlis::BoundExpressionKind::Call {
        function,
        arguments,
    } = &graph
        .expression(constraints[1].expression.unwrap())
        .unwrap()
        .kind
    else {
        panic!("expected a bound enum function");
    };
    assert_eq!(function.target_name(), "INTERLIS.isEnumSubVal");
    let ty = graph.expression_declared_type(arguments[1]).unwrap();
    assert!(matches!(ty, TypeInfo::EnumTree { domain: None, .. }));
    assert!(std::ptr::eq(
        graph.enum_tree_leaf_type(ty).unwrap(),
        graph.effective_type("M.Value").unwrap()
    ));

    for (condition, code) in [
        ("accepts(tree,leaf)", "ILI-FUNCTION-ARGUMENT"),
        (
            "INTERLIS.isEnumSubVal(leaf,unrelated)",
            "ILI-ENUM-ARGUMENT-TYPES",
        ),
        ("INTERLIS.isEnumSubVal(leaf,#missing)", "ILI-ENUM-CONSTANT"),
        ("INTERLIS.inEnumRange(#group,tree,#c)", "ILI-ENUM-CONSTANT"),
        (
            "INTERLIS.inEnumRange(tree,#group,#c)",
            "ILI-FUNCTION-ARGUMENT",
        ),
    ] {
        outcome(
            single(&format!(
                "{definitions} MANDATORY CONSTRAINT {condition}; END Item; END T;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
}

#[test]
fn attribute_derivations_keep_bound_values_transience_and_constant_override_finality() {
    for version in ["2.3", "2.4"] {
        let report = outcome(
            request(vec![(
                "model.ili",
                format!(
                    "INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" =
                     TOPIC T=CLASS Base=active:BOOLEAN; rank:0..10; END Base;
                     CLASS Child EXTENDS Base=
                         flag (TRANSIENT):BOOLEAN := active;
                         rank (EXTENDED):0..10 := 7;
                     END Child;
                     CLASS Grandchild EXTENDS Child=END Grandchild;
                     END T; END M.",
                ),
            )]),
            Status::CheckedSubset,
            None,
        );
        let graph = report.graph().unwrap();
        let flag = graph.declaration("M.T.Child.flag").unwrap();
        assert!(flag.is_transient());
        let interlis::BoundExpressionKind::Path { steps } =
            &graph.expression(flag.derivations()[0]).unwrap().kind
        else {
            panic!("expected a derivation from an inherited attribute");
        };
        assert_eq!(steps[0].reference.target_name(), "M.T.Base.active");
        let rank = graph.declaration("M.T.Child.rank").unwrap();
        assert!(rank.is_final());
        assert!(!rank.is_transient());
        assert_eq!(
            rank.base_reference().unwrap().target_name(),
            "M.T.Base.rank"
        );
    }
}

#[test]
fn attribute_derivations_reject_wrong_values_and_invalid_transient_overrides() {
    for (body, code) in [
        (
            "CLASS Item=x (TRANSIENT):BOOLEAN; END Item;",
            "ILI-TRANSIENT-DERIVATION",
        ),
        (
            "CLASS Item=x:BOOLEAN := 7; END Item;",
            "ILI-ATTRIBUTE-DERIVATION-TYPE",
        ),
        (
            "CLASS Item=x (TRANSIENT):BOOLEAN := #True; END Item;",
            "ILI-ENUM-CONSTANT",
        ),
        (
            "CLASS Item=x:0..3 := 4; END Item;",
            "ILI-ATTRIBUTE-DERIVATION-TYPE",
        ),
        (
            "CLASS Item=x:MANDATORY TEXT := UNDEFINED; END Item;",
            "ILI-ATTRIBUTE-DERIVATION-TYPE",
        ),
        (
            "CLASS Base=x (TRANSIENT):BOOLEAN := #true; END Base;
             CLASS Child EXTENDS Base=x (EXTENDED):BOOLEAN; END Child;",
            "ILI-ATTRIBUTE-TRANSIENT-OVERRIDE",
        ),
        (
            "CLASS Base=x:0..10; END Base;
             CLASS Child EXTENDS Base=x (EXTENDED):0..10 := 7; END Child;
             CLASS Leaf EXTENDS Child=x (EXTENDED):0..10; END Leaf;",
            "ILI-ATTRIBUTE-OVERRIDE",
        ),
        (
            "CLASS Base=x:TEXT*8; END Base;
             CLASS Child EXTENDS Base=x (EXTENDED):TEXT*8 := UNDEFINED; END Child;
             CLASS Leaf EXTENDS Child=x (EXTENDED):TEXT*8 := \"new\"; END Leaf;",
            "ILI-ATTRIBUTE-OVERRIDE",
        ),
    ] {
        outcome(
            single(&format!("TOPIC T={body} END T;")),
            Status::Rejected,
            Some(code),
        );
    }
}

#[test]
fn existence_constraints_reject_missing_dependencies_and_incompatible_values() {
    for (dependencies, ty, path, code) in [
        (
            "",
            "0..10 [INTERLIS.m]",
            "value",
            "ILI-EXISTENCE-DEPENDS-ON",
        ),
        (
            "DEPENDS ON Values;",
            "0..10 [INTERLIS.s]",
            "value",
            "ILI-EXISTENCE-TYPE",
        ),
        (
            "DEPENDS ON Values;",
            "TEXT*4",
            "value",
            "ILI-EXISTENCE-TYPE",
        ),
        (
            "DEPENDS ON Values;",
            "0..10 [INTERLIS.m]",
            "THIS",
            "ILI-EXISTENCE-PATH",
        ),
    ] {
        outcome(
            single(&format!(
                "TOPIC Values=CLASS Target=value:0..20 [INTERLIS.m]; END Target; END Values;
                 TOPIC Links={dependencies}
                    CLASS Obj=value:{ty};
                       EXISTENCE CONSTRAINT value REQUIRED IN M.Values.Target:{path};
                    END Obj;
                 END Links;"
            )),
            Status::Rejected,
            Some(code),
        );
    }
    outcome(
        single(
            "TOPIC T=STRUCTURE Part=x:TEXT*5; END Part;
            CLASS Values=v:BAG OF Part; END Values;
            CLASS Links=v:LIST OF Part; EXISTENCE CONSTRAINT v REQUIRED IN Values:v; END Links;
            END T;",
        ),
        Status::Rejected,
        Some("ILI-EXISTENCE-TYPE"),
    );
}

#[test]
fn generic_usage_includes_unrestricted_structures_and_indirect_line_vertices() {
    let domains =
        "DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; A=COORD 0..10,0..10; B=COORD 20..30,20..30;
        CONTEXT default=G=A OR B;";
    for model in [
        "STRUCTURE Nested=position:G; END Nested; TOPIC T={deferred} CLASS Obj=geometry:ANYSTRUCTURE; END Obj; END T;",
        "DOMAIN Vertex=COORD 0..10 <G[1]>,0..10; Line=POLYLINE WITH (STRAIGHTS) VERTEX Vertex;
         TOPIC T={deferred} CLASS Obj=geometry:Line; END Obj; END T;",
    ] {
        for (deferred, status, code) in [
            ("", Status::Rejected, Some("ILI-GENERIC-DEFERRED-REQUIRED")),
            ("DEFERRED GENERICS G;", Status::CheckedSubset, None),
        ] {
            outcome(
                single(&format!(
                    "{domains}{}",
                    model.replace("{deferred}", deferred)
                )),
                status,
                code,
            );
        }
    }
}

#[test]
fn contexts_recheck_units_and_rotation_in_each_concrete_alternative() {
    for (body, code) in [
        ("DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; P=COORD 0..1 [INTERLIS.m],0..1 [INTERLIS.m];
          N=0..1 [INTERLIS.s] <G[1]>; CONTEXT default=G=P; TOPIC T=CLASS Obj=x:N; END Obj; END T;", "ILI-REFSYSTEM-CONTEXT"),
        ("DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; P=COORD 0..1,0..1;
          N=0..1 [INTERLIS.rad] <G>; CONTEXT default=G=P; TOPIC T=CLASS Obj=x:N; END Obj; END T;", "ILI-REFSYSTEM-CONTEXT"),
        ("DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; P=COORD 0..1 [INTERLIS.s],0..1 [INTERLIS.s];
          Line=POLYLINE WITH (STRAIGHTS) VERTEX G; CONTEXT default=G=P; TOPIC T=CLASS Obj=x:Line; END Obj; END T;", "ILI-GEOMETRY-VERTEX-UNIT"),
        ("DOMAIN G (GENERIC)=COORD NUMERIC; P=COORD 0..1; CONTEXT P=G=P;", "ILI-CONTEXT-NAME"),
    ] {
        outcome(single(body), Status::Rejected, Some(code));
    }
}

#[test]
fn coordinate_selections_check_cycles_with_one_alternative_per_generic() {
    outcome(
        single(
            "DOMAIN Generic (GENERIC)=COORD NUMERIC,NUMERIC;
             Measure=0..10 <Generic[1]>; Selected=COORD 0..10 <Measure>,0..10;
             CONTEXT C=Generic=Selected;
             TOPIC T=CLASS C=value:Measure; END C; END T;",
        ),
        Status::Rejected,
        Some("ILI-REFSYSTEM-CYCLE"),
    );
    outcome(
        single(
            "DOMAIN Generic (GENERIC)=COORD NUMERIC,NUMERIC;
             First=COORD 0..10 <Generic[2]>,0..10;
             Second=COORD 0..10,0..10 <Generic[1]>;
             CONTEXT C=Generic=First OR Second;
             TOPIC T=DEFERRED GENERICS Generic;
                CLASS C=value:Generic; END C;
             END T;",
        ),
        Status::CheckedSubset,
        None,
    );
    outcome(
        single(
            "DOMAIN G1 (GENERIC)=COORD NUMERIC; G2 (GENERIC)=COORD NUMERIC;
             First=COORD 0..10 <G2[1]>; Second=COORD 0..10 <G1[1]>;
             CONTEXT C1=G1=First; C2=G2=Second;
             TOPIC T=CLASS C=value:G1; END C; END T;",
        ),
        Status::Rejected,
        Some("ILI-REFSYSTEM-CYCLE"),
    );
}

#[test]
fn generic_uses_include_references_introduced_by_the_selected_coordinates() {
    let domains = "DOMAIN First (GENERIC)=COORD NUMERIC; Second (GENERIC)=COORD NUMERIC;
        A=COORD 0..9 <Second[1]>; B=COORD 0..9; C=COORD 10..19;
        CONTEXT primary=First=A;";
    for (selection, deferred, status, code) in [
        (
            "",
            "",
            Status::Rejected,
            Some("ILI-GENERIC-CONTEXT-MISSING"),
        ),
        (
            "CONTEXT alternatives=Second=B OR C;",
            "",
            Status::Rejected,
            Some("ILI-GENERIC-DEFERRED-REQUIRED"),
        ),
        (
            "CONTEXT alternatives=Second=B OR C;",
            "DEFERRED GENERICS Second;",
            Status::CheckedSubset,
            None,
        ),
    ] {
        let report = outcome(
            single(&format!(
                "{domains} {selection} TOPIC T={deferred} CLASS Item=value:First; END Item; END T;"
            )),
            status,
            code,
        );
        if let Some(graph) = report.graph() {
            let bindings = graph.declaration("M.T").unwrap().generic_bindings();
            assert_eq!(bindings.len(), 2);
            assert_eq!(bindings[1].generic_domain, "M.Second");
            assert_eq!(bindings[1].concrete_domains, ["M.B", "M.C"]);
            assert!(bindings[1].deferred);
        }
    }
    outcome(
        single(
            "DOMAIN G (GENERIC)=COORD NUMERIC; Direct=0..9 <G[1]>; Indirect=0..9 <Direct>;
                TOPIC T=CLASS Item=value:Indirect; END Item; END T;",
        ),
        Status::Rejected,
        Some("ILI-GENERIC-CONTEXT-MISSING"),
    );
}

#[test]
fn generic_dependencies_never_switch_an_alternative_between_axes() {
    for alternatives in ["Left OR Right", "Right OR Left"] {
        let report = outcome(
            single(&format!(
                "DOMAIN G (GENERIC)=COORD NUMERIC,NUMERIC; Ghost (GENERIC)=COORD NUMERIC;
                 Left=COORD 0..9 <G[2]>,0..9; Right=COORD 0..9,0..9 <Ghost[1]>;
                 Measure=0..9 <G[1]>;
                 CONTEXT Primary=G={alternatives};
                 TOPIC T=DEFERRED GENERICS G; CLASS Item=value:Measure; END Item; END T;"
            )),
            Status::CheckedSubset,
            None,
        );
        let bindings = report
            .graph()
            .unwrap()
            .declaration("M.T")
            .unwrap()
            .generic_bindings();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].generic_domain, "M.G");
    }
}
