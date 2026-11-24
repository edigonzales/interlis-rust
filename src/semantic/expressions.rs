use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName, SyntaxDecl, TypeInfo};
use crate::{
    BinaryOperator, BoundConstraint, BoundExpression, BoundExpressionKind as BoundKind,
    ConstraintRule, ExpressionId, ExpressionType as ValueType, RelatedLocation, Span,
    TypeReference, UnitConstant,
    syntax::{
        ConstraintKind, ConstraintSyntax, DecimalConstant, ExprId, Expression, ExpressionKind,
        LiteralSyntax, ObjectPath, PathElement, SpecialPath,
    },
};
use std::collections::BTreeMap;

pub(super) fn supported_constraint(constraint: &ConstraintSyntax) -> bool {
    matches!(
        constraint.kind,
        ConstraintKind::Mandatory { .. } | ConstraintKind::Plausibility { .. }
    )
}

impl Analyzer<'_, '_> {
    pub(super) fn bind_constraints(&mut self) -> Option<()> {
        let documents: BTreeMap<_, _> = self
            .documents
            .iter()
            .map(|document| (document.span.sort_key().0, document.expressions.as_slice()))
            .collect();
        for owner in 0..self.symbols.len() {
            let mut bound = Vec::new();
            match self.symbols[owner].ast {
                Some(SyntaxDecl::Container {
                    constraints, span, ..
                }) => {
                    self.bind_named_constraints(
                        owner,
                        documents[&span.sort_key().0],
                        constraints,
                        &mut bound,
                    )?;
                }
                Some(SyntaxDecl::Association(association)) => {
                    self.bind_named_constraints(
                        owner,
                        documents[&association.span.sort_key().0],
                        &association.constraints,
                        &mut bound,
                    )?;
                }
                Some(SyntaxDecl::Domain {
                    constraints, span, ..
                }) => {
                    let expressions = documents[&span.sort_key().0];
                    let mut names = BTreeMap::new();
                    for constraint in constraints {
                        let name = &constraint.name;
                        if !self.step(1 + name.value.len(), &name.span) {
                            return None;
                        }
                        if let Some(first) = names.insert(name.value.as_str(), &name.span) {
                            self.duplicate(&name.span, first);
                            continue;
                        }
                        if let Some(constraint) = self.bind_constraint(
                            owner,
                            expressions,
                            Some(name.value.as_str()),
                            ConstraintRule::Domain,
                            constraint.expression,
                            &constraint.span,
                        ) {
                            bound.push(constraint);
                        }
                        if self.diagnostics.limited {
                            return None;
                        }
                    }
                }
                _ => {}
            }
            if !bound.is_empty() {
                self.symbols[owner].declaration.constraints = Some(bound.into_boxed_slice());
            }
        }
        Some(())
    }

    fn bind_named_constraints(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        syntax: &[ConstraintSyntax],
        bound: &mut Vec<BoundConstraint>,
    ) -> Option<()> {
        let mut names = BTreeMap::new();
        for constraint in syntax {
            if let Some(name) = &constraint.name {
                if !self.step(1 + name.value.len(), &name.span) {
                    return None;
                }
                if let Some(first) = names.insert(name.value.as_str(), &name.span) {
                    self.duplicate(&name.span, first);
                    continue;
                }
            }
            let (rule, root) = match &constraint.kind {
                ConstraintKind::Mandatory { expression } => {
                    (ConstraintRule::Mandatory, *expression)
                }
                ConstraintKind::Plausibility {
                    direction,
                    percentage,
                    expression,
                } => {
                    let value = self.literal(percentage)?.info;
                    let coefficient = value.coefficient();
                    let integer_digits = i32::try_from(coefficient.len())
                        .expect("bounded precision")
                        - value.scale();
                    let exact_hundred = coefficient
                        .strip_prefix('1')
                        .is_some_and(|tail| tail.bytes().all(|byte| byte == b'0'));
                    if coefficient.starts_with('-')
                        || coefficient != "0"
                            && (integer_digits > 3 || integer_digits == 3 && !exact_hundred)
                    {
                        self.diagnostics.push(
                            "ILI-CONSTRAINT-PERCENTAGE",
                            "A plausibility percentage must be between zero and one hundred",
                            Some(percentage.span.clone()),
                        );
                        continue;
                    }
                    (
                        ConstraintRule::Plausibility {
                            direction: direction.value,
                            percentage: value,
                        },
                        *expression,
                    )
                }
                _ => unreachable!("unsupported constraint forms are gated"),
            };
            if let Some(constraint) = self.bind_constraint(
                owner,
                expressions,
                constraint.name.as_ref().map(|name| name.value.as_str()),
                rule,
                root,
                &constraint.span,
            ) {
                bound.push(constraint);
            }
            if self.diagnostics.limited {
                return None;
            }
        }
        Some(())
    }

    fn bind_constraint(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        name: Option<&str>,
        rule: ConstraintRule,
        root: ExprId,
        span: &Span,
    ) -> Option<BoundConstraint> {
        let expression = self.bind_expression(owner, expressions, root)?;
        let bound = &self.expressions[expression.0 as usize];
        if bound.value_type != ValueType::Boolean {
            self.diagnostics.push(
                "ILI-CONSTRAINT-BOOLEAN",
                "A constraint requires a Boolean expression",
                Some(bound.span.clone()),
            );
            return None;
        }
        Some(BoundConstraint {
            name: name.map(str::to_owned),
            rule,
            expression,
            span: span.clone(),
        })
    }

