//! Private syntax values. No parser-library type crosses this module's boundary.
use crate::{
    Limits, SourceSet, Span,
    diagnostic::Diagnostics,
    keywords,
    lexer::{self, Kind, Token},
};

pub(crate) use crate::model::LanguageVersion as Language;
#[derive(Debug)]
pub(crate) struct Located<T> {
    pub value: T,
    pub span: Span,
}
pub(crate) type Name = Located<String>;
#[derive(Debug)]
pub(crate) struct QualifiedName {
    pub segments: Vec<Name>,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct Document {
    pub language: Language,
    pub models: Vec<ModelDecl>,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct ModelDecl {
    pub name: Name,
    pub language_tag: Option<Name>,
    pub issuer: Located<String>,
    pub version: Located<String>,
    pub imports: Vec<Name>,
    pub declarations: Vec<Declaration>,
    pub end_name: Name,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) enum Declaration {
    Topic {
        name: Name,
        declarations: Vec<Declaration>,
        end_name: Name,
        span: Span,
    },
    Container {
        kind: ContainerKind,
        name: Name,
        attributes: Vec<Declaration>,
        end_name: Name,
        span: Span,
    },
    Domain {
        name: Name,
        ty: DomainType,
        mandatory: bool,
        span: Span,
    },
    Attribute {
        name: Name,
        ty: TypeSyntax,
        mandatory: bool,
        span: Span,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Class,
    Structure,
}
#[derive(Debug)]
pub(crate) enum DomainType {
    Concrete(TypeSyntax),
    MandatoryDerivation(QualifiedName),
}
#[derive(Debug)]
pub(crate) enum TypeSyntax {
    Text {
        max_length: Located<String>,
        span: Span,
    },
    Boolean {
        span: Span,
    },
    Numeric {
        lower: Located<String>,
        upper: Located<String>,
        span: Span,
    },
    Named(QualifiedName),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Model,
    Topic,
    Container,
}
struct Issue {
    primary: Option<Span>,
    unsupported: bool,
    message: &'static str,
}
type Parsed<T> = Result<T, Issue>;
struct Parser<'a> {
    sources: &'a SourceSet,
    file: usize,
    tokens: Vec<Token>,
    at: usize,
    language: Language,
    depth: u32,
    limits: &'a Limits,
    diagnostics: &'a mut Diagnostics,
}
impl Parser<'_> {
    fn source(&self) -> &str {
        self.sources.files()[self.file].text()
    }
    fn start(&self) -> usize {
        self.tokens
            .get(self.at)
            .map_or(self.source().len(), |t| t.range.start)
    }
    fn end(&self) -> usize {
        self.at
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(0, |t| t.range.end)
    }
    fn span(&self, start: usize, end: usize) -> Span {
        self.sources
            .span(self.file, start, end)
            .expect("lexer boundaries in bounded source")
    }
    fn current_span(&self) -> Span {
        self.span(
            self.start(),
            self.tokens
                .get(self.at)
                .map_or(self.source().len(), |t| t.range.end),
        )
    }
    fn text(&self) -> &str {
        self.tokens
            .get(self.at)
            .map_or("", |t| &self.source()[t.range.clone()])
    }
    fn is(&self, text: &str) -> bool {
        self.text() == text
    }
    fn take(&mut self, text: &str) -> bool {
        if self.is(text) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expected(message: &'static str) -> Issue {
        Issue {
            primary: None,
            unsupported: false,
            message,
        }
    }
    fn unsupported(message: &'static str) -> Issue {
        Issue {
            primary: None,
            unsupported: true,
            message,
        }
    }
    fn expect(&mut self, text: &str) -> Parsed<()> {
        if self.take(text) {
            Ok(())
        } else {
            Err(Self::expected("Missing or unexpected syntax token"))
        }
    }
    fn report(&mut self, issue: Issue) {
        self.diagnostics.push(
            if issue.unsupported {
                "ILI-UNSUPPORTED"
            } else {
                "ILI-SYNTAX"
            },
            issue.message,
            Some(issue.primary.unwrap_or_else(|| self.current_span())),
        );
    }
    fn name(&mut self, builtin: bool) -> Parsed<Name> {
        let text = self.text();
        if !matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Kind::Word))
            || (keywords::reserved(text, self.language) && !(builtin && text == "INTERLIS"))
        {
            return Err(Self::expected("Expected a non-reserved identifier"));
        }
        let name = Located {
            value: text.into(),
            span: self.current_span(),
        };
        self.at += 1;
        Ok(name)
    }
    fn quoted(&mut self) -> Parsed<Located<String>> {
        let Some(Token {
            kind: Kind::String(value),
            ..
        }) = self.tokens.get(self.at)
        else {
            return Err(Self::expected("Expected a quoted string"));
        };
        let result = Located {
            value: value.clone(),
            span: self.current_span(),
        };
        self.at += 1;
        Ok(result)
    }
    fn qualified_name(&mut self) -> Parsed<QualifiedName> {
        let start = self.start();
        let mut segments = vec![self.name(true)?];
        while self.take(".") {
            // BOOLEAN is a supported predefined domain in INTERLIS; other predefined
            // names that are identifiers are preserved for semantic classification.
            if segments.len() == 1 && segments[0].value == "INTERLIS" && self.take("BOOLEAN") {
                segments.push(Located {
                    value: "BOOLEAN".into(),
                    span: self.span(self.tokens[self.at - 1].range.start, self.end()),
                });
            } else {
                if segments.len() == 1
                    && segments[0].value == "INTERLIS"
                    && keywords::reserved(self.text(), self.language)
                {
                    return Err(Self::unsupported(
                        "Predefined type is outside the supported subset",
                    ));
                }
                segments.push(self.name(false)?);
            }
        }
        if self.is("..") {
            return Err(Self::expected(
                "Expected a single dot between qualified name segments",
            ));
        }
        Ok(QualifiedName {
            segments,
            span: self.span(start, self.end()),
        })
    }
    fn number(&mut self) -> Parsed<Located<String>> {
        let start = self.start();
        let signed = self.take("-") || self.take("+");
        let Some(token) = self.tokens.get(self.at) else {
            return Err(Self::expected("Expected a number"));
        };
        if token.kind != Kind::Number || (signed && self.end() != token.range.start) {
            return Err(Self::expected("Expected a number with adjacent sign"));
        }
        self.at += 1;
        Ok(Located {
            value: self.source()[start..self.end()].into(),
            span: self.span(start, self.end()),
        })
    }
    fn ty(&mut self, named: bool) -> Parsed<TypeSyntax> {
        let ty = self.type_base(named)?;
        if matches!(
            self.text(),
            "[" | "{"
                | "<"
                | "CIRCULAR"
                | "CLOCKWISE"
                | "COUNTERCLOCKWISE"
                | "CONSTRAINTS"
                | "WITH"
                | "REFSYSTEM"
                | "REFSYS"
                | "FORMAT"
        ) {
            return Err(Self::unsupported(
                "Type modifiers are outside the supported subset",
            ));
        }
        Ok(ty)
    }
    fn type_base(&mut self, named: bool) -> Parsed<TypeSyntax> {
        let start = self.start();
        if self.take("TEXT") {
            if !self.take("*") {
                return Err(Self::unsupported(
                    "Unbounded TEXT is outside the supported subset",
                ));
            }
            let max_length = self.number()?;
            if max_length.value.bytes().any(|b| !b.is_ascii_digit()) {
                return Err(Issue {
                    primary: Some(max_length.span),
                    unsupported: false,
                    message: "TEXT length must be an unsigned integer",
                });
            }
            return Ok(TypeSyntax::Text {
                max_length,
                span: self.span(start, self.end()),
            });
        }
        if self.take("BOOLEAN") {
            return Ok(TypeSyntax::Boolean {
                span: self.span(start, self.end()),
            });
        }
        if self.is("-")
            || self.is("+")
            || self
                .tokens
                .get(self.at)
                .is_some_and(|t| t.kind == Kind::Number)
        {
            let lower = self.number()?;
            self.expect("..")?;
            let upper = self.number()?;
            return Ok(TypeSyntax::Numeric {
                lower,
                upper,
                span: self.span(start, self.end()),
            });
        }
        if matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Kind::Word))
            && (!keywords::reserved(self.text(), self.language) || self.is("INTERLIS"))
        {
            if !named {
                return Err(Self::expected(
                    "Domain definitions require a concrete type; direct aliases are invalid",
                ));
            }
            return Ok(TypeSyntax::Named(self.qualified_name()?));
        }
        if self.is(";") || self.is("END") || self.at == self.tokens.len() {
            return Err(Self::expected("Expected a supported type"));
        }
        Err(Self::unsupported(
            "Type construction is outside the supported subset",
        ))
    }
    fn recover(&mut self) {
        while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
            if self.take(";") {
                return;
            }
            self.at += 1;
        }
    }
    fn declarations(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        let mut declarations = vec![];
        while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
            let before = self.at;
            match self.declaration(context) {
                Ok(mut items) => declarations.append(&mut items),
                Err(issue) if issue.unsupported => return Err(issue),
                Err(issue) => {
                    self.report(issue);
                    self.recover();
                }
            }
            if self.at == before {
                self.at += 1;
            }
        }
        Ok(declarations)
    }
    fn declaration(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        let start = self.start();
        if matches!(self.text(), ";" | "." | "," | "}" | ")" | "]" | ":=") {
            return Err(Self::expected("Unexpected separator at declaration start"));
        }

        if context == Context::Container {
            if keywords::reserved(self.text(), self.language) {
                return Err(Self::unsupported(
                    "Container member construction is outside the supported subset",
                ));
            }
            let name = self.name(false)?;
            if self.is("(") {
                return Err(Self::unsupported(
                    "Attribute properties are outside the supported subset",
                ));
            }
            self.expect(":")?;
            let mandatory = self.take("MANDATORY");
            let ty = self.ty(true)?;
            if self.is(":=") {
                return Err(Self::unsupported(
                    "Attribute value assignments are outside the supported subset",
                ));
            }
            self.expect(";")?;
            return Ok(vec![Declaration::Attribute {
                name,
                ty,
                mandatory,
                span: self.span(start, self.end()),
            }]);
        }
        if self.take("DOMAIN") {
            let mut items = vec![];
            while self
                .tokens
                .get(self.at)
                .is_some_and(|t| t.kind == Kind::Word)
                && !keywords::reserved(self.text(), self.language)
            {
                let start = self.start();
                let name = self.name(false)?;
                let parsed = (|| {
                    if self.is("(") {
                        return Err(Self::unsupported(
                            "Domain properties are outside the supported subset",
                        ));
                    }
                    let (ty, mandatory) = if self.take("EXTENDS") {
                        let base = self.qualified_name()?;
                        self.expect("=")?;
                        if self.is(";") || self.is("END") || self.at == self.tokens.len() {
                            return Err(Self::expected(
                                "Expected a type or MANDATORY after derived domain '='",
                            ));
                        }
                        if !self.take("MANDATORY") {
                            return Err(Self::unsupported(
                                "Only EXTENDS Base = MANDATORY domain derivations are supported",
                            ));
                        }
                        if self.is("END") || self.at == self.tokens.len() {
                            return Err(Self::expected(
                                "Expected ';' after mandatory domain derivation",
                            ));
                        }
                        if !self.is(";") {
                            return Err(Self::unsupported(
                                "Refined inherited types are outside the supported subset",
                            ));
                        }
                        (DomainType::MandatoryDerivation(base), true)
                    } else {
                        self.expect("=")?;
                        let mandatory = self.take("MANDATORY");
                        (DomainType::Concrete(self.ty(false)?), mandatory)
                    };
                    self.expect(";")?;
                    Ok(Declaration::Domain {
                        name,
                        ty,
                        mandatory,
                        span: self.span(start, self.end()),
                    })
                })();
                match parsed {
                    Ok(item) => items.push(item),
                    Err(issue) if issue.unsupported => return Err(issue),
                    Err(issue) => {
                        self.report(issue);
                        self.recover();
                    }
                }
                if self.diagnostics.limited {
                    break;
                }
            }
            return Ok(items);
        }
        let topic = self.is("TOPIC");
        let kind = if self.take("CLASS") {
            Some(ContainerKind::Class)
        } else if self.take("STRUCTURE") {
            Some(ContainerKind::Structure)
        } else {
            None
        };
        if topic || kind.is_some() {
            if topic {
                self.at += 1;
                if context != Context::Model {
                    return Err(Self::expected("TOPIC may only occur in a model"));
                }
            }
            let name = self.name(false)?;
            if self.is("(") || self.is("EXTENDS") {
                return Err(Self::unsupported(
                    "Container modifiers or inheritance are outside the supported subset",
                ));
            }
            self.expect("=")?;
            if self.depth >= self.limits.max_nesting {
                self.diagnostics.push(
                    "ILI-LIMIT",
                    "Parser nesting budget exceeded",
                    Some(self.current_span()),
                );
                return Err(Self::expected("Nesting budget exceeded"));
            }
            self.depth += 1;
            let members = self.declarations(if topic {
                Context::Topic
            } else {
                Context::Container
            });
            self.depth -= 1;
            let members = members?;
            self.expect("END")?;
            let end_name = self.name(false)?;
            self.expect(";")?;
            let span = self.span(start, self.end());
            return Ok(vec![if let Some(kind) = kind {
                Declaration::Container {
                    kind,
                    name,
                    attributes: members,
                    end_name,
                    span,
                }
            } else {
                Declaration::Topic {
                    name,
                    declarations: members,
                    end_name,
                    span,
                }
            }]);
        }
        Err(Self::unsupported(
            "Declaration construction is outside the supported subset",
        ))
    }
    fn import_name(&mut self) -> Parsed<Name> {
        if self.is("UNQUALIFIED") {
            return Err(Self::unsupported(
                "UNQUALIFIED imports are outside the supported subset",
            ));
        }
        self.name(true)
    }
    fn model(&mut self) -> Parsed<ModelDecl> {
        let start = self.start();
        self.expect("MODEL")?;
        let name = self.name(false)?;
        let language_tag = if self.take("(") {
            let tag = self.name(false)?;
            self.expect(")")?;
            Some(tag)
        } else {
            None
        };
        if self.language == Language::V24 && self.is("NOINCREMENTALTRANSFER") {
            return Err(Self::unsupported(
                "NOINCREMENTALTRANSFER is outside the supported subset",
            ));
        }
        self.expect("AT")?;
        let issuer = self.quoted()?;
        self.expect("VERSION")?;
        let version = self.quoted()?;
        if self.is("TRANSLATION") || self.source()[self.start()..].starts_with("//") {
            return Err(Self::unsupported(
                "Additional model header options are outside the supported subset",
            ));
        }
        self.expect("=")?;
        let mut imports = vec![];
        while self.take("IMPORTS") {
            imports.push(self.import_name()?);
            while self.take(",") {
                imports.push(self.import_name()?);
            }
            self.expect(";")?;
        }
        let declarations = self.declarations(Context::Model)?;
        self.expect("END")?;
        let end_name = self.name(false)?;
        self.expect(".")?;
        Ok(ModelDecl {
            name,
            language_tag,
            issuer,
            version,
            imports,
            declarations,
            end_name,
            span: self.span(start, self.end()),
        })
    }
    fn document(&mut self) -> Parsed<Document> {
        let start = self.start();
        if !self.take("INTERLIS") {
            return Err(Self::unsupported(
                "Unsupported or missing INTERLIS language header",
            ));
        }
        self.language = if self.take("2.3") {
            Language::V23
        } else if self.take("2.4") {
            Language::V24
        } else {
            return Err(Self::unsupported("Only INTERLIS 2.3 and 2.4 are supported"));
        };
        self.expect(";")?;
        let mut models = vec![];
        while self.at < self.tokens.len() && !self.diagnostics.limited {
            if !self.is("MODEL") {
                if models.is_empty()
                    || matches!(
                        self.text(),
                        "CONTRACTED" | "TYPE" | "REFSYSTEM" | "SYMBOLOGY"
                    )
                {
                    return Err(Self::unsupported(
                        "Model construction is outside the supported subset",
                    ));
                }
                return Err(Self::expected(
                    "Unexpected token after complete model; EOF required",
                ));
            }
            models.push(self.model()?);
        }
        Ok(Document {
            language: self.language,
            models,
            span: self.span(start, self.source().len()),
        })
    }
}

