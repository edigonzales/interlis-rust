//! Private syntax values. No parser-library type crosses this module's boundary.
use crate::{
    Limits, SourceSet, Span,
    diagnostic::Diagnostics,
    keywords,
    lexer::{self, Kind, Token},
    limits::SyntaxBudget,
};

mod expressions;
mod structure;
mod types;
mod views;
pub(crate) use crate::model::LanguageVersion as Language;
pub(crate) use expressions::*;
pub(crate) use structure::*;
pub(crate) use types::*;
pub(crate) use views::*;
fn language_version(text: &str) -> Option<Language> {
    // Version-Dec is a number, not a model-version string. Recognize its exact
    // value without floats, bigint work, allocation or altering source spelling.
    let unsigned = text.strip_prefix('+').unwrap_or(text);
    if unsigned.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = if let Some(at) = unsigned.find(['e', 'E']) {
        (&unsigned[..at], unsigned[at + 1..].parse::<i32>().ok()?)
    } else {
        (unsigned, 0)
    };
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = integer
        .bytes()
        .chain(fraction.bytes())
        .skip_while(|b| *b == b'0');
    if digits.next() != Some(b'2') {
        return None;
    }
    let language = match digits.next()? {
        b'3' => Language::V23,
        b'4' => Language::V24,
        _ => return None,
    };
    let mut trailing = 0_i32;
    for digit in digits {
        if digit != b'0' {
            return None;
        }
        trailing = trailing.checked_add(1)?;
    }
    let scale = i32::try_from(fraction.len()).ok()?.checked_sub(exponent)?;
    (scale == trailing.checked_add(1)?).then_some(language)
}

