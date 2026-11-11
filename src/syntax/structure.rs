//! Model/container/relationship syntax. References remain unresolved source names.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelKind {
    Data,
    Type,
    RefSystem,
    Symbology,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Property {
    Abstract,
    Extended,
    Final,
    Transient,
    Oid,
    Hiding,
    Ordered,
    External,
}
#[derive(Debug)]
pub(crate) struct ImportSyntax {
    pub name: Name,
    pub unqualified: Option<Span>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct TranslationSyntax {
    pub original: Name,
    pub version: Located<String>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) enum OidRule {
    Unspecified,
    None(Span),
    Domain(QualifiedName),
}
#[derive(Debug)]
pub(crate) struct TopicOptions {
    pub view: Option<Span>,
    pub properties: Vec<Located<Property>>,
    pub base: Option<QualifiedName>,
    pub basket_oid: Option<QualifiedName>,
    pub oid: Option<QualifiedName>,
    pub dependencies: Vec<QualifiedName>,
    pub deferred_generics: Vec<QualifiedName>,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct Subdivision {
    pub continuous: Option<Span>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) enum Cardinality {
    Unbounded {
        star: Span,
        span: Span,
    },
    Exact {
        number: Located<String>,
        span: Span,
    },
    Range {
        lower: Located<String>,
        upper: CardinalityBound,
        span: Span,
    },
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) enum CardinalityBound {
    Number(Located<String>),
    Unbounded(Span),
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) enum TargetRef {
    Named(QualifiedName),
    AnyClass(Span),
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct RestrictedTarget {
    pub target: TargetRef,
    pub restrictions: Vec<QualifiedName>,
    pub span: Span,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RoleStrength {
    Association,
    Aggregation,
    Composition,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct RoleDecl {
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub strength: Located<RoleStrength>,
    pub cardinality: Option<Cardinality>,
    pub targets: Vec<RestrictedTarget>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct RenamedViewable {
    pub alias: Option<Name>,
    pub target: QualifiedName,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct AssociationDecl {
    pub name: Option<Name>,
    pub properties: Vec<Located<Property>>,
    pub base: Option<QualifiedName>,
    pub derived_from: Option<RenamedViewable>,
    pub oid: OidRule,
    pub roles: Vec<RoleDecl>,
    pub attribute_section: Option<Span>,
    pub attributes: Vec<Declaration>,
    pub cardinality: Option<Cardinality>,
    pub end_name: Option<Name>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) enum ParameterType {
    Attribute(TypeUseSyntax),
    Metaobject(Option<QualifiedName>),
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct ParameterDecl {
    pub name: Name,
    pub properties: Vec<Located<Property>>,
    pub ty: ParameterType,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct ContextBinding {
    pub generic: QualifiedName,
    pub concrete: Vec<QualifiedName>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)] // Preserve complete syntax while its semantics remain unimplemented.
pub(crate) struct ContextDecl {
    pub name: Name,
    pub bindings: Vec<ContextBinding>,
    pub span: Span,
}
const INHERITANCE: &[Property] = &[Property::Abstract, Property::Extended, Property::Final];
const ATTRIBUTES: &[Property] = &[
    Property::Abstract,
    Property::Extended,
    Property::Final,
    Property::Transient,
];

impl Parser<'_> {
    pub(super) fn next_is(&self, text: &str) -> bool {
        self.tokens
            .get(self.at + 1)
            .is_some_and(|t| &self.source()[t.range.clone()] == text)
    }
    pub(super) fn unreserved_name(&self) -> bool {
        self.tokens
            .get(self.at)
            .is_some_and(|t| t.kind == Kind::Word)
            && !keywords::reserved(self.text(), self.language)
    }
    fn marker(&mut self, text: &str) -> Option<Span> {
        let start = self.start();
        self.take(text).then(|| self.span(start, self.end()))
    }
    fn properties(&mut self, allowed: &[Property]) -> Parsed<Vec<Located<Property>>> {
        self.step()?;
        let mut out = vec![];
        if !self.take("(") {
            return Ok(out);
        }
        loop {
            self.step()?;
            let property = match self.text() {
                "ABSTRACT" => Property::Abstract,
                "EXTENDED" => Property::Extended,
                "FINAL" => Property::Final,
                "TRANSIENT" => Property::Transient,
                "OID" => Property::Oid,
                "HIDING" => Property::Hiding,
                "ORDERED" => Property::Ordered,
                "EXTERNAL" => Property::External,
                _ => {
                    return Err(Self::expected(
                        "Expected a property permitted at this position",
                    ));
                }
            };
            if !allowed.contains(&property) {
                return Err(Self::expected("Property is not permitted at this position"));
            }
            self.node()?;
            let span = self.current_span();
            self.at += 1;
            out.push(Located {
                value: property,
                span,
            });
            if !self.take(",") {
                break;
            }
        }
        self.expect(")")?;
        Ok(out)
    }
    fn explanation(&mut self) -> Parsed<Option<Located<String>>> {
        self.step()?;
        match self.tokens.get(self.at).map(|t| &t.kind) {
            Some(Kind::Invalid) => Err(Self::lexical()),
            Some(Kind::Explanation(_)) => {
                self.node()?;
                let token = &self.tokens[self.at];
                let Kind::Explanation(value) = &token.kind else {
                    unreachable!()
                };
                let result = Located {
                    value: value.clone(),
                    span: self.current_span(),
                };
                self.at += 1;
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }
    pub(super) fn model(&mut self) -> Parsed<ModelDecl> {
        self.step()?;
        let start = self.start();
        let contracted = self.marker("CONTRACTED");
        let kind = if self.take("TYPE") {
            ModelKind::Type
        } else if self.take("REFSYSTEM") {
            ModelKind::RefSystem
        } else if self.take("SYMBOLOGY") {
            ModelKind::Symbology
        } else {
            ModelKind::Data
        };
        self.expect("MODEL")?;
        let name = self.name(false)?;
        let language_tag = if self.take("(") {
            let n = self.name(false)?;
            self.expect(")")?;
            Some(n)
        } else {
            None
        };
        let no_incremental_transfer = self.marker("NOINCREMENTALTRANSFER");
        if no_incremental_transfer.is_some() && self.language == Language::V23 {
            return Err(Self::expected(
                "NOINCREMENTALTRANSFER requires INTERLIS 2.4",
            ));
        }
        self.expect("AT")?;
        let issuer = self.quoted()?;
        self.expect("VERSION")?;
        let version = self.quoted()?;
        let explanation = self.explanation()?;
        let translation = if self.take("TRANSLATION") {
            let start = self.tokens[self.at - 1].range.start;
            self.expect("OF")?;
            let original = self.name(false)?;
            self.expect("[")?;
            let version = self.quoted()?;
            self.expect("]")?;
            self.node()?;
            Some(TranslationSyntax {
                original,
                version,
                span: self.span(start, self.end()),
            })
        } else {
            None
        };
        self.expect("=")?;
        let charset = if self.take("CHARSET") {
            if self.language == Language::V23 {
                return Err(Self::expected("CHARSET requires INTERLIS 2.4"));
            }
            let value = self.quoted()?;
            self.expect(";")?;
            Some(value)
        } else {
            None
        };
        let xmlns = if self.take("XMLNS") {
            if self.language == Language::V23 {
                return Err(Self::expected("XMLNS requires INTERLIS 2.4"));
            }
            let value = self.quoted()?;
            self.expect(";")?;
            Some(value)
        } else {
            None
        };
        let mut imports = vec![];
        while self.take("IMPORTS") {
            loop {
                let start = self.start();
                let unqualified = self.marker("UNQUALIFIED");
                let name = self.name(true)?;
                self.node()?;
                imports.push(ImportSyntax {
                    name,
                    unqualified,
                    span: self.span(start, self.end()),
                });
                if !self.take(",") {
                    break;
                }
            }
            self.expect(";")?;
        }
        let declarations = self.declarations(Context::Model)?;
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(".")?;
        self.node()?;
        Ok(ModelDecl {
            name,
            kind,
            contracted,
            no_incremental_transfer,
            language_tag,
            issuer,
            version,
            explanation,
            translation,
            charset,
            xmlns,
            imports,
            declarations,
            end_name,
            span: self.span(start, self.end()),
        })
    }
    fn reference(&mut self, maximum: usize) -> Parsed<QualifiedName> {
        let name = self.qualified_name()?;
        if name.segments.len() > maximum {
            return Err(Issue {
                kind: IssueKind::Syntax,
                primary: Some(name.span),
                message: "Too many name segments in this reference",
            });
        }
        Ok(name)
    }
    fn class_reference(&mut self) -> Parsed<QualifiedName> {
        let name = self.qualified_name_in(true)?;
        if name.segments.len() > 3 {
            return Err(Issue {
                kind: IssueKind::Syntax,
                primary: Some(name.span),
                message: "Too many name segments in class reference",
            });
        }
        Ok(name)
    }
    fn optional_base(&mut self) -> Parsed<Option<QualifiedName>> {
        if self.take("EXTENDS") {
            Ok(Some(self.reference(3)?))
        } else {
            Ok(None)
        }
    }
    fn oid_rule(&mut self) -> Parsed<OidRule> {
        self.step()?;
        let start = self.start();
        if self.take("NO") {
            self.expect("OID")?;
            self.expect(";")?;
            self.node()?;
            Ok(OidRule::None(self.span(start, self.end())))
        } else if self.take("OID") {
            self.expect("AS")?;
            let name = self.reference(3)?;
            self.expect(";")?;
            self.node()?;
            Ok(OidRule::Domain(name))
        } else {
            Ok(OidRule::Unspecified)
        }
    }
    pub(super) fn topic(&mut self) -> Parsed<Declaration> {
        self.step()?;
        let start = self.start();
        let view = self.marker("VIEW");
        self.expect("TOPIC")?;
        let name = self.name(false)?;
        let properties = self.properties(&[Property::Abstract, Property::Final])?;
        let base = if self.take("EXTENDS") {
            Some(self.reference(2)?)
        } else {
            None
        };
        self.expect("=")?;
        let basket_oid = if self.take("BASKET") {
            self.expect("OID")?;
            self.expect("AS")?;
            let n = self.reference(3)?;
            self.expect(";")?;
            Some(n)
        } else {
            None
        };
        let oid = if self.take("OID") {
            self.expect("AS")?;
            let n = self.reference(3)?;
            self.expect(";")?;
            Some(n)
        } else {
            None
        };
        let mut dependencies = vec![];
        while self.take("DEPENDS") {
            self.expect("ON")?;
            dependencies.push(self.reference(2)?);
            while self.take(",") {
                dependencies.push(self.reference(2)?);
            }
            self.expect(";")?;
        }
        let mut deferred_generics = vec![];
        if self.take("DEFERRED") {
            if self.language == Language::V23 {
                return Err(Self::expected("DEFERRED GENERICS requires INTERLIS 2.4"));
            }
            self.expect("GENERICS")?;
            deferred_generics.push(self.reference(3)?);
            while self.take(",") {
                deferred_generics.push(self.reference(3)?);
            }
            self.expect(";")?;
        }
        self.enter()?;
        let declarations = self.declarations(Context::Topic);
        self.depth -= 1;
        let declarations = declarations?;
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(";")?;
        self.node()?;
        Ok(Declaration::Topic {
            name,
            options: TopicOptions {
                view,
                properties,
                base,
                basket_oid,
                oid,
                dependencies,
                deferred_generics,
            },
            declarations,
            end_name,
            span: self.span(start, self.end()),
        })
    }
    fn enter(&mut self) -> Parsed<()> {
        self.step()?;
        if self.depth >= self.limits.max_nesting {
            self.diagnostics
                .limit("Parser nesting budget exceeded", Some(self.current_span()));
            return Err(Self::limited());
        }
        self.depth += 1;
        Ok(())
    }
    pub(super) fn container(&mut self) -> Parsed<Declaration> {
        self.step()?;
        let start = self.start();
        let kind = if self.take("CLASS") {
            ContainerKind::Class
        } else {
            self.expect("STRUCTURE")?;
            ContainerKind::Structure
        };
        let name = self.name(false)?;
        let properties = self.properties(INHERITANCE)?;
        let base = if self.take("EXTENDS") {
            Some(if kind == ContainerKind::Class {
                self.class_reference()?
            } else {
                self.reference(3)?
            })
        } else {
            None
        };
        self.expect("=")?;
        let oid = if kind == ContainerKind::Class {
            self.oid_rule()?
        } else {
            OidRule::Unspecified
        };
        let attribute_section = self.marker("ATTRIBUTE");
        self.enter()?;
        let attributes = self.members();
        self.depth -= 1;
        let attributes = attributes?;
        let parameter_section = self.marker("PARAMETER");
        let mut parameters = vec![];
        if parameter_section.is_some() {
            while self.unreserved_name() {
                parameters.push(self.parameter()?);
            }
        }
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(";")?;
        self.node()?;
        Ok(Declaration::Container {
            kind,
            name,
            properties,
            base,
            oid,
            attribute_section,
            parameter_section,
            parameters,
            attributes,
            end_name,
            span: self.span(start, self.end()),
        })
    }
    fn members(&mut self) -> Parsed<Vec<Declaration>> {
        let mut out = vec![];
        while self.at < self.tokens.len()
            && !self.is("END")
            && !self.is("PARAMETER")
            && !self.is("CARDINALITY")
            && !self.diagnostics.limited
        {
            self.step()?;
            let before = self.at;
            match self.attribute() {
                Ok(value) => out.push(value),
                Err(issue) if matches!(issue.kind, IssueKind::Unsupported) => return Err(issue),
                Err(issue) => {
                    self.report(issue);
                    self.recover(Context::Container, false);
                }
            }
            if self.at == before {
                self.step()?;
                self.at += 1;
            }
        }
        Ok(out)
    }
    pub(super) fn attribute(&mut self) -> Parsed<Declaration> {
        self.step()?;
        let start = self.start();
        if matches!(
            self.text(),
            "MANDATORY" | "UNIQUE" | "EXISTENCE" | "SET" | "CONSTRAINT"
        ) {
            return Err(Self::unsupported(
                "Constraints await the expression syntax implementation",
            ));
        }
        let continuous = self.marker("CONTINUOUS");
        let subdivision = if self.take("SUBDIVISION") {
            self.node()?;
            Some(Subdivision {
                continuous,
                span: self.span(start, self.end()),
            })
        } else if continuous.is_some() {
            return Err(Self::expected("CONTINUOUS requires SUBDIVISION"));
        } else {
            None
        };
        let name = self.name(false)?;
        let properties = self.properties(ATTRIBUTES)?;
        self.expect(":")?;
        let type_use = self.type_use()?;
        if self.is(":=") {
            return Err(Self::unsupported(
                "Attribute derivations await expression syntax",
            ));
        }
        self.expect(";")?;
        self.node()?;
        Ok(Declaration::Attribute {
            name,
            properties,
            subdivision,
            type_use,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn type_use(&mut self) -> Parsed<TypeUseSyntax> {
        self.step()?;
        let start = self.start();
        let mandatory = self.take("MANDATORY");
        let ty = if mandatory && matches!(self.text(), ";" | ":=" | ")") {
            None
        } else {
            Some(self.ty(true)?)
        };
        self.node()?;
        Ok(TypeUseSyntax {
            mandatory,
            ty,
            span: self.span(start, self.end()),
        })
    }
    fn parameter(&mut self) -> Parsed<ParameterDecl> {
        self.step()?;
        let start = self.start();
        let name = self.name(false)?;
        let properties = self.properties(INHERITANCE)?;
        self.expect(":")?;
        let ty = if self.take("METAOBJECT") {
            ParameterType::Metaobject(if self.take("OF") {
                Some(self.class_reference()?)
            } else {
                None
            })
        } else {
            ParameterType::Attribute(self.type_use()?)
        };
        self.expect(";")?;
        self.node()?;
        Ok(ParameterDecl {
            name,
            properties,
            ty,
            span: self.span(start, self.end()),
        })
    }
    fn unsigned_integer(&mut self) -> Parsed<Located<String>> {
        let number = self.number()?;
        if !number.value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Issue {
                kind: IssueKind::Syntax,
                primary: Some(number.span),
                message: "Expected unsigned integer",
            });
        }
        Ok(number)
    }
    fn cardinality(&mut self) -> Parsed<Cardinality> {
        self.step()?;
        let start = self.start();
        self.expect("{")?;
        self.node()?;
        let mut value = if let Some(star) = self.marker("*") {
            Cardinality::Unbounded {
                star,
                span: self.span(start, self.end()),
            }
        } else {
            let lower = self.unsigned_integer()?;
            if self.take("..") {
                self.node()?;
                let upper = if let Some(span) = self.marker("*") {
                    CardinalityBound::Unbounded(span)
                } else {
                    CardinalityBound::Number(self.unsigned_integer()?)
                };
                Cardinality::Range {
                    lower,
                    upper,
                    span: self.span(start, self.end()),
                }
            } else {
                Cardinality::Exact {
                    number: lower,
                    span: self.span(start, self.end()),
                }
            }
        };
        self.expect("}")?;
        match &mut value {
            Cardinality::Unbounded { span, .. }
            | Cardinality::Exact { span, .. }
            | Cardinality::Range { span, .. } => *span = self.span(start, self.end()),
        }
        Ok(value)
    }
    fn restricted_target(&mut self) -> Parsed<RestrictedTarget> {
        self.step()?;
        let start = self.start();
        let target = if let Some(s) = self.marker("ANYCLASS") {
            TargetRef::AnyClass(s)
        } else {
            TargetRef::Named(self.class_reference()?)
        };
        let mut restrictions = vec![];
        if self.take("RESTRICTION") {
            self.expect("(")?;
            restrictions.push(self.class_reference()?);
            while self.take(";") {
                restrictions.push(self.class_reference()?);
            }
            self.expect(")")?;
        }
        self.node()?;
        Ok(RestrictedTarget {
            target,
            restrictions,
            span: self.span(start, self.end()),
        })
    }
    fn renamed_viewable(&mut self) -> Parsed<RenamedViewable> {
        let start = self.start();
        let alias = if self.next_is("~") {
            let n = self.name(false)?;
            self.expect("~")?;
            Some(n)
        } else {
            None
        };
        let target = self.class_reference()?;
        self.node()?;
        Ok(RenamedViewable {
            alias,
            target,
            span: self.span(start, self.end()),
        })
    }
    fn role(
        &mut self,
        name: Name,
        properties: Vec<Located<Property>>,
        start: usize,
    ) -> Parsed<RoleDecl> {
        self.step()?;
        let strength = match self.text() {
            "--" => RoleStrength::Association,
            "-<>" => RoleStrength::Aggregation,
            "-<#>" => RoleStrength::Composition,
            _ => return Err(Self::expected("Expected role strength")),
        };
        self.node()?;
        let strength = Located {
            value: strength,
            span: self.current_span(),
        };
        self.at += 1;
        let cardinality = if self.is("{") {
            Some(self.cardinality()?)
        } else {
            None
        };
        let mut targets = vec![self.restricted_target()?];
        while self.take("OR") {
            targets.push(self.restricted_target()?);
        }
        if self.is(":=") {
            return Err(Self::unsupported(
                "Role derivations await expression syntax",
            ));
        }
        self.expect(";")?;
        self.node()?;
        Ok(RoleDecl {
            name,
            properties,
            strength,
            cardinality,
            targets,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn association(&mut self) -> Parsed<AssociationDecl> {
        self.step()?;
        let start = self.start();
        self.expect("ASSOCIATION")?;
        let name = if self.unreserved_name() {
            Some(self.name(false)?)
        } else {
            None
        };
        let properties = self.properties(&[
            Property::Abstract,
            Property::Extended,
            Property::Final,
            Property::Oid,
        ])?;
        let base = self.optional_base()?;
        let derived_from = if self.take("DERIVED") {
            self.expect("FROM")?;
            Some(self.renamed_viewable()?)
        } else {
            None
        };
        self.expect("=")?;
        let oid = self.oid_rule()?;
        let mut roles = vec![];
        let mut attributes = vec![];
        // Role and attribute prefixes overlap. Parse their shared prefix once,
        // and retain the first attribute rather than rewinding or reparsing.
        while self.unreserved_name() {
            let start = self.start();
            let name = self.name(false)?;
            let properties = self.properties(&[
                Property::Abstract,
                Property::Extended,
                Property::Final,
                Property::Hiding,
                Property::Ordered,
                Property::External,
                Property::Transient,
            ])?;
            if matches!(self.text(), "--" | "-<>" | "-<#>") {
                if properties.iter().any(|p| p.value == Property::Transient) {
                    return Err(Self::expected("TRANSIENT is not a role property"));
                }
                roles.push(self.role(name, properties, start)?);
            } else {
                if properties.iter().any(|p| !ATTRIBUTES.contains(&p.value)) {
                    return Err(Self::expected("Role property on attribute"));
                }
                self.expect(":")?;
                let type_use = self.type_use()?;
                if self.is(":=") {
                    return Err(Self::unsupported(
                        "Attribute derivations await expression syntax",
                    ));
                }
                self.expect(";")?;
                self.node()?;
                attributes.push(Declaration::Attribute {
                    name,
                    properties,
                    subdivision: None,
                    type_use,
                    span: self.span(start, self.end()),
                });
                break;
            }
        }
        let attribute_section = if attributes.is_empty() {
            self.marker("ATTRIBUTE")
        } else {
            None
        };
        self.enter()?;
        let remaining = self.members();
        self.depth -= 1;
        attributes.extend(remaining?);
        let cardinality = if self.take("CARDINALITY") {
            self.expect("=")?;
            let c = self.cardinality()?;
            self.expect(";")?;
            Some(c)
        } else {
            None
        };
        if matches!(
            self.text(),
            "MANDATORY" | "UNIQUE" | "EXISTENCE" | "SET" | "CONSTRAINT"
        ) {
            return Err(Self::unsupported(
                "Association constraints await expression syntax",
            ));
        }
        self.expect("END")?;
        let end_name = if self.unreserved_name() {
            Some(self.name(false)?)
        } else {
            None
        };
        self.expect(";")?;
        self.node()?;
        Ok(AssociationDecl {
            name,
            properties,
            base,
            derived_from,
            oid,
            roles,
            attribute_section,
            attributes,
            cardinality,
            end_name,
            span: self.span(start, self.end()),
        })
    }
    fn qualified_end(&mut self, mut at: usize) -> Parsed<usize> {
        self.step()?;
        if !self.tokens.get(at).is_some_and(|t| t.kind == Kind::Word) {
            return Ok(at);
        }
        at += 1;
        while self
            .tokens
            .get(at)
            .is_some_and(|t| &self.source()[t.range.clone()] == ".")
        {
            self.step()?;
            if !self
                .tokens
                .get(at + 1)
                .is_some_and(|t| t.kind == Kind::Word)
            {
                break;
            }
            at += 2;
        }
        Ok(at)
    }
    fn context_header(&mut self) -> Parsed<bool> {
        if !self.unreserved_name() || !self.next_is("=") {
            return Ok(false);
        }
        // The same `Name =` starts either a mapping or a new context. A mapping
        // must continue with a concrete reference (then OR or ';'); another '='
        // after that reference, or a declaration/end token, starts a new group.
        let rhs = self.at + 2;
        let starts_reference = self.tokens.get(rhs).is_some_and(|t| {
            t.kind == Kind::Word
                && (!keywords::reserved(&self.source()[t.range.clone()], self.language)
                    || &self.source()[t.range.clone()] == "INTERLIS")
        });
        if !starts_reference {
            return Ok(true);
        }
        let end = self.qualified_end(rhs)?;
        Ok(self
            .tokens
            .get(end)
            .is_some_and(|t| &self.source()[t.range.clone()] == "="))
    }
    pub(super) fn contexts(&mut self) -> Parsed<Vec<Declaration>> {
        self.expect("CONTEXT")?;
        let mut contexts = vec![];
        while self.unreserved_name() {
            let start = self.start();
            let name = self.name(false)?;
            self.expect("=")?;
            let mut bindings = vec![];
            while self.unreserved_name() || self.is("INTERLIS") {
                if self.context_header()? {
                    break;
                }
                let start = self.start();
                let generic = self.reference(3)?;
                self.expect("=")?;
                let mut concrete = vec![self.reference(3)?];
                while self.take("OR") {
                    concrete.push(self.reference(3)?);
                }
                self.expect(";")?;
                self.node()?;
                bindings.push(ContextBinding {
                    generic,
                    concrete,
                    span: self.span(start, self.end()),
                });
            }
            self.node()?;
            contexts.push(Declaration::Context(ContextDecl {
                name,
                bindings,
                span: self.span(start, self.end()),
            }));
        }
        Ok(contexts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    pub(super) fn parse(text: &str) -> (SourceSet, Document) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "memory:///structure-😀.ili".into(),
            text: text.into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut budget = SyntaxBudget::new(&limits);
        let doc = super::super::parse(&sources, 0, &limits, &mut budget, &mut diagnostics);
        assert!(
            diagnostics.entries.is_empty(),
            "{text}\n{:?}",
            diagnostics.entries
        );
        (sources, doc.unwrap())
    }
    fn original<'a>(s: &'a SourceSet, span: &Span) -> &'a str {
        let p = s.location(span).unwrap();
        &s.files()[0].text()[p.start_byte as usize..p.end_byte as usize]
    }
    fn model(version: &str, body: &str) -> String {
        format!("INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.")
    }
    #[test]
    fn complete_headers_import_order_and_original_ranges_in_both_editions() {
        for version in ["2.3", "2.4"] {
            let extra = if version == "2.4" {
                "NOINCREMENTALTRANSFER"
            } else {
                ""
            };
            let declarations = if version == "2.4" {
                "CHARSET \"UTF-8\"; XMLNS \"urn:😀\";"
            } else {
                ""
            };
            let text = format!(
                "INTERLIS {version}; CONTRACTED SYMBOLOGY MODEL M (de_CH) {extra} AT \"issuer😀\" VERSION \"v1\" // /* raw */ !! 😀 // TRANSLATION OF Original [\"old\"] = {declarations} IMPORTS UNQUALIFIED X, INTERLIS; IMPORTS UNQUALIFIED Y; END Different. TYPE MODEL Types AT \"x\" VERSION \"2\" = END Types. REFSYSTEM MODEL Ref AT \"x\" VERSION \"2\" = END Ref."
            );
            let (sources, doc) = parse(&text);
            assert_eq!(doc.models.len(), 3);
            let m = &doc.models[0];
            assert_eq!(m.kind, ModelKind::Symbology);
            assert_eq!(doc.models[1].kind, ModelKind::Type);
            assert_eq!(doc.models[2].kind, ModelKind::RefSystem);
            assert_eq!(
                original(&sources, m.contracted.as_ref().unwrap()),
                "CONTRACTED"
            );
            assert_eq!(m.no_incremental_transfer.is_some(), version == "2.4");
            assert_eq!(m.explanation.as_ref().unwrap().value, " /* raw */ !! 😀 ");
            assert_eq!(
                original(&sources, &m.explanation.as_ref().unwrap().span),
                "// /* raw */ !! 😀 //"
            );
            let tr = m.translation.as_ref().unwrap();
            assert_eq!(tr.original.value, "Original");
            assert_eq!(tr.version.value, "old");
            assert_eq!(
                original(&sources, &tr.span),
                "TRANSLATION OF Original [\"old\"]"
            );
            assert_eq!(
                m.imports
                    .iter()
                    .map(|i| i.name.value.as_str())
                    .collect::<Vec<_>>(),
                ["X", "INTERLIS", "Y"]
            );
            assert!(m.imports[0].unqualified.is_some());
            assert!(m.imports[1].unqualified.is_none());
            assert!(m.imports[2].unqualified.is_some());
            assert_eq!(original(&sources, &m.imports[0].span), "UNQUALIFIED X");
            assert_eq!(m.end_name.value, "Different");
            assert_eq!(
                m.charset.as_ref().map(|v| v.value.as_str()),
                (version == "2.4").then_some("UTF-8")
            );
            assert_eq!(
                m.xmlns.as_ref().map(|v| v.value.as_str()),
                (version == "2.4").then_some("urn:😀")
            );
        }
    }
    #[test]
    fn topic_container_properties_sections_oid_and_parameters_are_owned_syntax() {
        for version in ["2.3", "2.4"] {
            let deferred = if version == "2.4" {
                "DEFERRED GENERICS G.Coord, Other.Generic;"
            } else {
                ""
            };
            let text = model(
                version,
                &format!(
                    "VIEW TOPIC T (ABSTRACT,FINAL) EXTENDS Missing.T = BASKET OID AS IDs.Basket; OID AS IDs.Object; DEPENDS ON First.T, Second.T; DEPENDS ON Third; {deferred} CLASS C (ABSTRACT,ABSTRACT,FINAL) EXTENDS Unknown.Base = NO OID; ATTRIBUTE CONTINUOUS SUBDIVISION part (EXTENDED,TRANSIENT): MANDATORY BOOLEAN; PARAMETER p (FINAL): METAOBJECT OF Unknown.Sign; q: TEXT*4; END Wrong; STRUCTURE S EXTENDS Unknown.S = ATTRIBUTE END S; CLASS Stable = OID AS INTERLIS.STANDARDOID; END Stable; END T;"
                ),
            );
            let (sources, doc) = parse(&text);
            let Declaration::Topic {
                options,
                declarations,
                ..
            } = &doc.models[0].declarations[0]
            else {
                panic!()
            };
            assert_eq!(original(&sources, options.view.as_ref().unwrap()), "VIEW");
            assert_eq!(
                options
                    .properties
                    .iter()
                    .map(|p| p.value)
                    .collect::<Vec<_>>(),
                [Property::Abstract, Property::Final]
            );
            assert_eq!(options.base.as_ref().unwrap().segments[0].value, "Missing");
            assert_eq!(
                options.basket_oid.as_ref().unwrap().segments[1].value,
                "Basket"
            );
            assert_eq!(options.oid.as_ref().unwrap().segments[1].value, "Object");
            assert_eq!(
                options
                    .dependencies
                    .iter()
                    .map(|n| n.segments[0].value.as_str())
                    .collect::<Vec<_>>(),
                ["First", "Second", "Third"]
            );
            assert_eq!(
                options.deferred_generics.len(),
                if version == "2.4" { 2 } else { 0 }
            );
            let Declaration::Container {
                properties,
                oid,
                attribute_section,
                parameter_section,
                parameters,
                attributes,
                end_name,
                ..
            } = &declarations[0]
            else {
                panic!()
            };
            assert_eq!(
                properties.iter().map(|p| p.value).collect::<Vec<_>>(),
                [Property::Abstract, Property::Abstract, Property::Final]
            );
            assert!(matches!(oid,OidRule::None(s) if original(&sources,s)=="NO OID;"));
            assert!(attribute_section.is_some());
            assert!(parameter_section.is_some());
            assert_eq!(end_name.value, "Wrong");
            let Declaration::Attribute {
                properties,
                subdivision,
                type_use,
                ..
            } = &attributes[0]
            else {
                panic!()
            };
            assert!(type_use.mandatory);
            assert_eq!(properties[1].value, Property::Transient);
            let sub = subdivision.as_ref().unwrap();
            assert_eq!(original(&sources, &sub.span), "CONTINUOUS SUBDIVISION");
            assert!(sub.continuous.is_some());
            assert_eq!(parameters.len(), 2);
            assert_eq!(parameters[0].name.value, "p");
            assert!(
                matches!(&parameters[0].ty,ParameterType::Metaobject(Some(n)) if n.segments[1].value=="Sign")
            );
            assert!(matches!(
                &declarations[1],
                Declaration::Container {
                    oid: OidRule::Unspecified,
                    ..
                }
            ));
            assert!(
                matches!(&declarations[2],Declaration::Container{oid:OidRule::Domain(n),..} if n.segments[1].value=="STANDARDOID")
            );
        }
    }
    #[test]
    fn ordered_roles_restricted_alternatives_anonymity_and_exact_cardinalities() {
        for version in ["2.3", "2.4"] {
            let text = model(
                version,
                "TOPIC T = ASSOCIATION (OID) EXTENDS Unknown.A DERIVED FROM base ~ Unknown.V = NO OID; first (ORDERED,EXTERNAL) -- {9007199254740993..*} Unknown.T.C RESTRICTION (Restrict.A; Restrict.B) OR ANYCLASS; second -<> {0} Other.C; third -<#> {*} Missing; ATTRIBUTE flag (FINAL): BOOLEAN; CARDINALITY = {1..2}; END; ASSOCIATION Named = a -- X; b -- Y; label:TEXT*2; END Other; END T;",
            );
            let (sources, doc) = parse(&text);
            let Declaration::Topic { declarations, .. } = &doc.models[0].declarations[0] else {
                panic!()
            };
            let Declaration::Association(a) = &declarations[0] else {
                panic!()
            };
            assert!(a.name.is_none());
            assert!(a.end_name.is_none());
            assert_eq!(
                a.roles
                    .iter()
                    .map(|r| r.name.value.as_str())
                    .collect::<Vec<_>>(),
                ["first", "second", "third"]
            );
            assert_eq!(
                a.roles.iter().map(|r| r.strength.value).collect::<Vec<_>>(),
                [
                    RoleStrength::Association,
                    RoleStrength::Aggregation,
                    RoleStrength::Composition
                ]
            );
            assert_eq!(original(&sources, &a.roles[2].strength.span), "-<#>");
            assert!(matches!(a.roles[0].properties[1].value, Property::External));
            let Cardinality::Range {
                lower,
                upper: CardinalityBound::Unbounded(star),
                span,
            } = a.roles[0].cardinality.as_ref().unwrap()
            else {
                panic!()
            };
            assert_eq!(lower.value, "9007199254740993");
            assert_eq!(original(&sources, star), "*");
            assert_eq!(original(&sources, span), "{9007199254740993..*}");
            assert!(
                matches!(a.roles[1].cardinality.as_ref().unwrap(),Cardinality::Exact{number,..} if number.value=="0")
            );
            assert!(matches!(
                a.roles[2].cardinality,
                Some(Cardinality::Unbounded { .. })
            ));
            let target = &a.roles[0].targets[0];
            assert!(matches!(&target.target,TargetRef::Named(n) if n.segments.len()==3));
            assert_eq!(target.restrictions[1].segments[1].value, "B");
            assert!(matches!(
                a.roles[0].targets[1].target,
                TargetRef::AnyClass(_)
            ));
            let derived = a.derived_from.as_ref().unwrap();
            assert_eq!(derived.alias.as_ref().unwrap().value, "base");
            assert_eq!(derived.target.segments[1].value, "V");
            assert_eq!(a.attributes.len(), 1);
            assert!(a.attribute_section.is_some());
            assert!(
                matches!(&a.cardinality,Some(Cardinality::Range{upper:CardinalityBound::Number(n),..}) if n.value=="2")
            );
            let Declaration::Association(named) = &declarations[1] else {
                panic!()
            };
            assert_eq!(named.name.as_ref().unwrap().value, "Named");
            assert_eq!(named.end_name.as_ref().unwrap().value, "Other");
            assert!(named.attribute_section.is_none());
            assert_eq!(named.attributes.len(), 1);
        }
    }
    #[test]
    fn context_groups_empty_groups_and_runtime_parameters_preserve_order() {
        let (sources, doc) = parse(&model(
            "2.4",
            "CONTEXT first = G = One OR Two; M.H = Three; empty = last = J = Four; PARAMETER scale:MANDATORY TEXT*8; other:BOOLEAN;",
        ));
        let items = &doc.models[0].declarations;
        assert_eq!(items.len(), 5);
        let Declaration::Context(first) = &items[0] else {
            panic!()
        };
        assert_eq!(first.name.value, "first");
        assert_eq!(first.bindings.len(), 2);
        assert_eq!(first.bindings[0].generic.segments[0].value, "G");
        assert_eq!(first.bindings[0].concrete[1].segments[0].value, "Two");
        assert_eq!(original(&sources, &first.bindings[1].span), "M.H = Three;");
        let Declaration::Context(empty) = &items[1] else {
            panic!()
        };
        assert_eq!(empty.name.value, "empty");
        assert!(empty.bindings.is_empty());
        let Declaration::Context(last) = &items[2] else {
            panic!()
        };
        assert_eq!(last.name.value, "last");
        assert_eq!(last.bindings[0].generic.segments[0].value, "J");
        assert!(
            matches!(&items[3],Declaration::RuntimeParameter{name,type_use:TypeUseSyntax{mandatory:true,ty:Some(TypeSyntax::Text{..}),..},..} if name.value=="scale")
        );
        parse(&model("2.3", "PARAMETER scale:BOOLEAN;"));
    }
    #[test]
    fn illegal_positions_versions_properties_and_separators_are_syntax_errors() {
        for (version, body) in [
            ("2.4", "ASSOCIATION = a -- C; END;"),
            ("2.4", "PARAMETER p:METAOBJECT;"),
            ("2.4", "STRUCTURE S=p:METAOBJECT; END S;"),
            ("2.4", "ATTRIBUTE"),
            ("2.4", "TOPIC T=TOPIC U=END U; END T;"),
            ("2.4", "STRUCTURE S=NO OID; END S;"),
            ("2.4", "TOPIC T=NO OID; END T;"),
            ("2.4", "TOPIC T (EXTENDED)=END T;"),
            ("2.4", "CLASS C (ORDERED)=END C;"),
            ("2.4", "CLASS C=CONTINUOUS x:BOOLEAN; END C;"),
            (
                "2.4",
                "CLASS C=ATTRIBUTE x:BOOLEAN; ATTRIBUTE y:BOOLEAN; END C;",
            ),
            (
                "2.4",
                "TOPIC T=ASSOCIATION = a (TRANSIENT) -- C; END; END T;",
            ),
            (
                "2.4",
                "TOPIC T=ASSOCIATION = a -- C RESTRICTION (A,B); END; END T;",
            ),
            ("2.4", "TOPIC T=ASSOCIATION = a - <> C; END; END T;"),
            ("2.4", "TOPIC T=ASSOCIATION = a -- {+1..*} C; END; END T;"),
            ("2.4", "TOPIC T=ASSOCIATION = a -- {1.0} C; END; END T;"),
            ("2.4", "TOPIC T=ASSOCIATION = a -- {1..} C; END; END T;"),
            (
                "2.4",
                "TOPIC T=ASSOCIATION = a -- C; PARAMETER p:BOOLEAN; END; END T;",
            ),
            ("2.4", "TOPIC T=DEPENDS ON A.B.C; END T;"),
            ("2.3", "CONTEXT X=G=C;"),
            ("2.3", "TOPIC T=DEFERRED GENERICS G; END T;"),
            ("2.3", "CHARSET \"UTF-8\";"),
            ("2.4", "XMLNS \"urn:x\"; CHARSET \"UTF-8\";"),
            ("2.4", "IMPORTS X,;"),
            ("2.4", "DOMAIN D=MANDATORY CLASS C=END C;"),
        ] {
            let text = model(version, body);
            let result = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text,
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(
                result.status(),
                AnalysisStatus::Rejected,
                "{version}: {body}\n{:?}",
                result.diagnostics()
            );
            assert!(result.graph().is_none());
            assert!(result.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
        for header in [
            "MODEL M NOINCREMENTALTRANSFER AT \"x\" VERSION \"1\" = END M.",
            "MODEL M AT \"x\" VERSION \"1\" TRANSLATION OF X \"v\" = END M.",
        ] {
            let text = format!("INTERLIS 2.3; {header}");
            let result = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text,
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(result.status(), AnalysisStatus::Rejected);
        }
    }
    #[test]
    fn new_syntax_cannot_silently_enter_the_partial_semantic_graph() {
        for body in [
            "TOPIC T (ABSTRACT)=END T;",
            "TOPIC T EXTENDS Unknown.T=END T;",
            "TOPIC T=DEPENDS ON Other.T; END T;",
            "CLASS C=NO OID; END C;",
            "STRUCTURE S (ABSTRACT)=END S;",
            "STRUCTURE S=ATTRIBUTE x (TRANSIENT):BOOLEAN; END S;",
            "STRUCTURE S=SUBDIVISION x:BOOLEAN; END S;",
            "STRUCTURE S=PARAMETER p:BOOLEAN; END S;",
            "TOPIC T=ASSOCIATION = a -- Unknown; END; END T;",
            "CONTEXT C=G=Unknown;",
            "PARAMETER p:BOOLEAN;",
        ] {
            let text = model("2.4", body);
            parse(&text);
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text,
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(report.status(), AnalysisStatus::Incomplete, "{body}");
            assert!(report.graph().is_none());
            assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
        for header in [
            "TYPE MODEL",
            "REFSYSTEM MODEL",
            "SYMBOLOGY MODEL",
            "CONTRACTED MODEL",
        ] {
            let text = format!("INTERLIS 2.4; {header} M AT \"x\" VERSION \"1\" = END M.");
            parse(&text);
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text,
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(report.status(), AnalysisStatus::Incomplete);
            assert!(report.graph().is_none());
        }
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::tests::parse;
    use super::*;
    #[test]
    fn independent_declaration_fixtures_reach_their_final_models() {
        let (_, relations) = parse(include_str!(
            "../../tests/fixtures/syntax/relationships23.ili"
        ));
        assert_eq!(relations.models[0].end_name.value, "Relationships");
        let Declaration::Topic { declarations, .. } = &relations.models[0].declarations[0] else {
            panic!()
        };
        let Declaration::Association(association) = &declarations[2] else {
            panic!()
        };
        assert_eq!(
            association.roles[1].strength.value,
            RoleStrength::Composition
        );
        let (_, contexts) = parse(include_str!(
            "../../tests/fixtures/syntax/headers-contexts24.ili"
        ));
        assert_eq!(contexts.models.len(), 2);
        assert_eq!(
            contexts.models[1]
                .translation
                .as_ref()
                .unwrap()
                .original
                .value,
            "Original"
        );
        assert_eq!(contexts.models[1].end_name.value, "Translation");
    }
}

#[cfg(test)]
mod review_regressions {
    use super::tests::parse;
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    fn model(body: &str, version: &str) -> String {
        format!("INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = {body} END M.")
    }
    #[test]
    fn mandatory_only_is_a_real_type_use_in_every_attribute_and_parameter_position() {
        for version in ["2.3", "2.4"] {
            let text = model(
                "PARAMETER runtime:MANDATORY; STRUCTURE S=ATTRIBUTE value:MANDATORY; PARAMETER p:MANDATORY; END S; TOPIC T=ASSOCIATION = a -- C; value:MANDATORY; END; END T;",
                version,
            );
            let (sources, doc) = parse(&text);
            let items = &doc.models[0].declarations;
            let Declaration::RuntimeParameter { type_use, .. } = &items[0] else {
                panic!()
            };
            assert!(type_use.mandatory);
            assert!(type_use.ty.is_none());
            let pos = sources.location(&type_use.span).unwrap();
            assert_eq!(
                &sources.files()[0].text()[pos.start_byte as usize..pos.end_byte as usize],
                "MANDATORY"
            );
            let Declaration::Container {
                attributes,
                parameters,
                ..
            } = &items[1]
            else {
                panic!()
            };
            assert!(matches!(
                &attributes[0],
                Declaration::Attribute {
                    type_use: TypeUseSyntax {
                        mandatory: true,
                        ty: None,
                        ..
                    },
                    ..
                }
            ));
            assert!(matches!(
                parameters[0].ty,
                ParameterType::Attribute(TypeUseSyntax {
                    mandatory: true,
                    ty: None,
                    ..
                })
            ));
            let Declaration::Topic { declarations, .. } = &items[2] else {
                panic!()
            };
            let Declaration::Association(a) = &declarations[0] else {
                panic!()
            };
            assert!(matches!(
                &a.attributes[0],
                Declaration::Attribute {
                    type_use: TypeUseSyntax {
                        mandatory: true,
                        ty: None,
                        ..
                    },
                    ..
                }
            ));
            for body in [
                "STRUCTURE S=value:MANDATORY; END S;",
                "STRUCTURE S=PARAMETER p:MANDATORY; END S;",
                "PARAMETER p:MANDATORY;",
            ] {
                let report = analyze(CompileRequest {
                    sources: vec![SourceInput {
                        name: "x".into(),
                        text: model(body, version),
                    }],
                    limits: Limits::default(),
                })
                .unwrap();
                assert_eq!(report.status(), AnalysisStatus::Incomplete, "{body}");
                assert!(report.graph().is_none());
                assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
            }
        }
    }
    #[test]
    fn empty_context_groups_survive_every_section_terminator() {
        for tail in [
            "",
            "DOMAIN D=BOOLEAN;",
            "TOPIC T=END T;",
            "CLASS C=END C;",
            "STRUCTURE S=END S;",
            "PARAMETER p:BOOLEAN;",
            "CONTEXT other=G=X;",
        ] {
            let text = model(&format!("CONTEXT first=G=X; empty= {tail}"), "2.4");
            let (_, doc) = parse(&text);
            let items = &doc.models[0].declarations;
            let Declaration::Context(first) = &items[0] else {
                panic!()
            };
            assert_eq!(first.bindings.len(), 1);
            let Declaration::Context(empty) = &items[1] else {
                panic!()
            };
            assert_eq!(empty.name.value, "empty");
            assert!(empty.bindings.is_empty());
        }
        let (_, doc) = parse(&model("CONTEXT a=b=c=", "2.4"));
        assert_eq!(doc.models[0].declarations.len(), 3);
        for item in &doc.models[0].declarations {
            let Declaration::Context(c) = item else {
                panic!()
            };
            assert!(c.bindings.is_empty());
        }
        for tail in [
            "UNIT u;",
            "FUNCTION f (x:BOOLEAN):BOOLEAN;",
            "LINE FORM curve:Segment;",
            "SIGN BASKET B~S;",
            "REFSYSTEM BASKET B~S;",
        ] {
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text: model(&format!("CONTEXT first=G=X; empty= {tail}"), "2.4"),
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(
                report.status(),
                AnalysisStatus::Incomplete,
                "{tail}: {:?}",
                report.diagnostics()
            );
            assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
    }
    #[test]
    fn predefined_classes_are_preserved_only_in_class_reference_contexts() {
        for version in ["2.3", "2.4"] {
            for class in ["SIGN", "METAOBJECT", "REFSYSTEM"] {
                for qualified in [class.to_string(), format!("INTERLIS.{class}")] {
                    let text = model(
                        &format!(
                            "CLASS C (ABSTRACT) EXTENDS {qualified}=PARAMETER p:METAOBJECT OF {qualified}; END C; TOPIC T=ASSOCIATION DERIVED FROM alias~{qualified}= a -- {qualified} RESTRICTION ({qualified}); END; END T;"
                        ),
                        version,
                    );
                    let (sources, doc) = parse(&text);
                    let Declaration::Container {
                        base, parameters, ..
                    } = &doc.models[0].declarations[0]
                    else {
                        panic!()
                    };
                    let base = base.as_ref().unwrap();
                    assert_eq!(base.segments.last().unwrap().value, class);
                    assert_eq!(
                        base.segments.len(),
                        if qualified.starts_with("INTERLIS.") {
                            2
                        } else {
                            1
                        }
                    );
                    for segment in &base.segments {
                        let p = sources.location(&segment.span).unwrap();
                        assert_eq!(
                            &sources.files()[0].text()[p.start_byte as usize..p.end_byte as usize],
                            segment.value
                        );
                    }
                    assert!(
                        matches!(&parameters[0].ty,ParameterType::Metaobject(Some(q)) if q.segments.last().unwrap().value==class)
                    );
                    let Declaration::Topic { declarations, .. } = &doc.models[0].declarations[1]
                    else {
                        panic!()
                    };
                    let Declaration::Association(a) = &declarations[0] else {
                        panic!()
                    };
                    assert_eq!(
                        a.derived_from
                            .as_ref()
                            .unwrap()
                            .target
                            .segments
                            .last()
                            .unwrap()
                            .value,
                        class
                    );
                    assert!(
                        matches!(&a.roles[0].targets[0].target,TargetRef::Named(q) if q.segments.last().unwrap().value==class)
                    );
                }
            }
        }
        for body in [
            "TOPIC T=LINE FORM curve:S; END T;",
            "CLASS C EXTENDS Other.SIGN=END C;",
            "CLASS C EXTENDS SIGN.Other=END C;",
            "CLASS C EXTENDS INTERLIS.SIGN.Other=END C;",
            "TOPIC T EXTENDS SIGN=END T;",
        ] {
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "x".into(),
                    text: model(body, "2.4"),
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(
                report.status(),
                AnalysisStatus::Rejected,
                "{body}: {:?}",
                report.diagnostics()
            );
            assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
    }
}