pub(crate) fn parse(
    sources: &SourceSet,
    file: usize,
    limits: &Limits,
    remaining: &mut u32,
    diagnostics: &mut Diagnostics,
) -> Option<Document> {
    let tokens = lexer::lex(sources, file, limits, remaining, diagnostics);
    if diagnostics.limited {
        return None;
    }
    let mut parser = Parser {
        sources,
        file,
        tokens,
        at: 0,
        language: Language::V24,
        depth: 0,
        limits,
        diagnostics,
    };
    match parser.document() {
        Ok(document) => Some(document),
        Err(issue) => {
            parser.report(issue);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceInput;
    const HEADER: &str =
        "INTERLIS 2.4; MODEL A (en) AT \"https://example.invalid\" VERSION \"1\" = ";
    fn parse_text(text: &str, limits: Limits) -> (SourceSet, Option<Document>, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "a".into(),
            text: text.into(),
        }])
        .unwrap();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut remaining = limits.max_tokens;
        let document = parse(&sources, 0, &limits, &mut remaining, &mut diagnostics);
        (sources, document, diagnostics)
    }
    fn original<'a>(sources: &'a SourceSet, span: &Span) -> &'a str {
        let l = sources.location(span).unwrap();
        &sources.files()[0].text()[l.start_byte as usize..l.end_byte as usize]
    }
    #[test]
    fn typed_multi_model_ast_and_exact_declaration_ranges() {
        let text = format!(
            "\u{feff}!! 😀\r\n{HEADER}DOMAIN Label=TEXT*40; Count=-1..100; Required EXTENDS Count = MANDATORY; TOPIC T = STRUCTURE S = label:MANDATORY Label; count:Wrong.Base.Count; END S; END T; END A. MODEL B AT \"b\" VERSION \"2\" = IMPORTS A, INTERLIS; END B."
        );
        let (sources, doc, d) = parse_text(&text, Limits::default());
        assert!(d.entries.is_empty(), "{:?}", d.entries);
        let doc = doc.unwrap();
        assert_eq!(doc.language, Language::V24);
        assert_eq!(doc.models.len(), 2);
        let a = &doc.models[0];
        assert_eq!(a.name.value, "A");
        assert_eq!(original(&sources, &a.name.span), "A");
        assert_eq!(a.language_tag.as_ref().unwrap().value, "en");
        assert_eq!(a.issuer.value, "https://example.invalid");
        assert_eq!(a.version.value, "1");
        assert_eq!(original(&sources, &a.version.span), "\"1\"");
        assert_eq!(original(&sources, &a.end_name.span), "A");
        assert_eq!(a.declarations.len(), 4);
        let Declaration::Domain {
            ty: DomainType::Concrete(TypeSyntax::Text { max_length, .. }),
            ..
        } = &a.declarations[0]
        else {
            panic!()
        };
        assert_eq!(max_length.value, "40");
        assert_eq!(original(&sources, &max_length.span), "40");
        let Declaration::Domain {
            ty: DomainType::Concrete(TypeSyntax::Numeric { lower, upper, .. }),
            ..
        } = &a.declarations[1]
        else {
            panic!()
        };
        assert_eq!(lower.value, "-1");
        assert_eq!(original(&sources, &lower.span), "-1");
        assert_eq!(upper.value, "100");
        let Declaration::Domain {
            ty: DomainType::MandatoryDerivation(base),
            mandatory,
            ..
        } = &a.declarations[2]
        else {
            panic!()
        };
        assert!(*mandatory);
        assert_eq!(base.segments[0].value, "Count");
        let Declaration::Topic { declarations, .. } = &a.declarations[3] else {
            panic!()
        };
        let Declaration::Container {
            kind, attributes, ..
        } = &declarations[0]
        else {
            panic!()
        };
        assert_eq!(*kind, ContainerKind::Structure);
        let Declaration::Attribute {
            mandatory, span, ..
        } = &attributes[0]
        else {
            panic!()
        };
        assert!(*mandatory);
        assert_eq!(original(&sources, span), "label:MANDATORY Label;");
        let Declaration::Attribute {
            ty: TypeSyntax::Named(q),
            ..
        } = &attributes[1]
        else {
            panic!()
        };
        assert_eq!(original(&sources, &q.span), "Wrong.Base.Count");
        assert_eq!(
            q.segments
                .iter()
                .map(|s| s.value.as_str())
                .collect::<Vec<_>>(),
            ["Wrong", "Base", "Count"]
        );
        for name in &q.segments {
            assert_eq!(original(&sources, &name.span), name.value);
        }
        assert_eq!(
            doc.models[1]
                .imports
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>(),
            ["A", "INTERLIS"]
        );
        assert_eq!(
            original(&sources, &doc.span),
            text.trim_start_matches('\u{feff}')
                .split_once('\n')
                .unwrap()
                .1
        );
    }
    #[test]
    fn nested_comments_eof_and_original_string() {
        let text = format!("/* outer /* inner */ outer */ {HEADER}END A.");
        let (_, doc, d) = parse_text(&text, Limits::default());
        assert!(doc.is_some());
        assert!(d.entries.is_empty());
        for tail in [" GARBAGE", " /* outer /* inner */"] {
            let (_, _, d) = parse_text(&(text.clone() + tail), Limits::default());
            assert!(d.rejected);
            assert!(!d.incomplete);
        }
        let text = r#"INTERLIS 2.3; MODEL A AT "a\"/* // 😀\uD83D\uDE00" VERSION "1" = END A."#;
        let (sources, doc, d) = parse_text(text, Limits::default());
        assert!(d.entries.is_empty());
        let doc = doc.unwrap();
        assert_eq!(doc.models[0].issuer.value, "a\"/* // 😀😀");
        assert_eq!(
            original(&sources, &doc.models[0].issuer.span),
            r#""a\"/* // 😀\uD83D\uDE00""#
        );
        for bad in [r#""bad\n""#, r#""bad\uD800""#] {
            let text = format!("INTERLIS 2.4; MODEL A AT {bad} VERSION \"1\" = END A.");
            let (_, _, d) = parse_text(&text, Limits::default());
            assert!(d.rejected);
        }
    }
    #[test]
    fn recovery_keeps_second_separated_error_and_progress() {
        let text = format!("{HEADER}DOMAIN Bad=; Worse=; Good=BOOLEAN; END A.");
        let (sources, doc, d) = parse_text(&text, Limits::default());
        assert!(doc.is_some());
        assert_eq!(d.entries.len(), 2);
        assert!(d.rejected);
        assert!(!d.limited);
        let offsets = d
            .entries
            .iter()
            .map(|d| {
                sources
                    .location(d.primary.as_ref().unwrap())
                    .unwrap()
                    .start_byte
            })
            .collect::<Vec<_>>();
        assert!(offsets[0] < offsets[1]);
        assert_eq!(doc.unwrap().models[0].declarations.len(), 1);
    }
    #[test]
    fn lexical_boundaries_signs_and_original_precision() {
        let sources = SourceSet::new(vec![SourceInput {
            name: "a".into(),
            text: "1..2 -1.0 a-1 1.0e-2".into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut d = Diagnostics::new(&limits);
        let mut remaining = limits.max_tokens;
        let t = lexer::lex(&sources, 0, &limits, &mut remaining, &mut d);
        assert_eq!(
            t.iter()
                .map(|t| &sources.files()[0].text()[t.range.clone()])
                .collect::<Vec<_>>(),
            ["1", "..", "2", "-", "1.0", "a", "-", "1", "1.0e-2"]
        );
        let text = format!("{HEADER}DOMAIN N=-0.1200E3..0.1201E3; END A.");
        let (_, doc, d) = parse_text(&text, limits);
        assert!(d.entries.is_empty());
        let Declaration::Domain {
            ty: DomainType::Concrete(TypeSyntax::Numeric { lower, upper, .. }),
            ..
        } = &doc.unwrap().models[0].declarations[0]
        else {
            panic!()
        };
        assert_eq!(lower.value, "-0.1200E3");
        assert_eq!(upper.value, "0.1201E3");
        let (_, _, d) = parse_text(
            &format!("{HEADER}DOMAIN N=- 1..2; END A."),
            Limits::default(),
        );
        assert!(d.rejected);
    }
    #[test]
    fn narrow_derivation_and_unsupported_constructs_never_disappear() {
        let (_, _, d) = parse_text(
            &format!("{HEADER}DOMAIN Alias=Base; END A."),
            Limits::default(),
        );
        assert!(d.rejected);
        assert!(!d.incomplete);
        for body in [
            "DOMAIN Alias EXTENDS Base = TEXT*2;",
            "CLASS B EXTENDS C = END B;",
            "VIEW V PROJECTION OF B; = ALL OF B; END V;",
            "CLASS B = MANDATORY CONSTRAINT x == 1; END B;",
            "DOMAIN E=(red,green);",
            "UNIT metre=1[INTERLIS.m];",
        ] {
            let (_, _, d) = parse_text(&format!("{HEADER}{body} END A."), Limits::default());
            assert!(d.incomplete, "{body}");
            assert!(
                d.entries
                    .iter()
                    .any(|e| e.code == "ILI-UNSUPPORTED" && e.primary.is_some())
            );
        }
    }
    #[test]
    fn actual_version_controls_complete_reserved_tables() {
        for (version, word, valid) in [
            ("2.3", "XMLNS", true),
            ("2.4", "XMLNS", false),
            ("2.3", "XML", true),
            ("2.4", "XML", true),
            ("2.3", "SUBDIVISION", false),
            ("2.4", "NOINCREMENTALTRANSFER", false),
        ] {
            let text =
                format!("INTERLIS {version}; MODEL {word} AT \"x\" VERSION \"1\" = END {word}.");
            let (_, doc, d) = parse_text(&text, Limits::default());
            assert_eq!(doc.is_some() && !d.rejected, valid, "{text}");
        }
    }
    #[test]
    fn budgets_stop_with_explicit_flag_even_after_full_diagnostics() {
        let text = format!("{HEADER}DOMAIN A=; B=; C=; END A.");
        let (_, _, d) = parse_text(
            &text,
            Limits {
                max_diagnostics: 1,
                ..Limits::default()
            },
        );
        assert!(d.limited);
        assert_eq!(d.entries.len(), 1);
        assert_eq!(d.entries[0].code, "ILI-LIMIT");
        let (_, _, d) = parse_text(
            &format!("{HEADER}END A."),
            Limits {
                max_tokens: 5,
                ..Limits::default()
            },
        );
        assert!(d.limited);
        let (_, _, d) = parse_text(
            &format!("{}{}", "/*".repeat(257), "*/".repeat(257)),
            Limits::default(),
        );
        assert!(d.limited);
        let (_, _, d) = parse_text(
            &format!("{HEADER}TOPIC T = CLASS C = END C; END T; END A."),
            Limits {
                max_nesting: 1,
                ..Limits::default()
            },
        );
        assert!(d.limited);
        let (_, doc, d) = parse_text(&format!("{HEADER}END A."), Limits::default());
        assert!(doc.is_some());
        assert!(!d.limited);
    }
}

#[cfg(test)]
mod unsupported_regressions {
    use crate::{AnalysisStatus, CompileRequest, Limits, SourceInput, analyze};
    #[test]
    fn known_valid_extensions_are_incomplete_without_claiming_support() {
        for language in ["2.3", "2.4"] {
            for body in [
                "DOMAIN Angle=0..360 CIRCULAR;",
                "DOMAIN Length=0..100 [INTERLIS.m];",
                "STRUCTURE S=value:0..100 [INTERLIS.m]; END S;",
                "DOMAIN Code (FINAL)=TEXT*4;",
                "DOMAIN Code (ABSTRACT)=TEXT*4;",
                "DOMAIN Angle=0..360 CLOCKWISE;",
                "DOMAIN Angle=0..360 COUNTERCLOCKWISE;",
                r#"DOMAIN Code=TEXT*4 CONSTRAINTS OnlyX: THIS == "X";"#,
            ] {
                if language == "2.3" && body.contains("CONSTRAINTS") {
                    continue;
                }
                let text = format!(
                    "INTERLIS {language}; MODEL A AT \"https://example.invalid\" VERSION \"1\" = {body} END A."
                );
                let report = analyze(CompileRequest {
                    sources: vec![SourceInput {
                        name: "a".into(),
                        text,
                    }],
                    limits: Limits::default(),
                })
                .unwrap();
                assert_eq!(
                    report.status(),
                    AnalysisStatus::Incomplete,
                    "{body}: {:?}",
                    report.diagnostics()
                );
                assert!(
                    report
                        .diagnostics()
                        .iter()
                        .any(|d| d.code == "ILI-UNSUPPORTED" && d.primary.is_some())
                );
                assert!(
                    !report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"),
                    "{body}"
                );
            }
        }
    }
    #[test]
    fn supported_syntax_errors_are_not_promoted_to_unsupported() {
        for body in [
            "DOMAIN A=TEXT*-5;",
            "DOMAIN A=TEXT*+5;",
            "DOMAIN A=TEXT*1.5;",
            "DOMAIN A=0..;",
            "DOMAIN A=TEXT*4 garbage;",
        ] {
            let text = format!(
                "INTERLIS 2.4; MODEL A AT \"https://example.invalid\" VERSION \"1\" = {body} END A."
            );
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "a".into(),
                    text,
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(report.status(), AnalysisStatus::Rejected, "{body}");
            assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
            assert!(
                !report
                    .diagnostics()
                    .iter()
                    .any(|d| d.code == "ILI-UNSUPPORTED")
            );
        }
    }
}

#[cfg(test)]
mod continuation_regressions {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    fn run(text: String) -> crate::AnalysisReport {
        analyze(CompileRequest {
            sources: vec![SourceInput {
                name: "continuation".into(),
                text,
            }],
            limits: Limits::default(),
        })
        .unwrap()
    }
    #[test]
    fn unsupported_at_each_model_import_and_attribute_transition() {
        for language in ["2.3", "2.4"] {
            let head = format!("INTERLIS {language};");
            let models = "MODEL Base AT \"https://example.invalid\" VERSION \"1\" = DOMAIN Label=TEXT*4; END Base. MODEL Other AT \"https://example.invalid\" VERSION \"1\" = DOMAIN Code=TEXT*6; END Other.";
            let data = "MODEL Data AT \"https://example.invalid\" VERSION \"1\" =";
            for (source, marker) in [
                (
                    format!("{head} {models} {data} IMPORTS UNQUALIFIED Base; END Data."),
                    "UNQUALIFIED",
                ),
                (
                    format!("{head} {models} {data} IMPORTS Base, UNQUALIFIED Other; END Data."),
                    "UNQUALIFIED",
                ),
                (
                    format!(
                        "{head} {models} {data} IMPORTS Base; IMPORTS UNQUALIFIED Other; END Data."
                    ),
                    "UNQUALIFIED",
                ),
                (
                    format!(
                        "{head} {data} TOPIC T=CLASS C=value:TEXT*4 := \"X\"; END C; END T; END Data."
                    ),
                    ":=",
                ),
            ] {
                let expected = source.find(marker).unwrap();
                let report = run(source);
                assert_eq!(report.status(), AnalysisStatus::Incomplete);
                assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
                let d = report
                    .diagnostics()
                    .iter()
                    .find(|d| {
                        d.code == "ILI-UNSUPPORTED"
                            && d.message != "Semantic analysis is not implemented yet"
                    })
                    .unwrap();
                let location = report
                    .sources()
                    .location(d.primary.as_ref().unwrap())
                    .unwrap();
                assert_eq!(location.start_byte as usize, expected);
                assert_eq!(location.end_byte as usize, expected + marker.len());
            }
        }
        let valid = "INTERLIS 2.4; MODEL A (en) NOINCREMENTALTRANSFER AT \"https://example.invalid\" VERSION \"1\" = END A.";
        let report = run(valid.into());
        assert_eq!(report.status(), AnalysisStatus::Incomplete);
        assert!(
            report
                .diagnostics()
                .iter()
                .any(|d| d.code == "ILI-UNSUPPORTED")
        );
        assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        for invalid in [
            valid.replace("2.4", "2.3"),
            valid.replace("NOINCREMENTALTRANSFER", "NOINCREMENTALTRANSFERX"),
            "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = IMPORTS Base UNQUALIFIED Other; END A."
                .into(),
        ] {
            let report = run(invalid);
            assert_eq!(report.status(), AnalysisStatus::Rejected);
            assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
    }
    #[test]
    fn repeated_simple_import_clauses_retain_every_name_in_order() {
        let source = "INTERLIS 2.4; MODEL A AT \"https://example.invalid\" VERSION \"1\" = END A. MODEL B AT \"https://example.invalid\" VERSION \"1\" = IMPORTS A; IMPORTS INTERLIS; END B.";
        let sources = SourceSet::new(vec![SourceInput {
            name: "a".into(),
            text: source.into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut remaining = limits.max_tokens;
        let doc = parse(&sources, 0, &limits, &mut remaining, &mut diagnostics).unwrap();
        assert!(diagnostics.entries.is_empty());
        assert_eq!(
            doc.models[1]
                .imports
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>(),
            ["A", "INTERLIS"]
        );
    }
}

#[cfg(test)]
mod definite_syntax_regressions {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    #[test]
    fn missing_supported_delimiters_are_rejected_instead_of_hidden() {
        for version in ["2.3", "2.4"] {
            let header =
                format!("INTERLIS {version}; MODEL M AT \"https://example.invalid\" VERSION \"1\"");
            for text in [
                format!("{header} END M."),
                format!("{header} = TOPIC T END T; END M."),
                format!("{header} = TOPIC T = CLASS C END C; END T; END M."),
                format!("{header} = STRUCTURE S END S; END M."),
                format!("{header} = DOMAIN Base=BOOLEAN; D EXTENDS Base = ; END M."),
                format!("{header} = DOMAIN Base=BOOLEAN; D EXTENDS Base = MANDATORY END M."),
                format!("{header} = END M. END"),
            ] {
                let report = analyze(CompileRequest {
                    sources: vec![SourceInput {
                        name: "bad".into(),
                        text: text.clone(),
                    }],
                    limits: Limits::default(),
                })
                .unwrap();
                assert_eq!(report.status(), AnalysisStatus::Rejected, "{text}");
                assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
                assert!(
                    !report
                        .diagnostics()
                        .iter()
                        .any(|d| d.code == "ILI-UNSUPPORTED")
                );
            }
        }
    }
    #[test]
    fn empty_domain_sections_and_header_only_sources_are_not_invented_errors() {
        for version in ["2.3", "2.4"] {
            for text in [
                format!("INTERLIS {version};"),
                format!(
                    "INTERLIS {version}; MODEL M AT \"https://example.invalid\" VERSION \"1\" = DOMAIN END M."
                ),
                format!(
                    "INTERLIS {version}; MODEL M AT \"https://example.invalid\" VERSION \"1\" = DOMAIN STRUCTURE S = END S; END M."
                ),
            ] {
                let sources = SourceSet::new(vec![SourceInput {
                    name: "a".into(),
                    text: text.clone(),
                }])
                .unwrap();
                let limits = Limits::default();
                let mut diagnostics = Diagnostics::new(&limits);
                let mut remaining = limits.max_tokens;
                let doc = parse(&sources, 0, &limits, &mut remaining, &mut diagnostics).unwrap();
                assert!(diagnostics.entries.is_empty(), "{text}");
                if text.ends_with(';') {
                    assert!(doc.models.is_empty());
                } else if text.contains("STRUCTURE") {
                    assert_eq!(doc.models[0].declarations.len(), 1);
                } else {
                    assert!(doc.models[0].declarations.is_empty());
                }
                let report = analyze(CompileRequest {
                    sources: vec![SourceInput {
                        name: "a".into(),
                        text,
                    }],
                    limits,
                })
                .unwrap();
                assert_eq!(
                    report.status(),
                    if doc.models.is_empty() {
                        AnalysisStatus::Rejected
                    } else {
                        AnalysisStatus::CheckedSubset
                    }
                );
                if doc.models.is_empty() {
                    assert!(
                        report
                            .diagnostics()
                            .iter()
                            .any(|d| d.code == "ILI-MODEL-MISSING")
                    );
                }
                assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
            }
        }
    }
}

#[cfg(test)]
mod stray_separator_tests {
    use crate::{AnalysisStatus, CompileRequest, Limits, SourceInput, analyze};
    #[test]
    fn closing_symbols_do_not_create_a_missing_feature_in_supported_bodies() {
        for version in ["2.3", "2.4"] {
            for body in [
                ";",
                "DOMAIN ;",
                "TOPIC T=; END T;",
                "STRUCTURE S=; END S;",
                "TOPIC T=CLASS C=; END C; END T;",
                ")",
                "]",
                "}",
                ",",
                ".",
                ":=",
            ] {
                let text = format!(
                    "INTERLIS {version}; MODEL M AT \"https://example.invalid\" VERSION \"1\" = {body} END M."
                );
                let report = analyze(CompileRequest {
                    sources: vec![SourceInput {
                        name: "a".into(),
                        text,
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
                assert!(report.graph().is_none());
                assert!(report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
                assert!(
                    !report
                        .diagnostics()
                        .iter()
                        .any(|d| d.code == "ILI-UNSUPPORTED")
                );
            }
        }
    }
}
