//! Expressions use document-local indices: flat chains neither walk nor drop a
//! recursive allocation tree. These are syntax values, without name resolution.
use super::*;
pub(crate) use crate::model::{BinaryOperator, PlausibilityDirection, UnaryOperator};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExprId(pub(crate) u32);
#[derive(Debug)]
#[allow(dead_code)] // Retained syntax; expression semantics are a later phase.
pub(crate) struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ExpressionKind {
    Literal(LiteralSyntax),
    Path(ObjectPath),
    Call {
        function: QualifiedName,
        arguments: Vec<ExprId>,
    },
    Unary {
        operator: Located<UnaryOperator>,
        operand: ExprId,
    },
    Binary {
        left: ExprId,
        operator: Located<BinaryOperator>,
        right: ExprId,
    },
    Parenthesized {
        expression: ExprId,
    },
    Defined {
        expression: ExprId,
    },
    RuntimeParameter(QualifiedName),
    ObjectSet(Option<RestrictedTarget>),
    Inspection(InspectionExpression),
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum LiteralSyntax {
    Undefined,
    Numeric {
        value: DecimalConstant,
        unit: Option<QualifiedName>,
    },
    String(Located<String>),
    Enumeration {
        path: Vec<Name>,
        others: Option<Span>,
    },
    Class(QualifiedName),
    AttributePath {
        viewable: Option<QualifiedName>,
        attribute: Name,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct InspectionExpression {
    pub inspection: InspectionSource,
    pub of: Option<ObjectPath>,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum InspectionSource {
    Inline(InlineInspection),
    View(QualifiedName),
}
#[derive(Debug)]
#[allow(dead_code)] // Preserved for expression and view binding.
pub(crate) struct InlineInspection {
    pub area: Option<Span>,
    pub base: RenamedViewable,
    pub attributes: Vec<Name>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct FunctionDecl {
    pub name: Name,
    pub arguments: Vec<FunctionArgument>,
    pub result: ArgumentType,
    pub explanation: Option<Located<String>>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct FunctionArgument {
    pub name: Name,
    pub ty: ArgumentType,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ArgumentType {
    Attribute(Box<TypeUseSyntax>),
    Objects {
        plural: bool,
        target: RestrictedTarget,
        span: Span,
    },
    EnumValue(Span),
    EnumTreeValue(Span),
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct DomainConstraint {
    pub name: Name,
    pub expression: ExprId,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ConstraintsDecl {
    pub target: QualifiedName,
    pub constraints: Vec<ConstraintSyntax>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ConstraintSyntax {
    pub name: Option<Name>,
    pub kind: ConstraintKind,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ConstraintKind {
    Mandatory {
        expression: ExprId,
    },
    Plausibility {
        direction: Located<PlausibilityDirection>,
        percentage: Located<String>,
        expression: ExprId,
    },
    Existence {
        path: ObjectPath,
        required: Vec<ExistenceTarget>,
    },
    Unique {
        basket: Option<Span>,
        filter: Option<ExprId>,
        key: UniqueKey,
    },
    Set {
        basket: Option<Span>,
        filter: Option<ExprId>,
        expression: ExprId,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ExistenceTarget {
    pub viewable: QualifiedName,
    pub path: ObjectPath,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UniqueKey {
    Global(Vec<ObjectPath>),
    Local {
        prefix: Vec<Name>,
        attributes: Vec<Name>,
        span: Span,
    },
}

impl Parser<'_> {
    fn push_expression(&mut self, kind: ExpressionKind, span: Span) -> Parsed<ExprId> {
        self.node()?;
        let index = u32::try_from(self.expressions.len()).map_err(|_| {
            self.diagnostics
                .limit("Expression index capacity exceeded", Some(span.clone()));
            Self::limited()
        })?;
        self.expressions.push(Expression { kind, span });
        Ok(ExprId(index))
    }
    fn expression_span(&self, id: ExprId) -> &Span {
        &self.expressions[id.0 as usize].span
    }
    pub(super) fn expression(&mut self) -> Parsed<ExprId> {
        let left = self.expression_sum()?;
        if self.language == Language::V24 && self.is("=>") {
            let operator = self.binary_operator(BinaryOperator::Implies)?;
            let right = self.expression_sum()?;
            self.binary(left, operator, right)
        } else {
            Ok(left)
        }
    }
    fn expression_sum(&mut self) -> Parsed<ExprId> {
        let mut left = self.expression_product()?;
        loop {
            self.step()?;
            let op = match self.text() {
                "OR" => BinaryOperator::Or,
                "+" if self.language == Language::V24 => BinaryOperator::Add,
                "-" if self.language == Language::V24 => BinaryOperator::Subtract,
                _ => break,
            };
            let operator = self.binary_operator(op)?;
            let right = self.expression_product()?;
            left = self.binary(left, operator, right)?;
        }
        Ok(left)
    }
    fn expression_product(&mut self) -> Parsed<ExprId> {
        let mut left = self.expression_relation()?;
        loop {
            self.step()?;
            let op = match self.text() {
                "AND" => BinaryOperator::And,
                "*" if self.language == Language::V24 => BinaryOperator::Multiply,
                "/" if self.language == Language::V24 => BinaryOperator::Divide,
                _ => break,
            };
            let operator = self.binary_operator(op)?;
            let right = self.expression_relation()?;
            left = self.binary(left, operator, right)?;
        }
        Ok(left)
    }
    fn expression_relation(&mut self) -> Parsed<ExprId> {
        let left = self.predicate()?;
        let op = match self.text() {
            "==" => BinaryOperator::Equal,
            "!=" | "<>" => BinaryOperator::NotEqual,
            "<=" => BinaryOperator::LessEqual,
            ">=" => BinaryOperator::GreaterEqual,
            "<" => BinaryOperator::Less,
            ">" => BinaryOperator::Greater,
            _ => return Ok(left),
        };
        let operator = self.binary_operator(op)?;
        let right = self.predicate()?;
        self.binary(left, operator, right)
    }
    fn binary_operator(&mut self, value: BinaryOperator) -> Parsed<Located<BinaryOperator>> {
        self.node()?;
        let span = self.current_span();
        self.step()?;
        self.at += 1;
        Ok(Located { value, span })
    }
    fn binary(
        &mut self,
        left: ExprId,
        operator: Located<BinaryOperator>,
        right: ExprId,
    ) -> Parsed<ExprId> {
        // Combine original bytes directly. Projecting UTF-16 columns here would
        // repeatedly scan a growing prefix in unindexed parser-only callers.
        let (_, start, _) = self.expression_span(left).sort_key();
        let (_, _, end) = self.expression_span(right).sort_key();
        self.push_expression(
            ExpressionKind::Binary {
                left,
                operator,
                right,
            },
            self.span(start as usize, end as usize),
        )
    }
    fn predicate(&mut self) -> Parsed<ExprId> {
        self.step()?;
        let start = self.start();
        if self.is("NOT") || self.is("(") {
            let not = self.marker("NOT");
            self.expect("(")?;
            self.enter()?;
            let result = self.expression();
            self.depth -= 1;
            let expression = result?;
            self.expect(")")?;
            let span = self.span(start, self.end());
            if let Some(span_op) = not {
                self.node()?;
                self.push_expression(
                    ExpressionKind::Unary {
                        operator: Located {
                            value: UnaryOperator::Not,
                            span: span_op,
                        },
                        operand: expression,
                    },
                    span,
                )
            } else {
                self.push_expression(ExpressionKind::Parenthesized { expression }, span)
            }
        } else if self.take("DEFINED") {
            self.expect("(")?;
            self.enter()?;
            let result = self.factor();
            self.depth -= 1;
            let expression = result?;
            self.expect(")")?;
            self.push_expression(
                ExpressionKind::Defined { expression },
                self.span(start, self.end()),
            )
        } else {
            self.factor()
        }
    }
    pub(super) fn factor(&mut self) -> Parsed<ExprId> {
        self.step()?;
        let start = self.start();
        let kind = if let Some(literal) = self.literal()? {
            ExpressionKind::Literal(literal)
        } else if self.take("PARAMETER") {
            ExpressionKind::RuntimeParameter(self.qualified_name_in(ReferenceContext::Ordinary, 2)?)
        } else if self.is("INSPECTION") || self.is("AREA") {
            ExpressionKind::Inspection(self.inspection_expression()?)
        } else if self.call_ahead()? {
            return self.call();
        } else {
            ExpressionKind::Path(self.object_path()?)
        };
        self.push_expression(kind, self.span(start, self.end()))
    }
    pub(super) fn constant(&mut self) -> Parsed<ExprId> {
        self.step()?;
        let start = self.start();
        let literal = self
            .literal()?
            .ok_or_else(|| Self::expected("Expected a constant"))?;
        self.push_expression(
            ExpressionKind::Literal(literal),
            self.span(start, self.end()),
        )
    }
    fn literal(&mut self) -> Parsed<Option<LiteralSyntax>> {
        let literal = if self.take("UNDEFINED") {
            LiteralSyntax::Undefined
        } else if self.is("PI")
            || self.is("LNBASE")
            || self.is("+")
            || self.is("-")
            || self
                .tokens
                .get(self.at)
                .is_some_and(|t| t.kind == Kind::Number)
        {
            let value = self.decimal_constant()?;
            let unit = if self.take("[") {
                let r = self.qualified_name_in(ReferenceContext::Ordinary, 3)?;
                self.expect("]")?;
                Some(r)
            } else {
                None
            };
            LiteralSyntax::Numeric { value, unit }
        } else if self
            .tokens
            .get(self.at)
            .is_some_and(|t| matches!(t.kind, Kind::String(_)))
        {
            LiteralSyntax::String(self.quoted()?)
        } else if self.take("#") {
            let mut path = vec![];
            let mut others = self.marker("OTHERS");
            if others.is_none() {
                path.push(self.name(false)?);
                while self.take(".") {
                    others = self.marker("OTHERS");
                    if others.is_some() {
                        break;
                    }
                    path.push(self.name(false)?);
                }
            }
            LiteralSyntax::Enumeration { path, others }
        } else if self.take(">>") {
            let first = self.class_reference()?;
            let (viewable, attribute) = if self.take("->") {
                (Some(first), self.name(false)?)
            } else {
                if first.segments.len() != 1
                    || keywords::reserved(&first.segments[0].value, self.language)
                {
                    return Err(Self::expected(
                        "Attribute constant needs an attribute name or -> after its viewable",
                    ));
                }
                (
                    None,
                    first.segments.into_iter().next().expect("nonempty name"),
                )
            };
            LiteralSyntax::AttributePath {
                viewable,
                attribute,
            }
        } else if self.take(">") {
            LiteralSyntax::Class(self.class_reference()?)
        } else {
            return Ok(None);
        };
        Ok(Some(literal))
    }
    fn call_ahead(&mut self) -> Parsed<bool> {
        // Only bounded lookahead to disambiguate Name from qualified FunctionCall.
        let mut at = self.at;
        for _ in 0..3 {
            self.step()?;
            if !self.tokens.get(at).is_some_and(|t| t.kind == Kind::Word) {
                return Ok(false);
            }
            at += 1;
            let next = self
                .tokens
                .get(at)
                .map_or("", |t| &self.source()[t.range.clone()]);
            if next == "(" {
                // A graphic WHERE condition can end in a bare name before
                // the drawing parameters `(Name := ...)`. `:=` cannot follow
                // a name in a call argument.
                let assignment = self
                    .tokens
                    .get(at + 1)
                    .is_some_and(|t| t.kind == Kind::Word)
                    && self
                        .tokens
                        .get(at + 2)
                        .is_some_and(|t| &self.source()[t.range.clone()] == ":=");
                return Ok(!assignment);
            }
            if next != "." {
                return Ok(false);
            }
            at += 1;
        }
        Ok(false)
    }
    fn call(&mut self) -> Parsed<ExprId> {
        let start = self.start();
        let function = self.qualified_name_in(ReferenceContext::Ordinary, 3)?;
        self.expect("(")?;
        self.enter()?;
        let result = (|| {
            let mut arguments = vec![];
            if self.is(")") && self.language == Language::V24 {
                return Ok(arguments);
            }
            loop {
                let argument = if self.is("ALL") {
                    self.object_set()?
                } else {
                    self.expression()?
                };
                arguments.push(argument);
                if !self.take(",") {
                    break;
                }
            }
            Ok(arguments)
        })();
        self.depth -= 1;
        let arguments = result?;
        self.expect(")")?;
        self.push_expression(
            ExpressionKind::Call {
                function,
                arguments,
            },
            self.span(start, self.end()),
        )
    }
    fn object_set(&mut self) -> Parsed<ExprId> {
        let start = self.start();
        self.expect("ALL")?;
        let target = if self.take("(") {
            let t = self.restricted_target()?;
            self.expect(")")?;
            Some(t)
        } else {
            None
        };
        self.push_expression(
            ExpressionKind::ObjectSet(target),
            self.span(start, self.end()),
        )
    }
    fn inspection_expression(&mut self) -> Parsed<InspectionExpression> {
        let start = self.start();
        let area = self.marker("AREA");
        self.expect("INSPECTION")?;
        let inspection = if self.take("OF") {
            InspectionSource::Inline(self.inspection_path(start, area)?)
        } else {
            if area.is_some() {
                return Err(Self::expected("AREA INSPECTION requires OF"));
            }
            InspectionSource::View(self.class_reference()?)
        };
        let of = if self.take("OF") {
            Some(self.object_path()?)
        } else {
            None
        };
        self.node()?;
        Ok(InspectionExpression { inspection, of })
    }
    pub(super) fn inspection_path(
        &mut self,
        start: usize,
        area: Option<Span>,
    ) -> Parsed<InlineInspection> {
        let base = self.renamed_viewable()?;
        self.expect("->")?;
        let mut attributes = vec![self.name(false)?];
        while self.take("->") {
            attributes.push(self.name(false)?);
        }
        self.node()?;
        Ok(InlineInspection {
            area,
            base,
            attributes,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn derived_attribute_value(&mut self) -> Parsed<ExprId> {
        if self.language == Language::V23 {
            self.factor()
        } else {
            self.expression()
        }
    }
    pub(super) fn derivations(&mut self) -> Parsed<Vec<ExprId>> {
        let mut out = vec![];
        if self.take(":=") {
            loop {
                out.push(self.derived_attribute_value()?);
                if !self.take(",") {
                    break;
                }
            }
        }
        Ok(out)
    }
    pub(super) fn function(&mut self) -> Parsed<FunctionDecl> {
        let start = self.start();
        self.expect("FUNCTION")?;
        let name = self.name(false)?;
        self.expect("(")?;
        let mut arguments = vec![];
        if !(self.is(")") && self.language == Language::V24) {
            loop {
                let start = self.start();
                let name = self.name(false)?;
                self.expect(":")?;
                let ty = self.argument_type()?;
                self.node()?;
                arguments.push(FunctionArgument {
                    name,
                    ty,
                    span: self.span(start, self.end()),
                });
                if !self.take(";") {
                    break;
                }
            }
        }
        self.expect(")")?;
        self.expect(":")?;
        let result = self.argument_type()?;
        let explanation = self.explanation()?;
        self.expect(";")?;
        self.node()?;
        Ok(FunctionDecl {
            name,
            arguments,
            result,
            explanation,
            span: self.span(start, self.end()),
        })
    }
    fn argument_type(&mut self) -> Parsed<ArgumentType> {
        let start = self.start();
        if self.is("OBJECT") || self.is("OBJECTS") {
            let plural = self.take("OBJECTS");
            if !plural {
                self.expect("OBJECT")?;
            }
            self.expect("OF")?;
            let target = self.restricted_target()?;
            self.node()?;
            Ok(ArgumentType::Objects {
                plural,
                target,
                span: self.span(start, self.end()),
            })
        } else if let Some(span) = self.marker("ENUMVAL") {
            self.node()?;
            Ok(ArgumentType::EnumValue(span))
        } else if let Some(span) = self.marker("ENUMTREEVAL") {
            self.node()?;
            Ok(ArgumentType::EnumTreeValue(span))
        } else {
            let ty = self.type_use()?;
            self.node()?;
            Ok(ArgumentType::Attribute(Box::new(ty)))
        }
    }
    pub(super) fn constraint_start(&self) -> bool {
        matches!(
            self.text(),
            "MANDATORY" | "CONSTRAINT" | "EXISTENCE" | "UNIQUE" | "SET"
        )
    }
    pub(super) fn constraints(&mut self) -> Parsed<Vec<ConstraintSyntax>> {
        let mut out = vec![];
        while self.constraint_start() && !self.diagnostics.limited {
            self.step()?;
            let before = self.at;
            match self.constraint() {
                Ok(value) => out.push(value),
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
    fn constraint_name(&mut self) -> Parsed<Option<Name>> {
        if self.language == Language::V24 && self.unreserved_name() && self.next_is(":") {
            let n = self.name(false)?;
            self.expect(":")?;
            Ok(Some(n))
        } else {
            Ok(None)
        }
    }
    fn basket(&mut self) -> Parsed<Option<Span>> {
        if self.is("(") && self.next_is("BASKET") {
            let start = self.start();
            if self.language != Language::V24 {
                return Err(Self::expected("Basket constraints require INTERLIS 2.4"));
            }
            self.expect("(")?;
            self.expect("BASKET")?;
            self.expect(")")?;
            Ok(Some(self.span(start, self.end())))
        } else {
            Ok(None)
        }
    }
    fn constraint(&mut self) -> Parsed<ConstraintSyntax> {
        let start = self.start();
        let (name, kind) = if self.take("MANDATORY") {
            self.expect("CONSTRAINT")?;
            let name = self.constraint_name()?;
            (
                name,
                ConstraintKind::Mandatory {
                    expression: self.expression()?,
                },
            )
        } else if self.take("CONSTRAINT") {
            let name = self.constraint_name()?;
            let at = self.current_span();
            let value = if self.take("<=") {
                PlausibilityDirection::AtMost
            } else {
                self.expect(">=")?;
                PlausibilityDirection::AtLeast
            };
            self.node()?;
            let direction = Located { value, span: at };
            let percentage = self.number()?;
            self.expect("%")?;
            (
                name,
                ConstraintKind::Plausibility {
                    direction,
                    percentage,
                    expression: self.expression()?,
                },
            )
        } else if self.take("EXISTENCE") {
            self.expect("CONSTRAINT")?;
            let name = self.constraint_name()?;
            let path = self.object_path()?;
            self.expect("REQUIRED")?;
            self.expect("IN")?;
            let mut required = vec![];
            loop {
                let start = self.start();
                let viewable = self.class_reference()?;
                self.expect(":")?;
                let path = self.object_path()?;
                self.node()?;
                required.push(ExistenceTarget {
                    viewable,
                    path,
                    span: self.span(start, self.end()),
                });
                if !self.take("OR") {
                    break;
                }
            }
            (name, ConstraintKind::Existence { path, required })
        } else if self.take("UNIQUE") {
            let basket = self.basket()?;
            let name = self.constraint_name()?;
            let filter = self.constraint_filter()?;
            let key = if self.is("(") && self.next_is("LOCAL") {
                let start = self.start();
                self.expect("(")?;
                self.expect("LOCAL")?;
                self.expect(")")?;
                let mut prefix = vec![self.name(false)?];
                while self.take("->") {
                    prefix.push(self.name(false)?);
                }
                self.expect(":")?;
                let mut attributes = vec![self.name(false)?];
                while self.take(",") {
                    attributes.push(self.name(false)?);
                }
                self.node()?;
                UniqueKey::Local {
                    prefix,
                    attributes,
                    span: self.span(start, self.end()),
                }
            } else {
                let mut paths = vec![self.object_path()?];
                while self.take(",") {
                    paths.push(self.object_path()?);
                }
                self.node()?;
                UniqueKey::Global(paths)
            };
            (
                name,
                ConstraintKind::Unique {
                    basket,
                    filter,
                    key,
                },
            )
        } else {
            self.expect("SET")?;
            self.expect("CONSTRAINT")?;
            let basket = self.basket()?;
            let name = self.constraint_name()?;
            let filter = self.constraint_filter()?;
            (
                name,
                ConstraintKind::Set {
                    basket,
                    filter,
                    expression: self.expression()?,
                },
            )
        };
        self.expect(";")?;
        self.node()?;
        Ok(ConstraintSyntax {
            name,
            kind,
            span: self.span(start, self.end()),
        })
    }
    fn constraint_filter(&mut self) -> Parsed<Option<ExprId>> {
        if self.take("WHERE") {
            let e = self.expression()?;
            self.expect(":")?;
            Ok(Some(e))
        } else {
            Ok(None)
        }
    }
    pub(super) fn constraints_declaration(&mut self) -> Parsed<ConstraintsDecl> {
        let start = self.start();
        self.expect("CONSTRAINTS")?;
        self.expect("OF")?;
        let target = self.class_reference()?;
        self.expect("=")?;
        let constraints = self.constraints()?;
        self.expect("END")?;
        self.expect(";")?;
        self.node()?;
        Ok(ConstraintsDecl {
            target,
            constraints,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn domain_constraints(&mut self) -> Parsed<Vec<DomainConstraint>> {
        let mut out = vec![];
        if self.take("CONSTRAINTS") {
            if self.language != Language::V24 {
                return Err(Self::expected("Domain constraints require INTERLIS 2.4"));
            }
            loop {
                let start = self.start();
                let name = self.name(false)?;
                self.expect(":")?;
                let expression = self.expression()?;
                self.node()?;
                out.push(DomainConstraint {
                    name,
                    expression,
                    span: self.span(start, self.end()),
                });
                if !self.take(",") {
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    fn source(version: &str, body: &str) -> String {
        format!("INTERLIS {version};\r\n!! 😀\r\nMODEL M AT \"x\" VERSION \"1\" = {body} END M.")
    }
    fn parse_body(
        version: &str,
        body: &str,
        limits: Limits,
    ) -> (SourceSet, Option<Document>, Diagnostics) {
        let s = SourceSet::new(vec![SourceInput {
            name: "memory:///expression-😀.ili".into(),
            text: source(version, body),
        }])
        .unwrap();
        let mut d = Diagnostics::new(&limits);
        let mut b = SyntaxBudget::new(&limits);
        let doc = parse(&s, 0, &limits, &mut b, &mut d);
        (s, doc, d)
    }
    fn clean(version: &str, body: &str) -> (SourceSet, Document) {
        let (s, d, di) = parse_body(version, body, Limits::default());
        assert!(
            d.is_some() && di.entries.is_empty() && !di.syntax_invalid && !di.incomplete,
            "{version} {body}: {:?}",
            di.entries
        );
        (s, d.unwrap())
    }
    fn expr(doc: &Document, id: ExprId) -> &Expression {
        &doc.expressions[id.0 as usize]
    }
    fn binary(doc: &Document, id: ExprId, op: BinaryOperator) -> (ExprId, ExprId) {
        let ExpressionKind::Binary {
            left,
            operator,
            right,
        } = &expr(doc, id).kind
        else {
            panic!("not binary: {:?}", expr(doc, id));
        };
        assert_eq!(operator.value, op);
        (*left, *right)
    }
    fn root(doc: &Document) -> ExprId {
        let Declaration::Container { constraints, .. } = &doc.models[0].declarations[0] else {
            panic!()
        };
        let ConstraintKind::Mandatory { expression } = constraints[0].kind else {
            panic!()
        };
        expression
    }
    fn original<'a>(s: &'a SourceSet, span: &Span) -> &'a str {
        let l = s.location(span).unwrap();
        &s.files()[0].text()[l.start_byte as usize..l.end_byte as usize]
    }
    #[test]
    fn precedence_associativity_and_parentheses_are_real_arena_edges() {
        let (s, d) = clean(
            "2.4",
            "STRUCTURE S=MANDATORY CONSTRAINT a == b AND c OR d + e * f => g; END S;",
        );
        let (left, right) = binary(&d, root(&d), BinaryOperator::Implies);
        assert!(matches!(expr(&d, right).kind, ExpressionKind::Path(_)));
        let (left, added) = binary(&d, left, BinaryOperator::Add);
        let (and, dpath) = binary(&d, left, BinaryOperator::Or);
        let (eq, _) = binary(&d, and, BinaryOperator::And);
        binary(&d, eq, BinaryOperator::Equal);
        assert!(matches!(expr(&d, dpath).kind, ExpressionKind::Path(_)));
        binary(&d, added, BinaryOperator::Multiply);
        let ExpressionKind::Binary { operator, .. } = &expr(&d, root(&d)).kind else {
            panic!()
        };
        assert_eq!(original(&s, &operator.span), "=>");
        assert_eq!(
            original(&s, &expr(&d, root(&d)).span),
            "a == b AND c OR d + e * f => g"
        );
        let (_, d) = clean("2.4", "STRUCTURE S=MANDATORY CONSTRAINT a - b - c; END S;");
        let (left, _) = binary(&d, root(&d), BinaryOperator::Subtract);
        binary(&d, left, BinaryOperator::Subtract);
        for v in ["2.3", "2.4"] {
            let (_, d) = clean(
                v,
                "STRUCTURE S=MANDATORY CONSTRAINT a OR b AND c == d; END S;",
            );
            let (_, right) = binary(&d, root(&d), BinaryOperator::Or);
            let (_, right) = binary(&d, right, BinaryOperator::And);
            binary(&d, right, BinaryOperator::Equal);
            let (_, d) = clean(
                v,
                "STRUCTURE S=MANDATORY CONSTRAINT (a OR b) AND NOT (c == d); END S;",
            );
            let (left, right) = binary(&d, root(&d), BinaryOperator::And);
            let ExpressionKind::Parenthesized { expression } = expr(&d, left).kind else {
                panic!()
            };
            binary(&d, expression, BinaryOperator::Or);
            let ExpressionKind::Unary { operator, operand } = &expr(&d, right).kind else {
                panic!()
            };
            assert_eq!(operator.value, UnaryOperator::Not);
            binary(&d, *operand, BinaryOperator::Equal);
        }
    }
    #[test]
    fn literal_constants_call_arguments_paths_and_inspections_keep_inner_fields() {
        for v in ["2.3", "2.4"] {
            let (s, d) = clean(
                v,
                "STRUCTURE S=a:BOOLEAN := M.T.f(UNDEFINED, -0.1200E3[U.m], PI, LNBASE, \"ä😀\", #red.dark.OTHERS, #OTHERS, >M.T.C, >>M.T.C->name, >>name, PARAMETER M.p, ALL, ALL(ANYCLASS RESTRICTION (M.T.C; Other)), DEFINED(THIS->parts[FIRST]->axis[0002]), \\role->owner[R]->parts[LAST], THISAREA->THATAREA->PARENT->AGGREGATES, INSPECTION V OF THIS->part, AREA INSPECTION OF b~M.T.C->parts->leaf OF THIS->part); END S;",
            );
            let Declaration::Container { attributes, .. } = &d.models[0].declarations[0] else {
                panic!()
            };
            let Declaration::Attribute { derivations, .. } = &attributes[0] else {
                panic!()
            };
            assert_eq!(derivations.len(), 1);
            let ExpressionKind::Call {
                function,
                arguments,
            } = &expr(&d, derivations[0]).kind
            else {
                panic!()
            };
            assert_eq!(
                function
                    .segments
                    .iter()
                    .map(|s| s.value.as_str())
                    .collect::<Vec<_>>(),
                ["M", "T", "f"]
            );
            assert_eq!(arguments.len(), 18);
            assert!(matches!(
                expr(&d, arguments[0]).kind,
                ExpressionKind::Literal(LiteralSyntax::Undefined)
            ));
            let ExpressionKind::Literal(LiteralSyntax::Numeric {
                value: DecimalConstant::Number(n),
                unit: Some(u),
            }) = &expr(&d, arguments[1]).kind
            else {
                panic!()
            };
            assert_eq!(n.value, "-0.1200E3");
            assert_eq!(u.segments[1].value, "m");
            assert_eq!(original(&s, &n.span), "-0.1200E3");
            assert!(matches!(
                expr(&d, arguments[2]).kind,
                ExpressionKind::Literal(LiteralSyntax::Numeric {
                    value: DecimalConstant::Pi(_),
                    unit: None
                })
            ));
            assert!(matches!(
                expr(&d, arguments[3]).kind,
                ExpressionKind::Literal(LiteralSyntax::Numeric {
                    value: DecimalConstant::LnBase(_),
                    unit: None
                })
            ));
            let ExpressionKind::Literal(LiteralSyntax::String(t)) = &expr(&d, arguments[4]).kind
            else {
                panic!()
            };
            assert_eq!(t.value, "ä😀");
            assert_eq!(original(&s, &t.span), "\"ä😀\"");
            let ExpressionKind::Literal(LiteralSyntax::Enumeration { path, others }) =
                &expr(&d, arguments[5]).kind
            else {
                panic!()
            };
            assert_eq!(path.len(), 2);
            assert_eq!(path[1].value, "dark");
            assert_eq!(original(&s, others.as_ref().unwrap()), "OTHERS");
            assert!(
                matches!(&expr(&d,arguments[6]).kind,ExpressionKind::Literal(LiteralSyntax::Enumeration{path,others:Some(_)}) if path.is_empty())
            );
            assert!(
                matches!(&expr(&d,arguments[7]).kind,ExpressionKind::Literal(LiteralSyntax::Class(q)) if q.segments[2].value=="C")
            );
            assert!(
                matches!(&expr(&d,arguments[8]).kind,ExpressionKind::Literal(LiteralSyntax::AttributePath{viewable:Some(q),attribute}) if q.segments[2].value=="C" && attribute.value=="name")
            );
            assert!(
                matches!(&expr(&d,arguments[9]).kind,ExpressionKind::Literal(LiteralSyntax::AttributePath{viewable:None,attribute}) if attribute.value=="name")
            );
            assert!(
                matches!(&expr(&d,arguments[10]).kind,ExpressionKind::RuntimeParameter(q) if q.segments[1].value=="p")
            );
            assert!(matches!(
                expr(&d, arguments[11]).kind,
                ExpressionKind::ObjectSet(None)
            ));
            assert!(
                matches!(&expr(&d,arguments[12]).kind,ExpressionKind::ObjectSet(Some(t)) if matches!(t.target,TargetRef::AnyClass(_)) && t.restrictions.len()==2)
            );
            let ExpressionKind::Defined { expression } = expr(&d, arguments[13]).kind else {
                panic!()
            };
            let ExpressionKind::Path(p) = &expr(&d, expression).kind else {
                panic!()
            };
            assert!(matches!(
                &p.elements[1],
                PathElement::Name {
                    selector: Some(PathSelector::First(_)),
                    ..
                }
            ));
            assert!(
                matches!(&p.elements[2],PathElement::Name{selector:Some(PathSelector::Index(n)),..} if n.value=="0002")
            );
            let ExpressionKind::Path(p) = &expr(&d, arguments[14]).kind else {
                panic!()
            };
            assert!(
                matches!(&p.elements[0],PathElement::AssociationAccess{name,..} if name.value=="role")
            );
            assert!(
                matches!(&p.elements[1],PathElement::Name{selector:Some(PathSelector::Association(n)),..} if n.value=="R")
            );
            assert!(matches!(
                &p.elements[2],
                PathElement::Name {
                    selector: Some(PathSelector::Last(_)),
                    ..
                }
            ));
            let ExpressionKind::Path(p) = &expr(&d, arguments[15]).kind else {
                panic!()
            };
            assert_eq!(p.elements.len(), 4);
            assert!(matches!(
                &p.elements[3],
                PathElement::Special(Located {
                    value: SpecialPath::Aggregates,
                    ..
                })
            ));
            let ExpressionKind::Inspection(i) = &expr(&d, arguments[16]).kind else {
                panic!()
            };
            assert!(matches!(&i.inspection,InspectionSource::View(q) if q.segments[0].value=="V"));
            assert_eq!(i.of.as_ref().unwrap().elements.len(), 2);
            let ExpressionKind::Inspection(i) = &expr(&d, arguments[17]).kind else {
                panic!()
            };
            let InspectionSource::Inline(InlineInspection {
                area: Some(area),
                base,
                attributes,
                ..
            }) = &i.inspection
            else {
                panic!()
            };
            assert_eq!(original(&s, area), "AREA");
            assert_eq!(base.alias.as_ref().unwrap().value, "b");
            assert_eq!(base.target.segments[2].value, "C");
            assert_eq!(
                attributes
                    .iter()
                    .map(|n| n.value.as_str())
                    .collect::<Vec<_>>(),
                ["parts", "leaf"]
            );
        }
    }
    #[test]
    fn functions_edition_specific_emptiness_and_all_argument_types() {
        for v in ["2.3", "2.4"] {
            let (s, d) = clean(
                v,
                "FUNCTION f(a:TEXT*4; b:OBJECT OF ANYCLASS RESTRICTION (A; B); c:OBJECTS OF M.T.V; e:ENUMVAL; t:ENUMTREEVAL; p:ATTRIBUTE OF @a RESTRICTION (TEXT*4)):BOOLEAN // description 😀 //;",
            );
            let Declaration::Function(f) = &d.models[0].declarations[0] else {
                panic!()
            };
            assert_eq!(f.name.value, "f");
            assert_eq!(f.arguments.len(), 6);
            assert!(
                matches!(&f.arguments[0].ty,ArgumentType::Attribute(t) if matches!(&t.ty,Some(TypeSyntax::Text(_))))
            );
            assert!(
                matches!(&f.arguments[1].ty,ArgumentType::Objects{plural:false,target,..} if target.restrictions.len()==2)
            );
            assert!(
                matches!(&f.arguments[2].ty,ArgumentType::Objects{plural:true,target,..} if matches!(&target.target,TargetRef::Named(q) if q.segments[2].value=="V"))
            );
            assert!(matches!(f.arguments[3].ty, ArgumentType::EnumValue(_)));
            assert!(matches!(f.arguments[4].ty, ArgumentType::EnumTreeValue(_)));
            assert_eq!(f.explanation.as_ref().unwrap().value, " description 😀 ");
            assert_eq!(original(&s, &f.name.span), "f");
        }
        let (_, d) = clean(
            "2.4",
            "FUNCTION f():BOOLEAN; STRUCTURE S=a:BOOLEAN := Missing(); END S;",
        );
        let Declaration::Function(f) = &d.models[0].declarations[0] else {
            panic!()
        };
        assert!(f.arguments.is_empty());
        assert!(d.expressions.iter().any(|e|matches!(&e.kind,ExpressionKind::Call{function,arguments} if function.segments[0].value=="Missing"&&arguments.is_empty())));
    }
    #[test]
    fn all_constraints_own_names_filters_targets_and_keys_in_order() {
        let (s, d) = clean(
            "2.4",
            "TOPIC T=CLASS C=a:BOOLEAN; MANDATORY CONSTRAINT m: a; CONSTRAINT p: <= 99.50 % a; EXISTENCE CONSTRAINT e: a->b REQUIRED IN M.T.A:x->y OR M.T.B:z; UNIQUE (BASKET) u: WHERE DEFINED(a): a->b,c; UNIQUE loc: WHERE a: (LOCAL) parts->sub:x,y; SET CONSTRAINT (BASKET) s: WHERE a: f(ALL(C)); PARAMETER p:BOOLEAN; END C; CONSTRAINTS OF M.T.C=MANDATORY CONSTRAINT ext: a; END; END T;",
        );
        let Declaration::Topic { declarations, .. } = &d.models[0].declarations[0] else {
            panic!()
        };
        let Declaration::Container {
            constraints,
            parameters,
            ..
        } = &declarations[0]
        else {
            panic!()
        };
        assert_eq!(parameters.len(), 1);
        assert_eq!(constraints.len(), 6);
        assert_eq!(
            constraints
                .iter()
                .map(|c| c.name.as_ref().unwrap().value.as_str())
                .collect::<Vec<_>>(),
            ["m", "p", "e", "u", "loc", "s"]
        );
        let ConstraintKind::Plausibility {
            direction,
            percentage,
            ..
        } = &constraints[1].kind
        else {
            panic!()
        };
        assert_eq!(direction.value, PlausibilityDirection::AtMost);
        assert_eq!(percentage.value, "99.50");
        assert_eq!(original(&s, &direction.span), "<=");
        let ConstraintKind::Existence { path, required } = &constraints[2].kind else {
            panic!()
        };
        assert_eq!(path.elements.len(), 2);
        assert_eq!(required.len(), 2);
        assert_eq!(required[0].viewable.segments[2].value, "A");
        assert_eq!(required[0].path.elements.len(), 2);
        assert_eq!(original(&s, &required[1].span), "M.T.B:z");
        let ConstraintKind::Unique {
            basket: Some(b),
            filter: Some(f),
            key: UniqueKey::Global(keys),
        } = &constraints[3].kind
        else {
            panic!()
        };
        assert_eq!(original(&s, b), "(BASKET)");
        assert!(matches!(expr(&d, *f).kind, ExpressionKind::Defined { .. }));
        assert_eq!(keys.len(), 2);
        let ConstraintKind::Unique {
            basket: None,
            filter: Some(_),
            key: UniqueKey::Local {
                prefix, attributes, ..
            },
        } = &constraints[4].kind
        else {
            panic!()
        };
        assert_eq!(
            prefix.iter().map(|n| n.value.as_str()).collect::<Vec<_>>(),
            ["parts", "sub"]
        );
        assert_eq!(
            attributes
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>(),
            ["x", "y"]
        );
        let ConstraintKind::Set {
            basket: Some(_),
            filter: Some(filter),
            expression,
        } = &constraints[5].kind
        else {
            panic!()
        };
        assert_ne!(filter, expression);
        assert!(matches!(
            expr(&d, *expression).kind,
            ExpressionKind::Call { .. }
        ));
        let Declaration::Constraints(ext) = &declarations[1] else {
            panic!()
        };
        assert_eq!(ext.target.segments[2].value, "C");
        assert_eq!(ext.constraints.len(), 1);
        clean(
            "2.3",
            "TOPIC T=CLASS C=MANDATORY CONSTRAINT a; CONSTRAINT >= 1% a; EXISTENCE CONSTRAINT a REQUIRED IN C:a; UNIQUE WHERE a:(LOCAL) p:a; SET CONSTRAINT WHERE a:a; END C; CONSTRAINTS OF C= END; END T;",
        );
    }
    #[test]
    fn domain_attribute_and_role_derivations_are_separate_syntax_owners() {
        let (_, d) = clean(
            "2.4",
            "DOMAIN D=TEXT*4 CONSTRAINTS a: THIS != UNDEFINED,b: M.f(THIS); TOPIC T=ASSOCIATION A=r--C := THIS->other; ATTRIBUTE a:BOOLEAN := #true, M.f(THIS); CARDINALITY={1..*}; MANDATORY CONSTRAINT a; END A; END T;",
        );
        let Declaration::Domain { constraints, .. } = &d.models[0].declarations[0] else {
            panic!()
        };
        assert_eq!(constraints.len(), 2);
        assert_eq!(constraints[1].name.value, "b");
        assert!(matches!(
            expr(&d, constraints[1].expression).kind,
            ExpressionKind::Call { .. }
        ));
        let Declaration::Topic { declarations, .. } = &d.models[0].declarations[1] else {
            panic!()
        };
        let Declaration::Association(a) = &declarations[0] else {
            panic!()
        };
        assert!(matches!(
            expr(&d, a.roles[0].derivation.unwrap()).kind,
            ExpressionKind::Path(_)
        ));
        assert!(a.cardinality.is_some());
        assert_eq!(a.constraints.len(), 1);
        let Declaration::Attribute { derivations, .. } = &a.attributes[0] else {
            panic!()
        };
        assert_eq!(derivations.len(), 2);
    }
    #[test]
    fn typed_attribute_derivations_follow_the_language_edition() {
        for (owner, end) in [
            ("CLASS C=", "END C;"),
            ("ASSOCIATION A=r--C; s--C;", "END A;"),
        ] {
            for value in ["b AND c", "b == c", "DEFINED(b)", "NOT(b)", "(b)"] {
                let body = format!("TOPIC T={owner} a:BOOLEAN := #true, {value}; {end} END T;");
                let (_, _, diagnostics) = parse_body("2.3", &body, Limits::default());
                assert!(diagnostics.syntax_invalid, "2.3 accepted {body}");
                clean("2.4", &body);
            }
            let body = format!("TOPIC T={owner} a:BOOLEAN := #true, f(b AND c); {end} END T;");
            for edition in ["2.3", "2.4"] {
                let (_, document) = clean(edition, &body);
                assert!(document.expressions.iter().any(|expression| matches!(
                    &expression.kind,
                    ExpressionKind::Call { arguments, .. }
                        if matches!(expr(&document, arguments[0]).kind, ExpressionKind::Binary { .. })
                )));
            }
        }
    }
    #[test]
    fn expression_semantics_keep_unsupported_constructs_explicit() {
        use AnalysisStatus::{CheckedSubset, Incomplete};
        for (v, body, status) in [
            (
                "2.3",
                "TOPIC T=CLASS C=MANDATORY CONSTRAINT Missing(THIS); END C; END T;",
                Incomplete,
            ),
            ("2.4", "FUNCTION f():BOOLEAN;", Incomplete),
            ("2.4", "CLASS C=a:BOOLEAN := Missing(); END C;", Incomplete),
            (
                "2.4",
                "DOMAIN D=TEXT*4 CONSTRAINTS check: THIS == \"X\";",
                CheckedSubset,
            ),
            (
                "2.3",
                "TOPIC T=CONSTRAINTS OF Missing= END; END T;",
                Incomplete,
            ),
        ] {
            clean(v, body);
            let report = analyze(CompileRequest {
                sources: vec![SourceInput {
                    name: "m".into(),
                    text: source(v, body),
                }],
                limits: Limits::default(),
            })
            .unwrap();
            assert_eq!(report.status(), status, "{body}");
            assert_eq!(report.graph().is_some(), status == CheckedSubset);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&report.to_json().unwrap()).unwrap()["fullValidation"],
                false
            );
            assert!(
                !report
                    .diagnostics()
                    .iter()
                    .any(|d| d.code == "ILI-SYNTAX" || d.code == "ILI-TYPE-NOT-FOUND")
            );
        }
    }
    #[test]
    fn malformed_operators_constraints_and_edition_boundaries_are_syntax_errors() {
        for (v, body) in [
            ("2.3", "FUNCTION f():BOOLEAN;"),
            ("2.3", "CLASS C=MANDATORY CONSTRAINT f(); END C;"),
            ("2.3", "CLASS C=MANDATORY CONSTRAINT a+b; END C;"),
            ("2.3", "CLASS C=MANDATORY CONSTRAINT a=>b; END C;"),
            ("2.3", "CLASS C=MANDATORY CONSTRAINT named:a; END C;"),
            ("2.3", "CLASS C=UNIQUE(BASKET) a; END C;"),
            ("2.3", "CLASS C=SET CONSTRAINT(BASKET) a; END C;"),
            ("2.3", "DOMAIN D=BOOLEAN CONSTRAINTS x:THIS;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT a AND; END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT a==b==c; END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT a=>b=>c; END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT NOT a; END C;"),
            (
                "2.4",
                "CLASS C=MANDATORY CONSTRAINT DEFINED(a AND b); END C;",
            ),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT f(a,); END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT f(ALL()); END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT #red.OTHERS.x; END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT >>SIGN; END C;"),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT >>M.C; END C;"),
            (
                "2.4",
                "CLASS C=MANDATORY CONSTRAINT PARAMETER M.T.p; END C;",
            ),
            (
                "2.4",
                "CLASS C=MANDATORY CONSTRAINT AREA INSPECTION V; END C;",
            ),
            (
                "2.4",
                "CLASS C=MANDATORY CONSTRAINT INSPECTION OF V; END C;",
            ),
            ("2.4", "CLASS C=MANDATORY CONSTRAINT a; b:BOOLEAN; END C;"),
            (
                "2.4",
                "TOPIC T=ASSOCIATION A=r--C := a AND b; END A; END T;",
            ),
            (
                "2.4",
                "CLASS C=EXISTENCE CONSTRAINT a REQUIRED IN C:; END C;",
            ),
            ("2.4", "CLASS C=UNIQUE WHERE a:a,; END C;"),
            ("2.4", "CLASS C=SET CONSTRAINT WHERE a:; END C;"),
            ("2.4", "FUNCTION f(a:BOOLEAN;):BOOLEAN;"),
        ] {
            let (_, _, d) = parse_body(v, body, Limits::default());
            assert!(
                d.syntax_invalid && !d.limited,
                "{v} {body}: {:?}",
                d.entries
            );
            assert!(!d.incomplete, "{body}");
        }
    }
    #[test]
    fn recovery_keeps_two_errors_next_constraint_and_outer_end() {
        let (_, doc, d) = parse_body(
            "2.4",
            "CLASS C=MANDATORY CONSTRAINT a AND; MANDATORY CONSTRAINT b ==; MANDATORY CONSTRAINT Good: #true; END C; DOMAIN After=BOOLEAN;",
            Limits::default(),
        );
        assert!(d.syntax_invalid);
        assert_eq!(
            d.entries.iter().filter(|e| e.code == "ILI-SYNTAX").count(),
            2
        );
        let doc = doc.unwrap();
        assert_eq!(doc.models[0].declarations.len(), 2);
        let Declaration::Container { constraints, .. } = &doc.models[0].declarations[0] else {
            panic!()
        };
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0].name.as_ref().unwrap().value, "Good");
    }
    #[test]
    fn long_flat_chains_walk_and_drop_iteratively_even_on_error_or_budget_failure() {
        const COUNT: usize = 30000;
        let chain = std::iter::repeat_n("a", COUNT)
            .collect::<Vec<_>>()
            .join(" AND ");
        let (_, doc) = clean(
            "2.4",
            &format!("CLASS C=MANDATORY CONSTRAINT {chain}; END C;"),
        );
        assert_eq!(doc.expressions.len(), 2 * COUNT - 1);
        // Real explicit-stack walk, never recursion in an ExprId chain.
        let mut pending = vec![root(&doc)];
        let mut visited = 0;
        while let Some(id) = pending.pop() {
            visited += 1;
            match &expr(&doc, id).kind {
                ExpressionKind::Binary { left, right, .. } => {
                    pending.push(*right);
                    pending.push(*left);
                }
                ExpressionKind::Path(_) => {}
                _ => panic!(),
            }
        }
        assert_eq!(visited, 2 * COUNT - 1);
        drop(doc);
        let body = format!("CLASS C=MANDATORY CONSTRAINT {chain} AND ; END C;");
        let (_, doc, d) = parse_body("2.4", &body, Limits::default());
        assert!(d.syntax_invalid);
        drop(doc);
        for limits in [
            Limits {
                max_syntax_nodes: 100,
                ..Limits::default()
            },
            Limits {
                max_parser_steps: 300000,
                ..Limits::default()
            },
        ] {
            let (_, doc, d) = parse_body("2.4", &body, limits);
            assert!(d.limited);
            assert!(doc.is_none());
        }
        clean("2.4", "CLASS C=MANDATORY CONSTRAINT #true; END C;");
    }
    #[test]
    fn nested_groups_and_calls_are_bounded_before_deeper_construction() {
        for text in [
            format!("{}a{}", "(".repeat(24), ")".repeat(24)),
            format!("{}a{}", "f(".repeat(24), ")".repeat(24)),
        ] {
            clean(
                "2.4",
                &format!("CLASS C=MANDATORY CONSTRAINT {text}; END C;"),
            );
            let (_, doc, d) = parse_body(
                "2.4",
                &format!("CLASS C=MANDATORY CONSTRAINT {text}; END C;"),
                Limits {
                    max_nesting: 8,
                    ..Limits::default()
                },
            );
            assert!(d.limited);
            assert!(doc.is_none());
            assert!(!d.syntax_invalid);
        }
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use crate::SourceInput;
    #[test]
    fn complete_expression_fixtures_parse_without_binding_their_unknown_names() {
        for (text, name) in [
            (
                include_str!("../../tests/fixtures/syntax/expressions23.ili"),
                "Expressions23",
            ),
            (
                include_str!("../../tests/fixtures/syntax/expressions24.ili"),
                "Expressions24",
            ),
        ] {
            let s = SourceSet::new(vec![SourceInput {
                name: format!("memory:///{name}.ili"),
                text: text.into(),
            }])
            .unwrap();
            let limits = Limits::default();
            let mut d = Diagnostics::new(&limits);
            let mut b = SyntaxBudget::new(&limits);
            let doc = parse(&s, 0, &limits, &mut b, &mut d).unwrap();
            assert!(d.entries.is_empty(), "{:?}", d.entries);
            assert!(!d.syntax_invalid && !d.incomplete);
            assert_eq!(s.files()[0].text(), text);
            assert_eq!(doc.models[0].name.value, name);
            assert_eq!(doc.models[0].end_name.value, name);
            let constraints = doc.models[0]
                .declarations
                .iter()
                .find_map(|d| {
                    if let Declaration::Container { constraints, .. } = d {
                        Some(constraints)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(
                constraints.len(),
                if name == "Expressions23" { 6 } else { 7 }
            );
            assert!(
                doc.expressions
                    .iter()
                    .any(|e| matches!(e.kind, ExpressionKind::Call { .. }))
            );
            assert!(matches!(
                &doc.models[0].declarations[0],
                Declaration::Function(_)
            ));
            assert!(
                doc.expressions
                    .iter()
                    .any(|e| matches!(e.kind, ExpressionKind::ObjectSet(_)))
            );
        }
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;
    use crate::SourceInput;
    #[test]
    fn missing_operand_after_emoji_has_original_byte_and_utf16_position() {
        let text = "INTERLIS 2.4;\r\nMODEL M AT \"x\" VERSION \"1\" = CLASS C=MANDATORY CONSTRAINT Unknown(\"😀\") AND ; END C; END M.";
        let s = SourceSet::new(vec![SourceInput {
            name: "memory:///😀.ili".into(),
            text: text.into(),
        }])
        .unwrap();
        let limits = Limits::default();
        let mut d = Diagnostics::new(&limits);
        let mut b = SyntaxBudget::new(&limits);
        let _ = parse(&s, 0, &limits, &mut b, &mut d);
        assert!(d.syntax_invalid);
        assert_eq!(d.entries.len(), 1);
        let l = s.location(d.entries[0].primary.as_ref().unwrap()).unwrap();
        let expected = text.find("; END C").unwrap();
        assert_eq!(l.start_byte as usize, expected);
        assert_eq!(l.end_byte, l.start_byte + 1);
        assert_eq!(l.start_line, 2);
        let line = text.split("\r\n").nth(1).unwrap();
        let prefix = line.split("; END C").next().unwrap();
        assert_eq!(
            l.start_column_utf16 as usize,
            prefix.encode_utf16().count() + 1
        );
        assert_ne!(prefix.len() + 1, l.start_column_utf16 as usize);
    }
}
