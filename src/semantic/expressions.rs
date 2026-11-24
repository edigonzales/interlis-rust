use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName, TypeInfo};
use crate::{
    BinaryOperator, BoundExpression, BoundExpressionKind as BoundKind, ClassValueKind,
    ClassValueType, DecimalInfo, ExpressionId, ExpressionType as ValueType, LanguageVersion,
    NumericBounds, NumericRange, RelatedLocation, Span, TypeReference, UnitConstant,
    number::arithmetic_bounds,
    syntax::{DecimalConstant, ExprId, Expression, ExpressionKind, LiteralSyntax},
};
use std::collections::BTreeMap;
use unicode_normalization::char::{canonical_combining_class, decompose_canonical};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum ExpressionContext {
    Value,
    IndividualConstraint,
    Set,
}

impl Analyzer<'_, '_> {
    pub(super) fn expression_language(&self, owner: usize) -> LanguageVersion {
        self.models[self.symbols[self.constraint_topic.unwrap_or(owner)].model].language
    }

    pub(super) fn bind_expression(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        root: ExprId,
        context: ExpressionContext,
    ) -> Option<ExpressionId> {
        enum Visit {
            Start(ExprId),
            Argument(ExprId, ExprId),
            Finish(ExprId, Option<BoundName>),
        }
        let mut pending = vec![Visit::Start(root)];
        let mut bound = BTreeMap::new();
        let mut literal_scopes = BTreeMap::new();
        let modern = self.expression_language(owner) == LanguageVersion::V24;
        let mut uses_objects = false;
        while let Some(visit) = pending.pop() {
            let id = match visit {
                Visit::Start(id) | Visit::Finish(id, _) => id,
                Visit::Argument(id, dependency) => {
                    let mut literal = id;
                    while let ExpressionKind::Parenthesized { expression } =
                        expressions[literal.0 as usize].kind
                    {
                        if !self.step(1, &expressions[literal.0 as usize].span) {
                            return None;
                        }
                        literal = expression;
                    }
                    let expression = &expressions[literal.0 as usize];
                    if matches!(
                        expression.kind,
                        ExpressionKind::Literal(LiteralSyntax::AttributePath {
                            viewable: None,
                            ..
                        })
                    ) {
                        let roots =
                            self.attribute_argument_roots(bound[&dependency.0], &expression.span)?;
                        if roots.len() == 1 {
                            literal_scopes.insert(literal.0, *roots.first()?);
                        }
                    }
                    pending.push(Visit::Start(id));
                    continue;
                }
            };
            let expression = &expressions[id.0 as usize];
            if !self.step(1, &expression.span) {
                return None;
            }
            let target = match visit {
                Visit::Finish(_, target) => target,
                Visit::Argument(..) => unreachable!("argument context was resolved before binding"),
                Visit::Start(_) => {
                    let target = if let ExpressionKind::Call {
                        function,
                        arguments,
                    } = &expression.kind
                    {
                        let target =
                            self.resolve_call(owner, function, arguments.len(), &expression.span)?;
                        if modern
                            && context != ExpressionContext::Value
                            && self.has_objects_parameter(target, &function.span)?
                        {
                            if context == ExpressionContext::IndividualConstraint {
                                self.diagnostics.push(
                                    "ILI-CONSTRAINT-OBJECTS",
                                    "An individual constraint cannot call a function with an OBJECTS parameter in INTERLIS 2.4",
                                    Some(function.span.clone()),
                                );
                                return None;
                            }
                            uses_objects = true;
                        }
                        Some(target)
                    } else {
                        None
                    };
                    pending.push(Visit::Finish(id, target));
                    match expression.kind {
                        ExpressionKind::Binary { left, right, .. } => {
                            pending.push(Visit::Start(right));
                            pending.push(Visit::Start(left));
                        }
                        ExpressionKind::Unary { operand, .. }
                        | ExpressionKind::Parenthesized {
                            expression: operand,
                        }
                        | ExpressionKind::Defined {
                            expression: operand,
                        } => pending.push(Visit::Start(operand)),
                        ExpressionKind::Call { ref arguments, .. } => {
                            if !self.step(arguments.len(), &expression.span) {
                                return None;
                            }
                            if let Some(BoundName::Declared(function)) = target {
                                let order = &self.symbols[function]
                                    .declaration
                                    .function()?
                                    .argument_order;
                                pending.extend(order.iter().rev().map(|binding| {
                                    let argument = arguments[binding.position];
                                    binding.dependency.map_or(
                                        Visit::Start(argument),
                                        |dependency| {
                                            Visit::Argument(argument, arguments[dependency])
                                        },
                                    )
                                }));
                            } else {
                                pending.extend(arguments.iter().rev().copied().map(Visit::Start));
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
            };
            let (kind, value_type, type_id) = match &expression.kind {
                ExpressionKind::Literal(literal) => self.expression_literal(
                    owner,
                    literal,
                    &expression.span,
                    literal_scopes.get(&id.0).copied(),
                )?,
                ExpressionKind::Path(path) => {
                    if context == ExpressionContext::Set {
                        self.diagnostics.push(
                            "ILI-SET-OBJECT-VALUE",
                            "An individual object value may appear in WHERE but cannot appear in a SET condition",
                            Some(path.span.clone()),
                        );
                        return None;
                    }
                    self.expression_path(owner, path)?
                }
                ExpressionKind::ObjectSet(target) => {
                    let selected = if let Some(target) = target {
                        self.bind_object_target(
                            self.constraint_topic.unwrap_or(owner),
                            target,
                            &[Kind::Class, Kind::Association, Kind::View],
                        )?
                    } else if matches!(
                        self.symbols[owner].declaration.kind,
                        Kind::Class | Kind::Association | Kind::View
                    ) {
                        let scope = self.expression_object(owner, &expression.span)?;
                        self.current_object_target(scope, "ALL", &expression.span)?
                    } else {
                        self.diagnostics.push(
                            "ILI-ALL-CONTEXT",
                            "ALL without an explicit target requires a class or association context",
                            Some(expression.span.clone()),
                        );
                        return None;
                    };
                    (
                        BoundKind::ObjectSet {
                            explicit: target.is_some(),
                        },
                        ValueType::ObjectSet,
                        Some(self.object_selection_type(true, selected)),
                    )
                }
                ExpressionKind::Call {
                    function,
                    arguments,
                } => self.bind_call(
                    owner,
                    target.expect("the function and arity were resolved before its arguments"),
                    function,
                    arguments
                        .iter()
                        .map(|argument| bound[&argument.0])
                        .collect(),
                )?,
                ExpressionKind::Inspection(inspection) => {
                    if context == ExpressionContext::Set
                        && let Some(restriction) = &inspection.of
                    {
                        self.diagnostics.push(
                            "ILI-SET-OBJECT-VALUE",
                            "An individual object restriction may appear in WHERE but cannot appear in a SET condition",
                            Some(restriction.span.clone()),
                        );
                        return None;
                    }
                    self.bind_inspection_expression(owner, inspection, &expression.span)?
                }
                ExpressionKind::RuntimeParameter(name) => {
                    let BoundName::Declared(target) = self.expression_name(
                        owner,
                        name,
                        &[Kind::RuntimeParameter],
                        "ILI-PARAMETER-NAME",
                    )?
                    else {
                        self.unsupported_expression(&name.span);
                        return None;
                    };
                    let type_id = self.symbols[target].declaration.type_id?;
                    let reference =
                        self.bound_reference(owner, name, BoundName::Declared(target))?;
                    (
                        BoundKind::RuntimeParameter { reference },
                        scalar_type(&self.types[type_id]),
                        Some(type_id),
                    )
                }
                ExpressionKind::Unary { operator, operand } => {
                    let operand: ExpressionId = bound[&operand.0];
                    if self.expressions[operand.0 as usize].value_type != ValueType::Boolean {
                        self.invalid_operands(
                            &operator.span,
                            &[operand],
                            "NOT requires a Boolean operand",
                        );
                        return None;
                    }
                    (
                        BoundKind::Unary {
                            operator: operator.value,
                            operand,
                        },
                        ValueType::Boolean,
                        None,
                    )
                }
                ExpressionKind::Binary {
                    left,
                    operator,
                    right,
                } => {
                    let left = bound[&left.0];
                    let right = bound[&right.0];
                    let value_type =
                        self.binary_type(operator.value, left, right, &operator.span)?;
                    let type_id = if value_type == ValueType::Numeric {
                        Some(self.arithmetic_type(operator.value, left, right, &operator.span)?)
                    } else {
                        None
                    };
                    (
                        BoundKind::Binary {
                            operator: operator.value,
                            left,
                            right,
                        },
                        value_type,
                        type_id,
                    )
                }
                ExpressionKind::Parenthesized { expression } => {
                    let expression: ExpressionId = bound[&expression.0];
                    let child = &self.expressions[expression.0 as usize];
                    (
                        BoundKind::Group { expression },
                        child.value_type,
                        child.type_id,
                    )
                }
                ExpressionKind::Defined { expression } => (
                    BoundKind::Defined {
                        expression: bound[&expression.0],
                    },
                    ValueType::Boolean,
                    None,
                ),
            };
            let result = self.record_expression(kind, value_type, type_id, &expression.span);
            bound.insert(id.0, result);
        }
        let result = *bound.get(&root.0)?;
        if modern
            && context == ExpressionContext::Set
            && !uses_objects
            && self.expressions[result.0 as usize].value_type == ValueType::Boolean
        {
            self.diagnostics.push(
                "ILI-SET-OBJECTS",
                "A SET condition in INTERLIS 2.4 requires a call with an OBJECTS parameter",
                Some(expressions[root.0 as usize].span.clone()),
            );
            return None;
        }
        Some(result)
    }

    pub(super) fn record_expression(
        &mut self,
        kind: BoundKind,
        value_type: ValueType,
        mut type_id: Option<usize>,
        span: &Span,
    ) -> ExpressionId {
        if type_id.is_none()
            && value_type == ValueType::Boolean
            && matches!(
                kind,
                BoundKind::Unary { .. } | BoundKind::Binary { .. } | BoundKind::Defined { .. }
            )
        {
            if self.expression_boolean_type.is_none() {
                self.expression_boolean_type = Some(self.types.len());
                self.types.push(TypeInfo::Boolean);
            }
            type_id = self.expression_boolean_type;
        }
        let id =
            ExpressionId(u32::try_from(self.expressions.len()).expect("bounded expression count"));
        self.expressions.push(BoundExpression {
            value_type,
            kind,
            type_id,
            span: span.clone(),
        });
        id
    }

    fn expression_literal(
        &mut self,
        owner: usize,
        literal: &LiteralSyntax,
        span: &Span,
        contextual_scope: Option<usize>,
    ) -> Option<(BoundKind, ValueType, Option<usize>)> {
        let (kind, value_type) = match literal {
            LiteralSyntax::Undefined => (BoundKind::Undefined, ValueType::Undefined),
            LiteralSyntax::Numeric { value, unit } => {
                let value = match value {
                    DecimalConstant::Number(number) => {
                        UnitConstant::Decimal(self.literal(number)?.info)
                    }
                    DecimalConstant::Pi(_) => UnitConstant::Pi,
                    DecimalConstant::LnBase(_) => UnitConstant::LnBase,
                };
                let unit = match unit {
                    Some(name) => Some(self.bind_unit_reference(owner, name)?),
                    None => None,
                };
                (BoundKind::Number { value, unit }, ValueType::Numeric)
            }
            LiteralSyntax::String(text) => {
                if !self.step(text.value.len(), &text.span) {
                    return None;
                }
                let mut canonical_length = 0;
                for character in text.value.chars() {
                    decompose_canonical(character, |part| {
                        canonical_length += usize::from(canonical_combining_class(part) == 0);
                    });
                }
                let value_type = if canonical_length == 0 {
                    ValueType::Undefined
                } else {
                    ValueType::Text
                };
                (
                    BoundKind::Text {
                        value: text.value.clone(),
                        canonical_length,
                    },
                    value_type,
                )
            }
            LiteralSyntax::Enumeration { path, others: None }
                if path.len() == 1 && matches!(path[0].value.as_str(), "true" | "false") =>
            {
                (
                    BoundKind::Boolean {
                        value: path[0].value == "true",
                    },
                    ValueType::Boolean,
                )
            }
            LiteralSyntax::Enumeration { path, others: None } => (
                BoundKind::Enumeration {
                    path: path.iter().map(|name| name.value.clone()).collect(),
                },
                ValueType::Enumeration,
            ),
            LiteralSyntax::Class(name) => {
                let target = self.expression_name(
                    owner,
                    name,
                    &[Kind::Class, Kind::Structure, Kind::Association, Kind::View],
                    "ILI-CLASS-CONSTANT",
                )?;
                let kind = match target {
                    BoundName::Predefined(_, kind) => kind,
                    BoundName::Declared(id) => self.symbols[id].declaration.kind,
                };
                let target = self.materialized_class_value(owner, target, &name.span)?;
                let reference = self.bound_reference(owner, name, target)?;
                let ty = self.types.len();
                self.types.push(TypeInfo::ClassValue(ClassValueType {
                    kind: if kind == Kind::Structure {
                        ClassValueKind::Structure
                    } else {
                        ClassValueKind::Class
                    },
                    restrictions: vec![reference.clone()],
                }));
                return Some((BoundKind::Class { reference }, ValueType::Class, Some(ty)));
            }
            LiteralSyntax::AttributePath {
                viewable,
                attribute,
            } => {
                return self.attribute_path_literal(
                    owner,
                    viewable.as_ref(),
                    attribute,
                    contextual_scope,
                );
            }
            _ => {
                self.unsupported_expression(span);
                return None;
            }
        };
        Some((kind, value_type, None))
    }

    pub(super) fn expression_name(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kinds: &[Kind],
        code: &str,
    ) -> Option<BoundName> {
        let scope = self.constraint_topic.unwrap_or(owner);
        if let Some(topic) = self.constraint_topic
            && let [name] = name.segments.as_slice()
            && let Some(target) = self
                .visible_name(topic, &name.value, Some(kinds), &name.span)
                .ok()?
        {
            return Some(BoundName::Declared(target));
        }
        match self.lookup_name(scope, &name.segments, &name.span, kinds) {
            Ok(target) => Some(target),
            Err(LookupError::Limit) => None,
            Err(LookupError::Ambiguous(candidates)) => {
                self.diagnostics.push_related(
                    "ILI-EXPRESSION-NAME-AMBIGUOUS",
                    "The name matches more than one imported declaration",
                    Some(name.span.clone()),
                    candidates
                        .into_iter()
                        .map(|candidate| RelatedLocation {
                            span: self.symbols[candidate].name_span.clone(),
                            message: "Candidate declaration".into(),
                        })
                        .collect(),
                );
                None
            }
            Err(_) => {
                self.diagnostics.push(
                    code,
                    "The name is absent from this expression context",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    fn binary_type(
        &mut self,
        operator: BinaryOperator,
        left: ExpressionId,
        right: ExpressionId,
        at: &Span,
    ) -> Option<ValueType> {
        use BinaryOperator::*;
        use ValueType::*;
        let a = self.expressions[left.0 as usize].value_type;
        let b = self.expressions[right.0 as usize].value_type;
        let equality = matches!(operator, Equal | NotEqual);
        let logical = matches!(operator, And | Or | Implies);
        let arithmetic = matches!(operator, Add | Subtract | Multiply | Divide);
        if equality && (a == Undefined || b == Undefined) {
            return Some(Boolean);
        }
        if a == Other || b == Other {
            self.unsupported_expression(at);
            return None;
        }
        if (a == Temporal || b == Temporal)
            && !logical
            && !arithmetic
            && self.temporal_comparable(left, right)?
        {
            return Some(Boolean);
        }
        if (a == Enumeration || b == Enumeration) && !logical && !arithmetic {
            if self.enum_comparable(left, right, equality, at)? {
                return Some(Boolean);
            }
            self.invalid_operands(
                at,
                &[left, right],
                "The operands must use related enumerations and ordered comparisons require ORDERED",
            );
            return None;
        }
        if a == Numeric && b == Numeric && !logical {
            if !matches!(operator, Multiply | Divide) {
                for (typed, literal) in [(left, right), (right, left)] {
                    if let Some(unit) = numeric_operand(&self.expressions, &self.types, typed).unit
                    {
                        self.bind_numeric_literal_unit(literal, unit.clone())?;
                    }
                }
            }
            return Some(if arithmetic { Numeric } else { Boolean });
        }
        if equality
            && let (Some(left), Some(right)) = (
                self.expressions[left.0 as usize].type_id,
                self.expressions[right.0 as usize].type_id,
            )
            && match (&self.types[left], &self.types[right]) {
                (TypeInfo::Coordinate(left), TypeInfo::Coordinate(right)) => {
                    left.kind == right.kind && left.axes.len() == right.axes.len()
                }
                (TypeInfo::Oid(left), TypeInfo::Oid(right)) => {
                    super::existence::oid_compatible(left, right)
                }
                _ => false,
            }
        {
            return Some(Boolean);
        }
        if a == Boolean && b == Boolean && !arithmetic
            || a == Text && b == Text && equality
            || a == Object && b == Object && equality
            || a == Class && b == Class && equality
            || a == AttributePath && b == AttributePath && equality
        {
            return Some(Boolean);
        }
        self.invalid_operands(
            at,
            &[left, right],
            "The operand types do not support this operator",
        );
        None
    }

    pub(super) fn bind_numeric_literal_unit(
        &mut self,
        expression: ExpressionId,
        unit: TypeReference,
    ) -> Option<()> {
        let actual = underlying_expression(&self.expressions, expression);
        let BoundKind::Number { unit: None, .. } = &actual.kind else {
            return Some(());
        };
        if actual.type_id.is_some() {
            return Some(());
        }
        let span = actual.span.clone();
        let (_, start, end) = span.sort_key();
        if !self.step((end - start) as usize + unit.target_name().len(), &span) {
            return None;
        }
        let bounds = match &underlying_expression(&self.expressions, expression).kind {
            BoundKind::Number {
                value: UnitConstant::Decimal(value),
                ..
            } => Some(NumericBounds {
                lower: value.clone(),
                upper: value.clone(),
            }),
            _ => None,
        };
        let ty = self.types.len();
        self.types.push(TypeInfo::Numeric(NumericRange {
            bounds,
            unit: Some(unit),
            reference_system: None,
            circular: false,
            direction: None,
        }));
        let mut id = expression;
        loop {
            let bound = &mut self.expressions[id.0 as usize];
            bound.type_id = Some(ty);
            if let BoundKind::Group { expression } = bound.kind {
                id = expression;
            } else {
                return Some(());
            }
        }
    }

    fn arithmetic_type(
        &mut self,
        operator: BinaryOperator,
        left: ExpressionId,
        right: ExpressionId,
        at: &Span,
    ) -> Option<usize> {
        let a = numeric_operand(&self.expressions, &self.types, left);
        let b = numeric_operand(&self.expressions, &self.types, right);
        let unit = match (a.unit, b.unit) {
            (None, None) => None,
            (a_unit, b_unit)
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) =>
            {
                match (a_unit, b_unit) {
                    (Some(a), Some(b)) if a.target_name() == b.target_name() => Some(a),
                    _ => {
                        self.unsupported_expression(at);
                        return None;
                    }
                }
            }
            _ => {
                self.unsupported_expression(at);
                return None;
            }
        };
        let unit = unit.cloned();
        let bounds = match (a.bounds, b.bounds) {
            (Some(a), Some(b)) => match arithmetic_bounds(operator, a, b, &mut self.work) {
                Ok(bounds) => bounds,
                Err(error) => {
                    self.number_error(error, at);
                    return None;
                }
            },
            _ => None,
        };
        let type_id = self.types.len();
        self.types.push(TypeInfo::Numeric(NumericRange {
            bounds,
            unit,
            reference_system: None,
            circular: false,
            direction: None,
        }));
        Some(type_id)
    }

    fn enum_comparable(
        &mut self,
        left: ExpressionId,
        right: ExpressionId,
        equality: bool,
        at: &Span,
    ) -> Option<bool> {
        let left_type = self.expressions[left.0 as usize]
            .type_id
            .filter(|&ty| self.is_enum_type(ty));
        let right_type = self.expressions[right.0 as usize]
            .type_id
            .filter(|&ty| self.is_enum_type(ty));
        if let Some(ty) = left_type {
            self.bind_enum_literal(right, ty)?;
        }
        if let Some(ty) = right_type {
            self.bind_enum_literal(left, ty)?;
        }
        let Some(a) = self.expressions[left.0 as usize]
            .type_id
            .filter(|&ty| self.is_enum_type(ty))
        else {
            return Some(false);
        };
        let Some(b) = self.expressions[right.0 as usize]
            .type_id
            .filter(|&ty| self.is_enum_type(ty))
        else {
            return Some(false);
        };
        if !equality && !(self.enum_is_ordered(a) && self.enum_is_ordered(b)) {
            return Some(false);
        }
        Some(self.enum_specializes(a, b, at)? || self.enum_specializes(b, a, at)?)
    }

    fn invalid_operands(&mut self, at: &Span, operands: &[ExpressionId], message: &str) {
        self.diagnostics.push_related(
            "ILI-EXPRESSION-OPERANDS",
            message,
            Some(at.clone()),
            operands
                .iter()
                .map(|operand| RelatedLocation {
                    span: self.expressions[operand.0 as usize].span.clone(),
                    message: "Operand".into(),
                })
                .collect(),
        );
    }

    pub(super) fn unsupported_expression(&mut self, at: &Span) {
        self.diagnostics.push(
            "ILI-UNSUPPORTED",
            "This expression requires semantics outside the implemented subset",
            Some(at.clone()),
        );
    }
}

pub(super) fn scalar_type(ty: &TypeInfo) -> ValueType {
    match ty {
        TypeInfo::Boolean => ValueType::Boolean,
        TypeInfo::Numeric(_) => ValueType::Numeric,
        TypeInfo::Text { .. }
        | TypeInfo::FixedText { .. }
        | TypeInfo::UnboundedText
        | TypeInfo::MultilineText { .. } => ValueType::Text,
        TypeInfo::Structure(_) | TypeInfo::Reference(_) => ValueType::Object,
        TypeInfo::ObjectSelection(selection) if selection.plural => ValueType::ObjectSet,
        TypeInfo::ObjectSelection(_) => ValueType::Object,
        TypeInfo::ClassValue(_) => ValueType::Class,
        TypeInfo::AttributePath(_) => ValueType::AttributePath,
        TypeInfo::Temporal(_) | TypeInfo::TemporalRange(_) => ValueType::Temporal,
        TypeInfo::Coordinate(_) => ValueType::Coordinate,
        TypeInfo::Geometry(_) => ValueType::Geometry,
        TypeInfo::Oid(_) => ValueType::Oid,
        TypeInfo::Enumeration { .. }
        | TypeInfo::EnumTree { .. }
        | TypeInfo::EnumArgument { .. }
        | TypeInfo::Alignment(_) => ValueType::Enumeration,
        _ => ValueType::Other,
    }
}

pub(super) struct NumericOperand<'a> {
    pub(super) bounds: Option<(&'a DecimalInfo, &'a DecimalInfo)>,
    pub(super) unit: Option<&'a TypeReference>,
}

pub(super) fn numeric_operand<'a>(
    expressions: &'a [BoundExpression],
    types: &'a [TypeInfo],
    id: ExpressionId,
) -> NumericOperand<'a> {
    let expression = underlying_expression(expressions, id);
    if let Some(TypeInfo::Numeric(range)) = expression.type_id.map(|id| &types[id]) {
        NumericOperand {
            bounds: range
                .bounds
                .as_ref()
                .map(|bounds| (&bounds.lower, &bounds.upper)),
            unit: range.unit.as_ref(),
        }
    } else if let BoundKind::Number { unit, value } = &expression.kind {
        NumericOperand {
            bounds: if let UnitConstant::Decimal(value) = value {
                Some((value, value))
            } else {
                None
            },
            unit: unit.as_ref(),
        }
    } else {
        unreachable!("a numeric expression must have a numeric type or be a number literal")
    }
}

pub(super) fn underlying_expression(
    expressions: &[BoundExpression],
    mut id: ExpressionId,
) -> &BoundExpression {
    loop {
        let expression = &expressions[id.0 as usize];
        if let BoundKind::Group { expression } = expression.kind {
            id = expression;
        } else {
            return expression;
        }
    }
}