// The same lexer stops after a complete foreign header. Its remaining language
// is unknown, so applying 2.3/2.4 lexical judgments to that remainder is unsound.
// No alternate lexer or source preprocessing is used.
pub(crate) fn foreign_header_complete(tokens: &[Token], source: &str) -> bool {
    let text = |t: &Token| &source[t.range.clone()];
    match tokens {
        [start, number, end] => {
            text(start) == "INTERLIS"
                && text(end) == ";"
                && number.kind == Kind::Number
                && language_version(text(number)).is_none()
        }
        [start, sign, number, end] => {
            text(start) == "INTERLIS"
                && text(end) == ";"
                && matches!(text(sign), "+" | "-")
                && sign.range.end == number.range.start
                && number.kind == Kind::Number
                && language_version(&source[sign.range.start..number.range.end]).is_none()
        }
        _ => false,
    }
}
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
    #[allow(dead_code)] // Document-local expression storage; no expression evaluation.
    pub expressions: Vec<Expression>,
    #[allow(dead_code)] // Whole-document range, including header-only sources.
    pub span: Span,
    // Preserved original comment ranges, without semantic interpretation.
    #[allow(dead_code)]
    pub trivia: Vec<lexer::Trivia>,
}
#[derive(Debug)]
pub(crate) struct ModelDecl {
    pub name: Name,
    pub language_tag: Option<Name>,
    pub issuer: Located<String>,
    pub version: Located<String>,
    pub kind: ModelKind,
    pub contracted: Option<Span>,
    pub no_incremental_transfer: Option<Span>,
    pub explanation: Option<Located<String>>,
    pub translation: Option<TranslationSyntax>,
    pub charset: Option<Located<String>>,
    pub xmlns: Option<Located<String>>,
    pub imports: Vec<ImportSyntax>,
    pub declarations: Vec<Declaration>,
    pub end_name: Name,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) enum Declaration {
    Topic {
        options: TopicOptions,
        name: Name,
        declarations: Vec<Declaration>,
        end_name: Name,
        span: Span,
    },
    Container {
        properties: Vec<Located<Property>>,
        base: Option<QualifiedName>,
        oid: OidRule,
        #[allow(dead_code)] // Preserved section spelling; semantically inert.
        attribute_section: Option<Span>,
        parameter_section: Option<Span>,
        parameters: Vec<ParameterDecl>,
        constraints: Vec<ConstraintSyntax>,
        kind: ContainerKind,
        name: Name,
        attributes: Vec<Declaration>,
        end_name: Name,
        span: Span,
    },
    Domain {
        name: Name,
        properties: Vec<Located<Property>>,
        base: Option<QualifiedName>,
        ty: Option<TypeSyntax>,
        mandatory: bool,
        constraints: Vec<DomainConstraint>,
        span: Span,
    },
    Unit(UnitDecl),
    LineForm(LineFormDecl),
    Function(FunctionDecl),
    Constraints(ConstraintsDecl),
    Association(AssociationDecl),
    Context(ContextDecl),
    View(ViewDecl),
    Graphic(GraphicDecl),
    MetadataBasket(MetadataBasketDecl),
    RuntimeParameter {
        name: Name,
        type_use: TypeUseSyntax,
        span: Span,
    },
    Attribute {
        properties: Vec<Located<Property>>,
        subdivision: Option<Subdivision>,
        name: Name,
        type_use: TypeUseSyntax,
        derivations: Vec<ExprId>,
        span: Span,
    },
}
impl Declaration {
    pub(crate) fn span(&self) -> &Span {
        match self {
            Self::Topic { span, .. }
            | Self::Container { span, .. }
            | Self::Domain { span, .. }
            | Self::Attribute { span, .. }
            | Self::RuntimeParameter { span, .. } => span,
            Self::Unit(value) => &value.span,
            Self::LineForm(value) => &value.span,
            Self::Function(value) => &value.span,
            Self::Constraints(value) => &value.span,
            Self::Association(value) => &value.span,
            Self::Context(value) => &value.span,
            Self::View(value) => &value.span,
            Self::Graphic(value) => &value.span,
            Self::MetadataBasket(value) => &value.span,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContainerKind {
    Class,
    Structure,
}
#[derive(Debug)]
pub(crate) struct TypeUseSyntax {
    pub mandatory: bool,
    pub ty: Option<TypeSyntax>,
    #[allow(dead_code)] // Original range of this type use, including MANDATORY.
    pub span: Span,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReferenceContext {
    Ordinary,
    Domain,
    Class,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Model,
    Topic,
    Container,
    View,
    Graphic,
}
#[derive(Clone, Copy)]
enum IssueKind {
    Syntax,
    UnsupportedVersion,
    Limit,
    Lexical,
}
struct Issue {
    primary: Option<Span>,
    kind: IssueKind,
    message: &'static str,
}
type Parsed<T> = Result<T, Issue>;
struct Parser<'a> {
    sources: &'a SourceSet,
    file: usize,
    tokens: Vec<Token>,
    trivia: Vec<lexer::Trivia>,
    expressions: Vec<Expression>,
    budget: &'a mut SyntaxBudget,
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
    fn step(&mut self) -> Parsed<()> {
        if self.diagnostics.limited {
            return Err(Self::limited());
        }
        if !self.budget.work(1) {
            self.diagnostics
                .limit("Parser work budget exceeded", Some(self.current_span()));
            return Err(Self::limited());
        }
        Ok(())
    }
    fn node(&mut self) -> Parsed<()> {
        if !self.budget.node() {
            self.diagnostics.limit(
                "Syntax node/work budget exceeded",
                Some(self.current_span()),
            );
            return Err(Self::limited());
        }
        Ok(())
    }
    fn limited() -> Issue {
        Issue {
            primary: None,
            kind: IssueKind::Limit,
            message: "Frontend budget exceeded",
        }
    }
    fn lexical() -> Issue {
        Issue {
            primary: None,
            kind: IssueKind::Lexical,
            message: "Lexical error already diagnosed",
        }
    }
    fn take(&mut self, text: &str) -> bool {
        if !self.diagnostics.limited && self.is(text) && self.step().is_ok() {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expected(message: &'static str) -> Issue {
        Issue {
            primary: None,
            kind: IssueKind::Syntax,
            message,
        }
    }
    fn expect(&mut self, text: &str) -> Parsed<()> {
        if self.diagnostics.limited {
            return Err(Self::limited());
        }
        if self.take(text) {
            Ok(())
        } else {
            Err(Self::expected("Missing or unexpected syntax token"))
        }
    }
    fn report(&mut self, issue: Issue) {
        if self.diagnostics.limited {
            return;
        }
        let span = Some(issue.primary.unwrap_or_else(|| self.current_span()));
        match issue.kind {
            IssueKind::Syntax => self.diagnostics.syntax_error(issue.message, span),
            IssueKind::UnsupportedVersion => {
                self.diagnostics.unsupported_version(issue.message, span)
            }
            IssueKind::Limit | IssueKind::Lexical => {} // The early failing charge already emitted its limit.
        }
    }
    fn name(&mut self, builtin: bool) -> Parsed<Name> {
        self.step()?;
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::Invalid)
        ) {
            return Err(Self::lexical());
        }
        let text = self.text();
        if !matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Kind::Word))
            || (keywords::reserved(text, self.language) && !(builtin && text == "INTERLIS"))
        {
            return Err(Self::expected("Expected a non-reserved identifier"));
        }
        self.node()?;
        let text = self.text();
        let name = Located {
            value: text.into(),
            span: self.current_span(),
        };
        self.step()?;
        self.at += 1;
        Ok(name)
    }
    fn quoted(&mut self) -> Parsed<Located<String>> {
        self.step()?;
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::Invalid)
        ) {
            return Err(Self::lexical());
        }
        if !matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::String(_))
        ) {
            return Err(Self::expected("Expected a quoted string"));
        }
        self.node()?;
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
        self.step()?;
        self.at += 1;
        Ok(result)
    }
    fn qualified_name(&mut self) -> Parsed<QualifiedName> {
        self.qualified_name_in(ReferenceContext::Domain, 3)
    }
    fn qualified_name_in(
        &mut self,
        context: ReferenceContext,
        maximum: usize,
    ) -> Parsed<QualifiedName> {
        self.step()?;
        let start = self.start();
        let class = |text: &str| matches!(text, "SIGN" | "METAOBJECT" | "REFSYSTEM");
        let domain = |text: &str| {
            matches!(
                text,
                "BOOLEAN" | "NAME" | "URI" | "HALIGNMENT" | "VALIGNMENT"
            )
        };
        let bare_class = context == ReferenceContext::Class && class(self.text());
        let mut segments = if bare_class {
            vec![self.predefined_name()?]
        } else {
            vec![self.name(true)?]
        };
        if segments[0].value == "INTERLIS" && !self.is(".") {
            return Err(Issue {
                kind: IssueKind::Syntax,
                primary: Some(segments[0].span.clone()),
                message: "INTERLIS must qualify a member in a reference",
            });
        }
        if bare_class && self.is(".") {
            return Err(Self::expected(
                "A predefined class cannot qualify another name",
            ));
        }
        while self.take(".") {
            self.step()?;
            if segments.len() >= maximum {
                return Err(Self::expected("Too many name segments in this reference"));
            }
            if segments.len() == 1
                && segments[0].value == "INTERLIS"
                && keywords::reserved(self.text(), self.language)
            {
                let allowed = match context {
                    ReferenceContext::Ordinary => false,
                    ReferenceContext::Domain => domain(self.text()),
                    ReferenceContext::Class => class(self.text()),
                };
                if !allowed {
                    return Err(Self::expected(
                        "Reserved predefined member is not permitted in this reference context",
                    ));
                }
                segments.push(self.predefined_name()?);
                if self.is(".") {
                    return Err(Self::expected(
                        "A predefined member cannot qualify another name",
                    ));
                }
            } else {
                segments.push(self.name(false)?);
            }
        }
        if self.is("..") {
            return Err(Self::expected(
                "Expected a single dot between qualified name segments",
            ));
        }
        self.node()?;
        Ok(QualifiedName {
            segments,
            span: self.span(start, self.end()),
        })
    }
    fn predefined_name(&mut self) -> Parsed<Name> {
        self.node()?;
        let name = Located {
            value: self.text().into(),
            span: self.current_span(),
        };
        self.step()?;
        self.at += 1;
        Ok(name)
    }
    fn number_range(&mut self) -> Parsed<std::ops::Range<usize>> {
        self.step()?;
        let start = self.start();
        let signed = self.take("-") || self.take("+");
        let Some(token) = self.tokens.get(self.at) else {
            return Err(Self::expected("Expected a number"));
        };
        if token.kind == Kind::Invalid {
            return Err(Self::lexical());
        }
        if token.kind != Kind::Number || signed && self.end() != token.range.start {
            return Err(Self::expected("Expected a number with adjacent sign"));
        }
        self.step()?;
        self.at += 1;
        Ok(start..self.end())
    }
    fn number(&mut self) -> Parsed<Located<String>> {
        let range = self.number_range()?;
        self.node()?;
        Ok(Located {
            value: self.source()[range.clone()].into(),
            span: self.span(range.start, range.end),
        })
    }
    fn recovery_start(&self, context: Context, domain_section: bool) -> bool {
        let word = matches!(self.tokens.get(self.at).map(|t| &t.kind), Some(Kind::Word));
        let next = self
            .tokens
            .get(self.at + 1)
            .map_or("", |t| &self.source()[t.range.clone()]);
        if word && !keywords::reserved(self.text(), self.language) {
            if domain_section && matches!(next, "=" | "EXTENDS" | "(") {
                return true;
            }
            if matches!(
                context,
                Context::Container | Context::View | Context::Graphic
            ) && matches!(next, ":" | "(")
                || context == Context::View && next == ":="
                || context == Context::Graphic && next == "OF"
            {
                return true;
            }
        }
        match context {
            Context::Model => {
                self.is("VIEW") && self.next_is("TOPIC")
                    || matches!(
                        self.text(),
                        "DOMAIN"
                            | "UNIT"
                            | "FUNCTION"
                            | "LINE"
                            | "PARAMETER"
                            | "CONTEXT"
                            | "CLASS"
                            | "STRUCTURE"
                            | "TOPIC"
                            | "SIGN"
                            | "REFSYSTEM"
                    )
            }
            Context::Topic => matches!(
                self.text(),
                "DOMAIN"
                    | "UNIT"
                    | "FUNCTION"
                    | "CLASS"
                    | "STRUCTURE"
                    | "ASSOCIATION"
                    | "CONSTRAINTS"
                    | "VIEW"
                    | "GRAPHIC"
                    | "SIGN"
                    | "REFSYSTEM"
            ),
            Context::Container => matches!(
                self.text(),
                "CONTINUOUS"
                    | "SUBDIVISION"
                    | "ATTRIBUTE"
                    | "PARAMETER"
                    | "CARDINALITY"
                    | "MANDATORY"
                    | "CONSTRAINT"
                    | "EXISTENCE"
                    | "UNIQUE"
                    | "SET"
            ),
            Context::View => {
                matches!(self.text(), "ALL" | "CONTINUOUS" | "SUBDIVISION")
                    || self.constraint_start()
            }
            Context::Graphic => false,
        }
    }
    fn recover(&mut self, context: Context, domain_section: bool) {
        let mut nesting = 0_u32;
        while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
            if self.step().is_err() {
                return;
            }
            if nesting == 0 && self.recovery_start(context, domain_section) {
                return;
            }
            if self.is(";") && nesting == 0 {
                self.take(";");
                return;
            }
            match self.text() {
                "(" | "[" | "{" => nesting += 1,
                ")" | "]" | "}" => nesting = nesting.saturating_sub(1),
                _ => {}
            }
            self.at += 1;
        }
    }
    fn declarations(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        self.step()?;
        let mut declarations = vec![];
        while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
            self.step()?;
            let before = self.at;
            match self.declaration(context) {
                Ok(mut items) => declarations.append(&mut items),
                Err(issue) => {
                    self.report(issue);
                    self.recover(context, false);
                }
            }
            if self.at == before {
                self.step()?;
                self.at += 1;
            }
        }
        Ok(declarations)
    }
    fn declaration(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        self.step()?;
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::Invalid)
        ) {
            return Err(Self::lexical());
        }
        if self
            .tokens
            .get(self.at)
            .is_some_and(|token| token.kind == Kind::Symbol)
        {
            return Err(Self::expected("Unexpected symbol at declaration start"));
        }

        if context == Context::Container {
            return Ok(vec![self.attribute()?]);
        }
        if self.is("FUNCTION") {
            return Ok(vec![Declaration::Function(self.function()?)]);
        }
        if self.is("CONSTRAINTS") {
            if context != Context::Topic {
                return Err(Self::expected("CONSTRAINTS OF requires topic scope"));
            }
            return Ok(vec![Declaration::Constraints(
                self.constraints_declaration()?,
            )]);
        }
        if self.is("ASSOCIATION") {
            if context != Context::Topic {
                return Err(Self::expected("ASSOCIATION requires a topic"));
            }
            return Ok(vec![Declaration::Association(self.association()?)]);
        }
        if self.is("CONTEXT") {
            if context != Context::Model || self.language != Language::V24 {
                return Err(Self::expected("CONTEXT requires an INTERLIS 2.4 model"));
            }
            return self.contexts();
        }
        if self.take("PARAMETER") {
            if context != Context::Model {
                return Err(Self::expected("Runtime PARAMETER requires a model"));
            }
            let mut items = vec![];
            while self.unreserved_name() {
                let start = self.start();
                let name = self.name(false)?;
                self.expect(":")?;
                let type_use = self.type_use()?;
                self.expect(";")?;
                self.node()?;
                items.push(Declaration::RuntimeParameter {
                    name,
                    type_use,
                    span: self.span(start, self.end()),
                });
            }
            return Ok(items);
        }
        if self.take("DOMAIN") {
            return self.domain_section(context);
        }
        if self.take("UNIT") {
            return self.unit_section(context);
        }
        if self.is("LINE") {
            if context != Context::Model {
                return Err(Self::expected("LINE FORM requires model scope"));
            }
            return self.line_form_section();
        }
        if self.is("TOPIC") || self.is("VIEW") && self.next_is("TOPIC") {
            if context != Context::Model {
                return Err(Self::expected("TOPIC may only occur in a model"));
            }
            return Ok(vec![self.topic()?]);
        }
        if self.is("CLASS") || self.is("STRUCTURE") {
            return Ok(vec![self.container()?]);
        }
        if matches!(self.text(), "SIGN" | "REFSYSTEM") {
            return Ok(vec![Declaration::MetadataBasket(self.metadata_basket()?)]);
        }
        if context == Context::Topic {
            if self.is("VIEW") {
                return Ok(vec![Declaration::View(self.view()?)]);
            }
            if self.is("GRAPHIC") {
                return Ok(vec![Declaration::Graphic(self.graphic()?)]);
            }
        }
        if self.unreserved_name() {
            return Err(Self::expected("Expected a declaration keyword"));
        }
        Err(Self::expected(
            "Declaration is not permitted at this position",
        ))
    }
    fn document(&mut self) -> Parsed<Document> {
        self.step()?;
        let start = self.start();
        if !self.take("INTERLIS") {
            return Err(Self::expected("Expected INTERLIS language header"));
        }
        let version = self.number_range()?;
        let language = language_version(&self.source()[version]);
        self.expect(";")?;
        let Some(language) = language else {
            return Err(Issue {
                primary: Some(self.span(start, self.end())),
                kind: IssueKind::UnsupportedVersion,
                message: "Only INTERLIS 2.3 and 2.4 are supported",
            });
        };
        self.language = language;
        let mut models = vec![];
        while self.at < self.tokens.len() && !self.diagnostics.limited {
            if !matches!(
                self.text(),
                "MODEL" | "CONTRACTED" | "TYPE" | "REFSYSTEM" | "SYMBOLOGY"
            ) {
                return Err(Self::expected(
                    "Unexpected token after complete model; EOF required",
                ));
            }
            self.step()?;
            models.push(self.model()?);
        }
        self.node()?;
        Ok(Document {
            language: self.language,
            models,
            expressions: std::mem::take(&mut self.expressions),
            span: self.span(start, self.source().len()),
            trivia: std::mem::take(&mut self.trivia),
        })
    }
}

