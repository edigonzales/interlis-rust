//! View, metadata basket and graphic productions with unresolved model references.
use super::*;
use crate::MetadataBasketKind;

const VIEW_PROPERTIES: &[Property] = &[
    Property::Abstract,
    Property::Extended,
    Property::Final,
    Property::Transient,
];
const RULE_PROPERTIES: &[Property] = &[Property::Abstract, Property::Extended, Property::Final];

#[derive(Debug)]
#[allow(dead_code)] // These fields belong to later model binding.
pub(crate) struct ViewDecl {
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub origin: Option<ViewOrigin>,
    pub base_extensions: Vec<BaseExtension>,
    pub selections: Vec<Selection>,
    pub attribute_section: Option<Span>,
    pub attributes: Vec<ViewAttribute>,
    pub constraints: Vec<ConstraintSyntax>,
    pub end_name: Name,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ViewOrigin {
    Extends(QualifiedName),
    Formation(Located<ViewFormation>),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ViewFormation {
    Projection(RenamedViewable),
    Join {
        first: RenamedViewable,
        others: Vec<JoinBase>,
    },
    Union(Vec<RenamedViewable>),
    Aggregation {
        base: RenamedViewable,
        grouping: AggregationGrouping,
    },
    Inspection(InlineInspection),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct JoinBase {
    pub base: RenamedViewable,
    pub or_null: Option<Span>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum AggregationGrouping {
    All(Span),
    Equal(Located<Vec<ObjectPath>>),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct BaseExtension {
    pub base: Name,
    pub extensions: Vec<RenamedViewable>,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Selection {
    pub expression: ExprId,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ViewAttribute {
    Typed(Box<Declaration>),
    AllOf(Located<Name>),
    Derived {
        name: Name,
        properties: Vec<Located<Property>>,
        value: ExprId,
        span: Span,
    },
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct MetadataBasketDecl {
    pub kind: Located<MetadataBasketKind>,
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub base: Option<QualifiedName>,
    pub topic: QualifiedName,
    pub groups: Vec<MetaObjectGroup>,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct MetaObjectGroup {
    pub class: Name,
    pub objects: Vec<Name>,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct GraphicDecl {
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub base: Option<QualifiedName>,
    pub viewable: Option<QualifiedName>,
    pub selections: Vec<Selection>,
    pub rules: Vec<DrawingRule>,
    pub end_name: Name,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct DrawingRule {
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub class: Option<QualifiedName>,
    pub clauses: Vec<DrawingClause>,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct DrawingClause {
    pub condition: Option<ExprId>,
    pub assignments: Vec<SignParameterAssignment>,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct SignParameterAssignment {
    pub parameter: Name,
    pub value: SignParameterValue,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum SignParameterValue {
    MetaObject(Located<QualifiedName>),
    Factor(ExprId),
    According {
        path: ObjectPath,
        cases: Vec<EnumAssignment>,
        span: Span,
    },
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct EnumAssignment {
    pub value: EnumAssignmentValue,
    pub range: EnumRange,
    pub span: Span,
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum EnumAssignmentValue {
    MetaObject(Located<QualifiedName>),
    Constant(ExprId),
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct EnumRange {
    pub first: ExprId,
    pub last: Option<ExprId>,
    pub span: Span,
}

impl Parser<'_> {
    pub(super) fn view(&mut self) -> Parsed<ViewDecl> {
        self.step()?;
        let start = self.start();
        self.expect("VIEW")?;
        let name = self.name(false)?;
        let properties = self.properties(VIEW_PROPERTIES)?;
        let origin = if self.take("EXTENDS") {
            self.node()?;
            Some(ViewOrigin::Extends(self.reference(3)?))
        } else if matches!(
            self.text(),
            "PROJECTION" | "JOIN" | "UNION" | "AGGREGATION" | "AREA" | "INSPECTION"
        ) {
            Some(ViewOrigin::Formation(self.view_formation()?))
        } else {
            None
        };
        let mut base_extensions = vec![];
        while self.is("BASE") {
            base_extensions.push(self.base_extension()?);
        }
        let selections = self.selections()?;
        self.expect("=")?;
        let attribute_section = self.marker("ATTRIBUTE");
        self.enter()?;
        let body = (|| {
            let mut attributes = vec![];
            while self.at < self.tokens.len()
                && !self.is("END")
                && !self.constraint_start()
                && !self.diagnostics.limited
            {
                self.step()?;
                let before = self.at;
                match self.view_attribute() {
                    Ok(attribute) => attributes.push(attribute),
                    Err(issue) => {
                        self.report(issue);
                        self.recover(Context::View, false);
                    }
                }
                if self.at == before {
                    self.step()?;
                    self.at += 1;
                }
            }
            Ok((attributes, self.constraints()?))
        })();
        self.depth -= 1;
        let (attributes, constraints) = body?;
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(";")?;
        self.node()?;
        Ok(ViewDecl {
            name,
            properties,
            origin,
            base_extensions,
            selections,
            attribute_section,
            attributes,
            constraints,
            end_name,
            span: self.span(start, self.end()),
        })
    }

    fn view_formation(&mut self) -> Parsed<Located<ViewFormation>> {
        self.step()?;
        let start = self.start();
        let value = if self.take("PROJECTION") {
            self.expect("OF")?;
            ViewFormation::Projection(self.renamed_viewable()?)
        } else if self.take("JOIN") {
            self.expect("OF")?;
            let first = self.renamed_viewable()?;
            self.expect(",")?;
            let mut others = vec![];
            loop {
                let base = self.renamed_viewable()?;
                let start = self.start();
                let or_null = if self.take("(") {
                    self.expect("OR")?;
                    self.expect("NULL")?;
                    self.expect(")")?;
                    Some(self.span(start, self.end()))
                } else {
                    None
                };
                self.node()?;
                others.push(JoinBase { base, or_null });
                if !self.take(",") {
                    break;
                }
            }
            ViewFormation::Join { first, others }
        } else if self.take("UNION") {
            self.expect("OF")?;
            let first = self.renamed_viewable()?;
            self.expect(",")?;
            let mut bases = vec![first, self.renamed_viewable()?];
            while self.take(",") {
                bases.push(self.renamed_viewable()?);
            }
            ViewFormation::Union(bases)
        } else if self.take("AGGREGATION") {
            self.expect("OF")?;
            let base = self.renamed_viewable()?;
            let selection = self.start();
            let grouping = if let Some(all) = self.marker("ALL") {
                AggregationGrouping::All(all)
            } else {
                self.expect("EQUAL")?;
                self.expect("(")?;
                self.enter()?;
                let keys = (|| {
                    let mut paths = vec![self.object_path()?];
                    while self.take(",") {
                        paths.push(self.object_path()?);
                    }
                    self.expect(")")?;
                    Ok(paths)
                })();
                self.depth -= 1;
                AggregationGrouping::Equal(Located {
                    value: keys?,
                    span: self.span(selection, self.end()),
                })
            };
            self.node()?;
            ViewFormation::Aggregation { base, grouping }
        } else {
            let area = self.marker("AREA");
            self.expect("INSPECTION")?;
            self.expect("OF")?;
            ViewFormation::Inspection(self.inspection_path(start, area)?)
        };
        self.expect(";")?;
        self.node()?;
        Ok(Located {
            value,
            span: self.span(start, self.end()),
        })
    }

    fn base_extension(&mut self) -> Parsed<BaseExtension> {
        self.step()?;
        let start = self.start();
        self.expect("BASE")?;
        let base = self.name(false)?;
        self.expect("EXTENDED")?;
        self.expect("BY")?;
        let mut extensions = vec![self.renamed_viewable()?];
        while self.take(",") {
            extensions.push(self.renamed_viewable()?);
        }
        self.node()?;
        Ok(BaseExtension {
            base,
            extensions,
            span: self.span(start, self.end()),
        })
    }

    fn selections(&mut self) -> Parsed<Vec<Selection>> {
        let mut selections = vec![];
        while self.is("WHERE") {
            let start = self.start();
            self.expect("WHERE")?;
            let expression = self.expression()?;
            self.expect(";")?;
            self.node()?;
            selections.push(Selection {
                expression,
                span: self.span(start, self.end()),
            });
        }
        Ok(selections)
    }

    fn view_attribute(&mut self) -> Parsed<ViewAttribute> {
        let start = self.start();
        if self.take("ALL") {
            self.expect("OF")?;
            let value = self.name(false)?;
            self.expect(";")?;
            self.node()?;
            return Ok(ViewAttribute::AllOf(Located {
                value,
                span: self.span(start, self.end()),
            }));
        }
        let head = self.attribute_head()?;
        if head.subdivision.is_none() && self.take(":=") {
            let value = self.derived_attribute_value()?;
            self.expect(";")?;
            self.node()?;
            Ok(ViewAttribute::Derived {
                name: head.name,
                properties: head.properties,
                value,
                span: self.span(start, self.end()),
            })
        } else {
            Ok(ViewAttribute::Typed(Box::new(self.typed_attribute(head)?)))
        }
    }

    pub(super) fn metadata_basket(&mut self) -> Parsed<MetadataBasketDecl> {
        self.step()?;
        let start = self.start();
        let value = if self.take("SIGN") {
            MetadataBasketKind::Sign
        } else {
            self.expect("REFSYSTEM")?;
            MetadataBasketKind::ReferenceSystem
        };
        self.node()?;
        let kind = Located {
            value,
            span: self.span(start, self.end()),
        };
        self.expect("BASKET")?;
        let name = self.name(false)?;
        let properties = self.properties(&[Property::Final])?;
        let base = if self.take("EXTENDS") {
            Some(self.reference(3)?)
        } else {
            None
        };
        self.expect("~")?;
        let topic = self.reference(2)?;
        let mut groups = vec![];
        while self.is("OBJECTS") {
            let start = self.start();
            self.expect("OBJECTS")?;
            self.expect("OF")?;
            let class = self.name(false)?;
            self.expect(":")?;
            let mut objects = vec![self.name(false)?];
            while self.take(",") {
                objects.push(self.name(false)?);
            }
            self.node()?;
            groups.push(MetaObjectGroup {
                class,
                objects,
                span: self.span(start, self.end()),
            });
        }
        self.expect(";")?;
        self.node()?;
        Ok(MetadataBasketDecl {
            kind,
            name,
            properties,
            base,
            topic,
            groups,
            span: self.span(start, self.end()),
        })
    }

    pub(super) fn graphic(&mut self) -> Parsed<GraphicDecl> {
        self.step()?;
        let start = self.start();
        self.expect("GRAPHIC")?;
        let name = self.name(false)?;
        let properties = self.properties(&[Property::Abstract, Property::Final])?;
        let base = if self.take("EXTENDS") {
            Some(self.reference(3)?)
        } else {
            None
        };
        let viewable = if self.take("BASED") {
            self.expect("ON")?;
            Some(self.class_reference()?)
        } else {
            None
        };
        self.expect("=")?;
        self.enter()?;
        let body = (|| {
            let selections = self.selections()?;
            let mut rules = vec![];
            while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
                self.step()?;
                let before = self.at;
                match self.drawing_rule() {
                    Ok(rule) => rules.push(rule),
                    Err(issue) => {
                        self.report(issue);
                        self.recover(Context::Graphic, false);
                    }
                }
                if self.at == before {
                    self.step()?;
                    self.at += 1;
                }
            }
            Ok((selections, rules))
        })();
        self.depth -= 1;
        let (selections, rules) = body?;
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(";")?;
        self.node()?;
        Ok(GraphicDecl {
            name,
            properties,
            base,
            viewable,
            selections,
            rules,
            end_name,
            span: self.span(start, self.end()),
        })
    }

    fn drawing_rule(&mut self) -> Parsed<DrawingRule> {
        self.step()?;
        let start = self.start();
        let name = self.name(false)?;
        let properties = self.properties(RULE_PROPERTIES)?;
        let class = if self.take("OF") {
            Some(self.class_reference()?)
        } else {
            None
        };
        self.expect(":")?;
        let mut clauses = vec![self.drawing_clause()?];
        while self.take(",") {
            clauses.push(self.drawing_clause()?);
        }
        self.expect(";")?;
        self.node()?;
        Ok(DrawingRule {
            name,
            properties,
            class,
            clauses,
            span: self.span(start, self.end()),
        })
    }

    fn drawing_clause(&mut self) -> Parsed<DrawingClause> {
        self.step()?;
        let start = self.start();
        let condition = if self.take("WHERE") {
            Some(self.expression()?)
        } else {
            None
        };
        self.expect("(")?;
        self.enter()?;
        let assignments = (|| {
            let mut items = vec![self.sign_parameter_assignment()?];
            while self.take(";") {
                items.push(self.sign_parameter_assignment()?);
            }
            self.expect(")")?;
            Ok(items)
        })();
        self.depth -= 1;
        let assignments = assignments?;
        self.node()?;
        Ok(DrawingClause {
            condition,
            assignments,
            span: self.span(start, self.end()),
        })
    }

    fn sign_parameter_assignment(&mut self) -> Parsed<SignParameterAssignment> {
        self.step()?;
        let start = self.start();
        let parameter = self.name(false)?;
        self.expect(":=")?;
        let value = if self.is("{") {
            SignParameterValue::MetaObject(self.braced_meta_object()?)
        } else {
            let start = self.start();
            if self.take("ACCORDING") {
                let path = self.object_path()?;
                self.expect("(")?;
                self.enter()?;
                let cases = (|| {
                    let mut cases = vec![self.enum_assignment()?];
                    while self.take(",") {
                        cases.push(self.enum_assignment()?);
                    }
                    self.expect(")")?;
                    Ok(cases)
                })();
                self.depth -= 1;
                let cases = cases?;
                self.node()?;
                SignParameterValue::According {
                    path,
                    cases,
                    span: self.span(start, self.end()),
                }
            } else {
                SignParameterValue::Factor(self.factor()?)
            }
        };
        self.node()?;
        Ok(SignParameterAssignment {
            parameter,
            value,
            span: self.span(start, self.end()),
        })
    }

    fn braced_meta_object(&mut self) -> Parsed<Located<QualifiedName>> {
        let start = self.start();
        self.expect("{")?;
        let value = self.reference(4)?;
        self.expect("}")?;
        self.node()?;
        Ok(Located {
            value,
            span: self.span(start, self.end()),
        })
    }

    fn enum_assignment(&mut self) -> Parsed<EnumAssignment> {
        self.step()?;
        let start = self.start();
        let value = if self.is("{") {
            EnumAssignmentValue::MetaObject(self.braced_meta_object()?)
        } else {
            EnumAssignmentValue::Constant(self.constant()?)
        };
        self.expect("WHEN")?;
        self.expect("IN")?;
        let range_start = self.start();
        let first = self.enum_endpoint()?;
        let last = if self.take("..") {
            Some(self.enum_endpoint()?)
        } else {
            None
        };
        self.node()?;
        let range = EnumRange {
            first,
            last,
            span: self.span(range_start, self.end()),
        };
        self.node()?;
        Ok(EnumAssignment {
            value,
            range,
            span: self.span(start, self.end()),
        })
    }

    fn enum_endpoint(&mut self) -> Parsed<ExprId> {
        if !self.is("#") {
            return Err(Self::expected("Expected an enumeration constant"));
        }
        self.constant()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};

    const FIXTURE: &str = include_str!("../../tests/fixtures/syntax/views-graphics24.ili");

    fn syntax(text: &str, limits: Limits) -> (SourceSet, Document, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "drawings".into(),
            text: text.into(),
        }])
        .unwrap();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut budget = SyntaxBudget::new(&limits);
        let document = parse(&sources, 0, &limits, &mut budget, &mut diagnostics)
            .unwrap_or_else(|| panic!("{:?}", diagnostics.entries));
        (sources, document, diagnostics)
    }

    fn analyze_text(text: &str, limits: Limits) -> crate::AnalysisReport {
        analyze(CompileRequest {
            sources: vec![SourceInput {
                name: "drawings".into(),
                text: text.into(),
            }],
            limits,
        })
        .unwrap()
    }

    fn original(sources: &SourceSet, span: &Span) -> String {
        let l = sources.location(span).unwrap();
        sources.files()[0].text()[l.start_byte as usize..l.end_byte as usize].to_owned()
    }

    #[test]
    fn view_formations_keep_bases_aliases_attributes_and_grouping_in_order() {
        let (sources, doc, diagnostics) = syntax(FIXTURE, Limits::default());
        assert!(diagnostics.entries.is_empty(), "{:?}", diagnostics.entries);
        let Declaration::Topic { declarations, .. } = &doc.models[0].declarations[1] else {
            panic!()
        };
        let views: Vec<_> = declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::View(view) => Some(view),
                _ => None,
            })
            .collect();
        assert_eq!(
            views
                .iter()
                .map(|v| v.name.value.as_str())
                .collect::<Vec<_>>(),
            [
                "Projected",
                "Joined",
                "JoinedExtra",
                "Extending",
                "JoinedUnion",
                "Groups",
                "AllGroups",
                "Inspect",
                "Areas",
            ]
        );

        let p = views[0];
        let Some(ViewOrigin::Formation(Located {
            value: ViewFormation::Projection(base),
            span,
        })) = &p.origin
        else {
            panic!()
        };
        assert_eq!(base.alias.as_ref().unwrap().value, "p");
        assert_eq!(
            original(&sources, span),
            "PROJECTION OF p ~ Data.Features.Feature;"
        );
        assert_eq!(p.base_extensions[0].base.value, "p");
        assert_eq!(
            original(&sources, &p.base_extensions[0].span),
            "BASE p EXTENDED BY s ~ Special, Other"
        );
        assert_eq!(
            p.base_extensions[0].extensions[0]
                .alias
                .as_ref()
                .unwrap()
                .value,
            "s"
        );
        assert_eq!(
            p.base_extensions[0].extensions[1].target.segments[0].value,
            "Other"
        );
        assert_eq!(
            original(&sources, &p.selections[1].span),
            "WHERE s != UNDEFINED;"
        );
        assert_eq!(
            original(&sources, p.attribute_section.as_ref().unwrap()),
            "ATTRIBUTE"
        );
        let [
            ViewAttribute::AllOf(all),
            ViewAttribute::Typed(typed),
            ViewAttribute::Derived { name, value, .. },
        ] = p.attributes.as_slice()
        else {
            panic!()
        };
        assert_eq!(all.value.value, "p");
        let Declaration::Attribute { derivations, .. } = typed.as_ref() else {
            panic!()
        };
        assert_eq!(derivations.len(), 2);
        assert_eq!(name.value, "derived");
        assert!(matches!(
            doc.expressions[value.0 as usize].kind,
            ExpressionKind::Binary { .. }
        ));
        assert_eq!(p.constraints[0].name.as_ref().unwrap().value, "keep");
        assert_eq!(p.end_name.value, "Projected");

        let Some(ViewOrigin::Formation(Located {
            value: ViewFormation::Join { first, others },
            ..
        })) = &views[1].origin
        else {
            panic!()
        };
        assert_eq!(first.alias.as_ref().unwrap().value, "a");
        assert_eq!(others[0].base.alias.as_ref().unwrap().value, "b");
        assert_eq!(
            original(&sources, others[0].or_null.as_ref().unwrap()),
            "(OR NULL)"
        );
        assert!(others[1].or_null.is_none());
        assert!(
            matches!(&views[2].origin, Some(ViewOrigin::Extends(base)) if base.segments[0].value == "Joined")
        );
        assert!(views[3].origin.is_none());
        assert!(
            matches!(&views[4].origin, Some(ViewOrigin::Formation(Located { value: ViewFormation::Union(bases), .. })) if bases.len() == 2 && bases[1].alias.as_ref().unwrap().value == "p")
        );
        assert!(
            matches!(&views[5].origin, Some(ViewOrigin::Formation(Located { value: ViewFormation::Aggregation { grouping: AggregationGrouping::Equal(keys), .. }, .. })) if keys.value.len() == 2 && keys.value[0].elements.len() == 2)
        );
        assert!(matches!(
            &views[6].origin,
            Some(ViewOrigin::Formation(Located {
                value: ViewFormation::Aggregation {
                    grouping: AggregationGrouping::All(_),
                    ..
                },
                ..
            }))
        ));
        assert!(
            matches!(&views[7].origin, Some(ViewOrigin::Formation(Located { value: ViewFormation::Inspection(inspect), .. })) if inspect.area.is_none() && inspect.attributes[1].value == "segments")
        );
        assert!(
            matches!(&views[8].origin, Some(ViewOrigin::Formation(Located { value: ViewFormation::Inspection(inspect), .. })) if inspect.area.is_some())
        );

        let report = analyze_text(FIXTURE, Limits::default());
        assert_eq!(report.status(), AnalysisStatus::Rejected);
        assert!(
            report
                .diagnostics()
                .iter()
                .any(|d| d.code == "ILI-NAME-NOT-FOUND")
        );
        assert!(report.graph().is_none());
    }

    #[test]
    fn metadata_and_graphics_keep_group_order_source_ranges_and_assignment_kinds() {
        let (sources, doc, diagnostics) = syntax(FIXTURE, Limits::default());
        assert!(diagnostics.entries.is_empty(), "{:?}", diagnostics.entries);
        let Declaration::MetadataBasket(palette) = &doc.models[0].declarations[0] else {
            panic!()
        };
        assert_eq!(palette.kind.value, MetadataBasketKind::Sign);
        assert_eq!(palette.name.value, "Palette");
        assert_eq!(palette.properties[0].value, Property::Final);
        assert_eq!(palette.base.as_ref().unwrap().segments[1].value, "Base");
        assert_eq!(palette.topic.segments[1].value, "Signs");
        assert_eq!(palette.groups[0].class.value, "LineSign");
        assert_eq!(palette.groups[0].objects[1].value, "Thick");
        assert_eq!(
            original(&sources, &palette.groups[1].span),
            "OBJECTS OF PointSign: Dot"
        );

        let Declaration::Topic { declarations, .. } = &doc.models[0].declarations[1] else {
            panic!()
        };
        let Declaration::MetadataBasket(frames) = &declarations[0] else {
            panic!()
        };
        assert_eq!(frames.kind.value, MetadataBasketKind::ReferenceSystem);
        assert!(frames.groups.is_empty());

        let Declaration::Graphic(g) = declarations.last().unwrap() else {
            panic!()
        };
        assert_eq!(g.base.as_ref().unwrap().segments[1].value, "Common");
        assert_eq!(g.viewable.as_ref().unwrap().segments[2].value, "Projected");
        assert_eq!(original(&sources, &g.selections[0].span), "WHERE visible;");
        assert_eq!(g.end_name.value, "Features");
        let [line, fallback] = g.rules.as_slice() else {
            panic!()
        };
        assert_eq!(line.class.as_ref().unwrap().segments[2].value, "LineSign");
        let [filtered, default] = line.clauses.as_slice() else {
            panic!()
        };
        assert!(matches!(
            doc.expressions[filtered.condition.unwrap().0 as usize].kind,
            ExpressionKind::Path(_)
        ));
        assert!(default.condition.is_none());
        let [shape, priority, geometry] = filtered.assignments.as_slice() else {
            panic!()
        };
        assert!(
            matches!(&shape.value, SignParameterValue::MetaObject(meta) if meta.value.segments.len() == 4 && original(&sources, &meta.span) == "{Symbols.Signs.Palette.Thin}")
        );
        let SignParameterValue::According { path, cases, .. } = &priority.value else {
            panic!()
        };
        assert_eq!(path.elements.len(), 2);
        assert!(
            matches!(cases[0].value, EnumAssignmentValue::Constant(id) if matches!(&doc.expressions[id.0 as usize].kind, ExpressionKind::Literal(LiteralSyntax::Numeric { unit: Some(unit), .. }) if unit.segments[0].value == "m"))
        );
        assert_eq!(
            original(&sources, &cases[0].range.span),
            "#n.low .. #n.high"
        );
        assert!(cases[0].range.last.is_some());
        assert!(
            matches!(&cases[1].value,EnumAssignmentValue::MetaObject(meta) if meta.value.segments[1].value == "Thick")
        );
        assert!(cases[1].range.last.is_none());
        assert!(
            matches!(geometry.value, SignParameterValue::Factor(id) if matches!(doc.expressions[id.0 as usize].kind, ExpressionKind::Path(_)))
        );
        assert!(
            matches!(fallback.clauses[0].assignments[0].value, SignParameterValue::Factor(id) if matches!(doc.expressions[id.0 as usize].kind, ExpressionKind::Call { .. }))
        );
    }

    #[test]
    fn editions_and_required_view_and_graphic_delimiters() {
        let legacy = FIXTURE
            .replace("INTERLIS 2.4", "INTERLIS 2.3")
            .replace("p->visible AND s->visible", "p->visible")
            .replace("CONSTRAINT keep:", "CONSTRAINT");
        for text in [&legacy, FIXTURE] {
            let (_, _, diagnostics) = syntax(text, Limits::default());
            assert!(diagnostics.entries.is_empty(), "{:?}", diagnostics.entries);
        }
        for bad in [
            legacy.replace(
                "derived (FINAL) := p->visible",
                "derived (FINAL) := p->visible AND s->visible",
            ),
            FIXTURE.replace(", b ~ Second (OR NULL), Third", ""),
            FIXTURE.replace(", p ~ Projected; =", "; ="),
            FIXTURE.replace("a ~ Projected,", "a ~ Projected (OR NULL),"),
            FIXTURE.replace("s ~ Special, Other", "s ~ Special, Other;"),
            FIXTURE.replace(
                "OBJECTS OF LineSign: Thin, Thick",
                "OBJECTS OF LineSign: Thin, Thick;",
            ),
            FIXTURE.replace("~ Symbols.Signs", "~ Outer.Symbols.Signs"),
            FIXTURE.replace("#n.low .. #n.high", "#n.low .. 42"),
            FIXTURE.replace("100[m] WHEN IN", "Compute(style) WHEN IN"),
            FIXTURE.replace("Priority := Pick(#normal)", "Priority := 1 + 2"),
            FIXTURE.replace(
                "Shape := {Symbols.Signs.Palette.Thin};",
                "Shape := {Symbols.Signs.Palette.Thin},",
            ),
        ] {
            let report = analyze_text(&bad, Limits::default());
            assert_eq!(report.status(), AnalysisStatus::Rejected, "{bad}");
            assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
    }

    #[test]
    fn recovery_keeps_siblings_and_graphic_nesting_is_bounded() {
        let start = "INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" = TOPIC T=";
        let text = format!(
            "{start} VIEW V=ALL OF ; Bad:=; ALL OF Good; END V; GRAPHIC G=bad:(P:=); good:(P:=5); END G; END T; END M."
        );
        let (_, doc, diagnostics) = syntax(&text, Limits::default());
        assert_eq!(diagnostics.entries.len(), 3, "{:?}", diagnostics.entries);
        let Declaration::Topic {
            declarations,
            end_name,
            ..
        } = &doc.models[0].declarations[0]
        else {
            panic!()
        };
        assert_eq!(end_name.value, "T");
        let [Declaration::View(view), Declaration::Graphic(graphic)] = declarations.as_slice()
        else {
            panic!()
        };
        assert!(
            matches!(&view.attributes[..], [ViewAttribute::AllOf(all)] if all.value.value == "Good")
        );
        assert_eq!(graphic.rules[0].name.value, "good");

        for (max_nesting, status) in [
            (3, AnalysisStatus::LimitExceeded),
            (4, AnalysisStatus::Incomplete),
        ] {
            let limits = Limits {
                max_nesting,
                ..Limits::default()
            };
            let text = format!(
                "{start} GRAPHIC G=r:(P:=ACCORDING a (1 WHEN IN #red)); END G; END T; END M."
            );
            assert_eq!(analyze_text(&text, limits).status(), status);
        }
    }
}