    fn bind_expression(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        root: ExprId,
    ) -> Option<ExpressionId> {
        let mut pending = vec![(root, false)];
        let mut bound = BTreeMap::new();
        while let Some((id, visited)) = pending.pop() {
            let expression = &expressions[id.0 as usize];
            if !self.step(1, &expression.span) {
                return None;
            }
            if !visited {
                pending.push((id, true));
                match expression.kind {
                    ExpressionKind::Binary { left, right, .. } => {
                        pending.push((right, false));
                        pending.push((left, false));
                    }
                    ExpressionKind::Unary { operand, .. }
                    | ExpressionKind::Parenthesized {
                        expression: operand,
                    }
                    | ExpressionKind::Defined {
                        expression: operand,
                    } => pending.push((operand, false)),
                    _ => {}
                }
                continue;
            }
            let (kind, value_type, type_id) = match &expression.kind {
                ExpressionKind::Literal(literal) => {
                    self.expression_literal(owner, literal, &expression.span)?
                }
                ExpressionKind::Path(path) => self.expression_path(owner, path)?,
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
                    (
                        BoundKind::Binary {
                            operator: operator.value,
                            left,
                            right,
                        },
                        value_type,
                        None,
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
                _ => {
                    self.unsupported_expression(&expression.span);
                    return None;
                }
            };
            let result = ExpressionId(
                u32::try_from(self.expressions.len()).expect("bounded expression count"),
            );
            self.expressions.push(BoundExpression {
                value_type,
                kind,
                type_id,
                span: expression.span.clone(),
            });
            bound.insert(id.0, result);
        }
        bound.get(&root.0).copied()
    }

    fn expression_literal(
        &mut self,
        owner: usize,
        literal: &LiteralSyntax,
        span: &Span,
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
                (
                    BoundKind::Text {
                        value: text.value.clone(),
                    },
                    ValueType::Text,
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
            _ => {
                self.unsupported_expression(span);
                return None;
            }
        };
        Some((kind, value_type, None))
    }

    fn expression_path(
        &mut self,
        owner: usize,
        path: &ObjectPath,
    ) -> Option<(BoundKind, ValueType, Option<usize>)> {
        let elements = match path.elements.split_first() {
            Some((PathElement::Special(special), tail)) if special.value == SpecialPath::This => {
                if tail.is_empty() {
                    let type_id = self.symbols[owner].declaration.type_id;
                    let value_type =
                        type_id.map_or(ValueType::Object, |ty| scalar_type(&self.types[ty]));
                    return Some((BoundKind::This, value_type, type_id));
                }
                if self.symbols[owner].declaration.kind == Kind::Domain {
                    self.unsupported_expression(&path.span);
                    return None;
                }
                tail
            }
            _ => &path.elements,
        };
        let [
            PathElement::Name {
                name,
                selector: None,
                ..
            },
        ] = elements
        else {
            self.unsupported_expression(&path.span);
            return None;
        };
        match self.visible_name(owner, &name.value, Some(&[Kind::Attribute]), &name.span) {
            Ok(Some(target)) => {
                let type_id = self.symbols[target].declaration.type_id?;
                let reference = self.reference_to_symbol(owner, target, name)?;
                Some((
                    BoundKind::Attribute { reference },
                    scalar_type(&self.types[type_id]),
                    Some(type_id),
                ))
            }
            Err(LookupError::Limit) => None,
            _ => {
                if self.has_role_access(owner, &name.value, &name.span)? {
                    self.unsupported_expression(&path.span);
                } else {
                    self.diagnostics.push(
                        "ILI-PATH-NAME",
                        "The attribute is absent from this object context",
                        Some(name.span.clone()),
                    );
                }
                None
            }
        }
    }

    fn expression_name(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kinds: &[Kind],
        code: &str,
    ) -> Option<BoundName> {
        match self.lookup_name(owner, &name.segments, &name.span, kinds) {
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
        if a == Other || b == Other || a == Object && b == Object {
            self.unsupported_expression(at);
            return None;
        }
        if a == Numeric && b == Numeric && !logical {
            if arithmetic
                && [left, right]
                    .into_iter()
                    .any(|operand| numeric_unit(&self.expressions, &self.types, operand).is_some())
            {
                self.unsupported_expression(at);
                return None;
            }
            return Some(if arithmetic { Numeric } else { Boolean });
        }
        if a == Boolean && b == Boolean && !arithmetic || a == Text && b == Text && equality {
            return Some(Boolean);
        }
        self.invalid_operands(
            at,
            &[left, right],
            "The operand types do not support this operator",
        );
        None
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

    fn unsupported_expression(&mut self, at: &Span) {
        self.diagnostics.push(
            "ILI-UNSUPPORTED",
            "This expression requires semantics outside the implemented subset",
            Some(at.clone()),
        );
    }
}

fn scalar_type(ty: &TypeInfo) -> ValueType {
    match ty {
        TypeInfo::Boolean => ValueType::Boolean,
        TypeInfo::Numeric(_) => ValueType::Numeric,
        TypeInfo::Text { .. }
        | TypeInfo::FixedText { .. }
        | TypeInfo::UnboundedText
        | TypeInfo::MultilineText { .. } => ValueType::Text,
        TypeInfo::Structure(_) | TypeInfo::Reference(_) => ValueType::Object,
        _ => ValueType::Other,
    }
}

fn numeric_unit<'a>(
    expressions: &'a [BoundExpression],
    types: &'a [TypeInfo],
    mut id: ExpressionId,
) -> Option<&'a TypeReference> {
    loop {
        let expression = &expressions[id.0 as usize];
        if let Some(TypeInfo::Numeric(range)) = expression.type_id.map(|id| &types[id]) {
            return range.unit.as_ref();
        }
        match &expression.kind {
            BoundKind::Number { unit, .. } => return unit.as_ref(),
            BoundKind::Group { expression } => id = *expression,
            _ => return None,
        }
    }
}