pub(crate) fn parse(
    sources: &SourceSet,
    file: usize,
    limits: &Limits,
    budget: &mut SyntaxBudget,
    diagnostics: &mut Diagnostics,
) -> Option<Document> {
    let lexed = lexer::lex(sources, file, limits, budget, diagnostics);
    if diagnostics.limited {
        return None;
    }
    let mut parser = Parser {
        sources,
        file,
        tokens: lexed.tokens,
        trivia: lexed.trivia,
        expressions: vec![],
        budget,
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
        let mut remaining = SyntaxBudget::new(&limits);
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
            ty:
                Some(TypeSyntax::Text(TextSyntax {
                    max_length: Some(max_length),
                    ..
                })),
            ..
        } = &a.declarations[0]
        else {
            panic!()
        };
        assert_eq!(max_length.value, "40");
        assert_eq!(original(&sources, &max_length.span), "40");
        let Declaration::Domain {
            ty:
                Some(TypeSyntax::Numeric(NumericSyntax {
                    bounds: Some(NumericBounds { lower, upper }),
                    ..
                })),
            ..
        } = &a.declarations[1]
        else {
            panic!()
        };
        assert_eq!(lower.value, "-1");
        assert_eq!(original(&sources, &lower.span), "-1");
        assert_eq!(upper.value, "100");
        let Declaration::Domain {
            ty: None,
            base: Some(base),
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
        let Declaration::Attribute { type_use, span, .. } = &attributes[0] else {
            panic!()
        };
        assert!(type_use.mandatory);
        assert_eq!(original(&sources, span), "label:MANDATORY Label;");
        let Declaration::Attribute {
            type_use:
                TypeUseSyntax {
                    ty: Some(TypeSyntax::Named(q)),
                    ..
                },
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
                .map(|n| n.name.value.as_str())
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
            text: "1..2 -1.0 a-1 0.10e-2".into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut d = Diagnostics::new(&limits);
        let mut remaining = SyntaxBudget::new(&limits);
        let t = lexer::lex(&sources, 0, &limits, &mut remaining, &mut d).tokens;
        assert_eq!(
            t.iter()
                .map(|t| &sources.files()[0].text()[t.range.clone()])
                .collect::<Vec<_>>(),
            ["1", "..", "2", "-", "1.0", "a", "-", "1", "0.10e-2"]
        );
        let text = format!("{HEADER}DOMAIN N=-0.1200E3..0.1201E3; END A.");
        let (_, doc, d) = parse_text(&text, limits);
        assert!(d.entries.is_empty());
        let Declaration::Domain {
            ty:
                Some(TypeSyntax::Numeric(NumericSyntax {
                    bounds: Some(NumericBounds { lower, upper }),
                    ..
                })),
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
    fn direct_domain_alias_is_invalid_syntax() {
        let (_, _, d) = parse_text(
            &format!("{HEADER}DOMAIN Alias=Base; END A."),
            Limits::default(),
        );
        assert!(d.rejected);
        assert!(!d.incomplete);
    }
    #[test]
    fn actual_version_controls_complete_reserved_tables() {
        for (version, word, valid) in [
            ("2.3", "XMLNS", true),
            ("2.4", "XMLNS", false),
            ("2.3", "TIMEOFDAY", true),
            ("2.4", "TIMEOFDAY", false),
            ("2.3", "DATETIME", true),
            ("2.4", "DATETIME", false),
            ("2.3", "DATE", false),
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
    fn unqualified_import_and_unsupported_attribute_transitions() {
        for language in ["2.3", "2.4"] {
            let head = format!("INTERLIS {language};");
            let models = "MODEL Base AT \"https://example.invalid\" VERSION \"1\" = DOMAIN Label=TEXT*4; END Base. MODEL Other AT \"https://example.invalid\" VERSION \"1\" = DOMAIN Code=TEXT*6; END Other.";
            let data = "MODEL Data AT \"https://example.invalid\" VERSION \"1\" =";
            for (imports, name, target) in [
                ("IMPORTS UNQUALIFIED Base;", "Label", "Base.Label"),
                ("IMPORTS Base, UNQUALIFIED Other;", "Code", "Other.Code"),
                (
                    "IMPORTS Base; IMPORTS UNQUALIFIED Other;",
                    "Code",
                    "Other.Code",
                ),
            ] {
                let report = run(format!(
                    "{head} {models} {data} {imports} STRUCTURE S=value:{name}; END S; END Data."
                ));
                assert_eq!(report.status(), AnalysisStatus::CheckedSubset);
                let reference = report
                    .graph()
                    .unwrap()
                    .declaration("Data.S.value")
                    .unwrap()
                    .type_reference()
                    .unwrap();
                assert_eq!(reference.declared_name(), name);
                assert_eq!(reference.target_name(), target);
            }
            let marker = "value:TEXT*4 := \"X\";";
            let source = format!("{head} {data} TOPIC T=CLASS C={marker} END C; END T; END Data.");
            let expected = source.find(marker).unwrap();
            let report = run(source);
            assert_eq!(report.status(), AnalysisStatus::Incomplete);
            let d = &report.diagnostics()[0];
            assert_eq!(d.code, "ILI-UNSUPPORTED");
            let location = report
                .sources()
                .location(d.primary.as_ref().unwrap())
                .unwrap();
            assert_eq!(location.start_byte as usize, expected);
            assert_eq!(location.end_byte as usize, expected + marker.len());
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
        let mut remaining = SyntaxBudget::new(&limits);
        let doc = parse(&sources, 0, &limits, &mut remaining, &mut diagnostics).unwrap();
        assert!(diagnostics.entries.is_empty());
        assert_eq!(
            doc.models[1]
                .imports
                .iter()
                .map(|n| n.name.value.as_str())
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
                let mut remaining = SyntaxBudget::new(&limits);
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
                assert_eq!(report.status(), AnalysisStatus::CheckedSubset);
                assert_eq!(report.graph().unwrap().models().len(), doc.models.len());
                assert!(!report.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
            }
        }
    }
}

#[cfg(test)]
mod stray_separator_tests {
    use crate::{AnalysisStatus, CompileRequest, Limits, SourceInput, analyze};
    #[test]
    fn symbols_cannot_start_declarations_in_supported_bodies() {
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
                "=",
                "==",
                "(",
                "[",
                "{",
                "--",
                "->",
                "+",
                "-",
                "/",
                ":",
                "*",
                "..",
                "<",
                ">",
                "<=",
                ">=",
                "!=",
                "<>",
                "TOPIC T=( END T;",
                "STRUCTURE S== END S;",
                "TOPIC T=CLASS C=-> END C; END T;",
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

#[cfg(test)]
mod frontend_contract_tests {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    const SIMPLE: &str = "INTERLIS 2.4; MODEL A AT \"a\" VERSION \"1\" = END A.";
    fn run(text: &str, limits: Limits) -> (Option<Document>, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "a".into(),
            text: text.into(),
        }])
        .unwrap();
        let mut d = Diagnostics::new(&limits);
        let mut budget = SyntaxBudget::new(&limits);
        let doc = parse(&sources, 0, &limits, &mut budget, &mut d);
        (doc, d)
    }
    #[test]
    fn malformed_headers_are_syntax_errors_and_other_versions_are_not() {
        for text in [
            "",
            "MODEL A",
            "INTERLIS;",
            "INTERLIS 2.5",
            "INTERLIS 2.3.4;",
            "INTERLIS 1E2;",
        ] {
            let (_, d) = run(text, Limits::default());
            assert!(d.syntax_invalid, "{text}");
            assert!(!d.unsupported_version, "{text}");
        }
        for text in ["INTERLIS 1.0;", "INTERLIS 2.2;", "INTERLIS 2.5;"] {
            let (doc, d) = run(text, Limits::default());
            assert!(doc.is_none());
            assert!(d.unsupported_version);
            assert!(!d.syntax_invalid);
        }
        for version in ["2.3", "2.4"] {
            let (doc, d) = run(&format!("INTERLIS {version};"), Limits::default());
            assert!(d.entries.is_empty());
            assert!(doc.unwrap().models.is_empty());
        }
    }
    #[test]
    fn foreign_remainder_is_not_judged_as_supported_language() {
        for version in [
            "1.0",
            "2.2",
            "2.5",
            "+2.5",
            "-2.5",
            "-2.3",
            "2.300000000000000000000000001",
        ] {
            let (_, d) = run(
                &format!("INTERLIS {version}; ? 😀 \"open /*"),
                Limits::default(),
            );
            assert!(d.unsupported_version);
            assert!(!d.syntax_invalid);
            assert_eq!(d.entries.len(), 1);
            assert!(!d.limited);
        }
        let (_, d) = run("INTERLIS 2.4; ? 😀", Limits::default());
        assert!(d.syntax_invalid);
        assert!(!d.unsupported_version);
    }
    #[test]
    fn comments_survive_the_parser_instance_in_the_owned_document() {
        let (doc, d) = run(
            &format!("!!@ key=\"😀\"\r\n/* outer /* inner */ */ {SIMPLE}"),
            Limits::default(),
        );
        assert!(d.entries.is_empty());
        let doc = doc.unwrap();
        assert_eq!(doc.trivia.len(), 2);
        assert_eq!(doc.trivia[0].kind, lexer::TriviaKind::MetaAttribute);
        assert_eq!(doc.trivia[1].kind, lexer::TriviaKind::BlockComment);
    }
    #[test]
    fn exact_syntax_node_budget_is_shared_by_the_entire_request() {
        // Document, model, two names and two strings: six syntax values.
        let (doc, d) = run(
            SIMPLE,
            Limits {
                max_syntax_nodes: 6,
                ..Limits::default()
            },
        );
        assert!(doc.is_some());
        assert!(d.entries.is_empty());
        let (doc, d) = run(
            SIMPLE,
            Limits {
                max_syntax_nodes: 5,
                ..Limits::default()
            },
        );
        assert!(doc.is_none());
        assert!(d.limited);
        assert!(!d.syntax_invalid);
        let input = CompileRequest {
            sources: vec![
                SourceInput {
                    name: "a".into(),
                    text: SIMPLE.into(),
                },
                SourceInput {
                    name: "b".into(),
                    text: SIMPLE.replace(" A ", " B ").replace(" A.", " B."),
                },
            ],
            limits: Limits {
                max_syntax_nodes: 11,
                ..Limits::default()
            },
        };
        let report = analyze(input).unwrap();
        assert_eq!(report.status(), AnalysisStatus::LimitExceeded);
        assert!(report.graph().is_none());
        assert!(run(SIMPLE, Limits::default()).1.entries.is_empty());
    }
    #[test]
    fn recovery_preserves_parent_end_and_later_declarations() {
        let text = "INTERLIS 2.4; MODEL A AT \"a\" VERSION \"1\" = TOPIC T = CLASS C = broken:BOOLEAN ( ; ); other:; good:BOOLEAN; END C; END T; END A.";
        let (doc, d) = run(text, Limits::default());
        assert!(d.syntax_invalid);
        assert!(!d.limited);
        assert_eq!(d.entries.len(), 2);
        let doc = doc.unwrap();
        let Declaration::Topic {
            declarations,
            end_name,
            ..
        } = &doc.models[0].declarations[0]
        else {
            panic!()
        };
        assert_eq!(end_name.value, "T");
        let Declaration::Container {
            attributes,
            end_name,
            ..
        } = &declarations[0]
        else {
            panic!()
        };
        assert_eq!(end_name.value, "C");
        assert_eq!(attributes.len(), 1);
        let Declaration::Attribute { name, .. } = &attributes[0] else {
            panic!()
        };
        assert_eq!(name.value, "good");
        let (_, d) = run(
            "INTERLIS 2.4; MODEL A AT \"a\" VERSION \"1\" = DOMAIN D=/; END A.",
            Limits::default(),
        );
        assert!(d.syntax_invalid);
        assert!(!d.incomplete);
    }
}

#[cfg(test)]
mod lexical_recovery_regressions {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    fn parse_text(text: &str) -> (Option<Document>, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "memory:///regression.ili".into(),
            text: text.into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut budget = SyntaxBudget::new(&limits);
        let mut d = Diagnostics::new(&limits);
        let doc = parse(&sources, 0, &limits, &mut budget, &mut d);
        (doc, d)
    }
    fn analyze_text(text: &str) -> crate::AnalysisReport {
        analyze(CompileRequest {
            sources: vec![SourceInput {
                name: "memory:///regression.ili".into(),
                text: text.into(),
            }],
            limits: Limits::default(),
        })
        .unwrap()
    }
    fn model(body: &str) -> String {
        format!("INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" = {body} END M.")
    }
    #[test]
    fn lexical_errors_recover_without_unsupported_surrogates() {
        for bad in [
            "?;".to_owned(),
            format!("{};", "A".repeat(256)),
            "\"unfinished\n".to_owned(),
        ] {
            for body in [
                format!("{bad} DOMAIN Good=BOOLEAN;"),
                format!("TOPIC T = {bad} DOMAIN Good=BOOLEAN; END T;"),
            ] {
                let text = model(&body);
                let (doc, d) = parse_text(&text);
                assert!(d.syntax_invalid);
                assert!(!d.incomplete);
                assert!(!d.limited);
                assert_eq!(d.entries.len(), 1, "{:?}", d.entries);
                let doc = doc.unwrap();
                let declarations = match &doc.models[0].declarations[0] {
                    Declaration::Topic { declarations, .. } => declarations.as_slice(),
                    _ => doc.models[0].declarations.as_slice(),
                };
                assert_eq!(declarations.len(), 1);
                let Declaration::Domain { name, .. } = &declarations[0] else {
                    panic!()
                };
                assert_eq!(name.value, "Good");
                let report = analyze_text(&text);
                assert_eq!(report.status(), AnalysisStatus::Rejected);
                assert!(report.graph().is_none());
                assert!(
                    !report
                        .diagnostics()
                        .iter()
                        .any(|d| d.code == "ILI-UNSUPPORTED")
                );
            }
        }
        // A genuine unsupported family elsewhere cannot erase a proven lexer error.
        let report = analyze_text(&model("UNIT metre=1[INTERLIS.m]; ?"));
        assert_eq!(report.status(), AnalysisStatus::Rejected);
        assert!(report.graph().is_none());
    }
    #[test]
    fn incomplete_explanations_are_lexical_errors_not_header_options() {
        let text = "INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" // unfinished\n = END M.";
        let report = analyze_text(text);
        assert_eq!(report.status(), AnalysisStatus::Rejected);
        assert_eq!(report.diagnostics().len(), 1);
        assert_eq!(report.diagnostics()[0].code, "ILI-SYNTAX");
        assert_eq!(report.diagnostics()[0].message, "Unfinished explanation");
        let report =
            analyze_text("INTERLIS 2.4; MODEL M AT \"x\" VERSION \"1\" // closed // = END M.");
        assert_eq!(report.status(), AnalysisStatus::Incomplete);
        assert_eq!(report.diagnostics()[0].code, "ILI-UNSUPPORTED");
    }
    #[test]
    fn missing_semicolons_do_not_consume_next_declarations_or_their_errors() {
        let (doc, d) = parse_text(&model("DOMAIN A=BOOLEAN DOMAIN B=; DOMAIN Good=BOOLEAN;"));
        assert_eq!(d.entries.len(), 2);
        assert!(d.syntax_invalid);
        assert!(!d.incomplete);
        let doc = doc.unwrap();
        assert_eq!(doc.models[0].declarations.len(), 1);
        let Declaration::Domain { name, .. } = &doc.models[0].declarations[0] else {
            panic!()
        };
        assert_eq!(name.value, "Good");
        let (doc, d) = parse_text(&model("STRUCTURE S=a:BOOLEAN b:; good:BOOLEAN; END S;"));
        assert_eq!(d.entries.len(), 2);
        assert!(!d.limited);
        let doc = doc.unwrap();
        let Declaration::Container {
            attributes,
            end_name,
            ..
        } = &doc.models[0].declarations[0]
        else {
            panic!()
        };
        assert_eq!(end_name.value, "S");
        assert_eq!(attributes.len(), 1);
        let Declaration::Attribute { name, .. } = &attributes[0] else {
            panic!()
        };
        assert_eq!(name.value, "good");
        let (doc, d) = parse_text(&model("DOMAIN A=BOOLEAN DOMAIN B=BOOLEAN;"));
        assert_eq!(d.entries.len(), 1);
        let doc = doc.unwrap();
        let Declaration::Domain { name, .. } = &doc.models[0].declarations[0] else {
            panic!()
        };
        assert_eq!(name.value, "B");
    }
    #[test]
    fn version_decimal_value_is_recognized_without_changing_original_text() {
        for (version, language) in [
            ("2.3", Language::V23),
            ("+2.3", Language::V23),
            ("02.300", Language::V23),
            ("0.23E1", Language::V23),
            ("2.4", Language::V24),
            ("+02.400", Language::V24),
            ("0.240e+1", Language::V24),
        ] {
            let text = format!("INTERLIS {version}; MODEL M AT \"x\" VERSION \"1\" = END M.");
            let (doc, d) = parse_text(&text);
            assert!(d.entries.is_empty(), "{version}: {:?}", d.entries);
            let doc = doc.unwrap();
            assert_eq!(doc.language, language);
            assert_eq!(doc.models.len(), 1);
            let report = analyze_text(&text);
            assert_eq!(report.status(), AnalysisStatus::CheckedSubset);
            assert_eq!(report.sources().files()[0].text(), text);
            assert!(parse_text(&(text + " ?")).1.syntax_invalid);
        }
    }
    #[test]
    fn definite_version_errors_and_future_type_families_remain_distinct() {
        let text = model("DOMAIN D=DATE;");
        assert_eq!(analyze_text(&text).status(), AnalysisStatus::CheckedSubset);
        let report = analyze_text(&text.replace("2.4", "2.3"));
        assert_eq!(report.status(), AnalysisStatus::Rejected);
        assert_eq!(report.diagnostics()[0].code, "ILI-SYNTAX");
        for version in ["2.3", "2.4"] {
            let text = model("DOMAIN D=MANDATORY;").replace("2.4", version);
            let report = analyze_text(&text);
            assert_eq!(report.status(), AnalysisStatus::Incomplete);
            assert_eq!(report.diagnostics()[0].code, "ILI-UNSUPPORTED");
        }
    }
}
