use super::{
    Analyzer, BoundName, Kind, Namespace, QualifiedName, SyntaxDecl, TypeInfo,
    collections::compare_count,
    expressions::{scalar_type, underlying_expression},
    units::unit_extends,
};
use crate::{
    BoundExpressionKind, DecimalInfo, ExpressionId, ExpressionType, FunctionInfo, NumericRange,
    RelatedLocation, Span, UnitConstant,
    limits::WorkBudget,
    number::{DecimalLiteral, NumberError},
    syntax::ArgumentType,
};
use std::cmp::Ordering;

impl Analyzer<'_, '_> {
    pub(super) fn bind_functions(&mut self) -> Option<()> {
        for owner in 0..self.symbols.len() {
            let Some(SyntaxDecl::Function(function)) = self.symbols[owner].ast else {
                continue;
            };
            let mut arguments = Vec::with_capacity(function.arguments.len());
            for argument in &function.arguments {
                let id = self.scope_names[&(owner, Namespace::Member, argument.name.value.clone())];
                self.bind_function_type(id, &argument.ty)?;
                arguments.push(id);
            }
            self.bind_function_type(owner, &function.result)?;
            self.symbols[owner].declaration.function = Some(Box::new(FunctionInfo {
                arguments: arguments.into_boxed_slice(),
                explanation: function.explanation.as_ref().map(|text| text.value.clone()),
                explanation_span: function.explanation.as_ref().map(|text| text.span.clone()),
            }));
        }
        Some(())
    }

    fn bind_function_type(&mut self, owner: usize, argument: &ArgumentType) -> Option<()> {
        let ArgumentType::Attribute(ty) = argument else {
            unreachable!("unsupported function arguments are gated");
        };
        let resolved = self.concrete_type(
            owner,
            ty.ty.as_ref().expect("a concrete signature was checked"),
        )?;
        let declaration = &mut self.symbols[owner].declaration;
        declaration.type_id = Some(resolved.ty);
        declaration.declared_mandatory = Some(ty.mandatory);
        declaration.effective_mandatory = Some(ty.mandatory || resolved.mandatory);
        Some(())
    }

    pub(super) fn bind_call(
        &mut self,
        owner: usize,
        function: &QualifiedName,
        arguments: Vec<ExpressionId>,
        span: &Span,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        let target =
            self.expression_name(owner, function, &[Kind::Function], "ILI-FUNCTION-NAME")?;
        let result = match target {
            BoundName::Declared(target) => {
                let count = self.symbols[target].declaration.function()?.arguments.len();
                if !self.function_arity(count, arguments.len(), span) {
                    return None;
                }
                for (position, argument) in arguments.iter().copied().enumerate() {
                    let formal = self.symbols[target].declaration.function()?.arguments[position];
                    let declaration = &self.symbols[formal].declaration;
                    let ty = declaration.type_id?;
                    let mandatory = declaration.mandatory() == Some(true);
                    let at = self.expressions[argument.0 as usize].span.clone();
                    match self.argument_fits(argument, ty, mandatory) {
                        Ok(Some(true)) => {}
                        Ok(Some(false)) => {
                            self.diagnostics.push_related(
                                "ILI-FUNCTION-ARGUMENT",
                                "The argument cannot be assigned to the declared parameter",
                                Some(at),
                                vec![RelatedLocation {
                                    span: self.symbols[formal].name_span.clone(),
                                    message: "Formal parameter".into(),
                                }],
                            );
                            return None;
                        }
                        Ok(None) => {
                            self.unsupported_expression(&at);
                            return None;
                        }
                        Err(error) => {
                            self.number_error(error, &at);
                            return None;
                        }
                    }
                }
                self.symbols[target].declaration.type_id?
            }
            BoundName::Predefined(name, _) => self.bind_predefined_call(name, &arguments, span)?,
        };
        let reference = self.bound_reference(owner, function, target)?;
        Some((
            BoundExpressionKind::Call {
                function: reference,
                arguments,
            },
            scalar_type(&self.types[result]),
            Some(result),
        ))
    }

    fn function_arity(&mut self, expected: usize, given: usize, span: &Span) -> bool {
        if expected == given {
            return true;
        }
        self.diagnostics.push(
            "ILI-FUNCTION-ARITY",
            &format!("Function expects {expected} arguments but received {given}"),
            Some(span.clone()),
        );
        false
    }

    fn bind_predefined_call(
        &mut self,
        name: &str,
        arguments: &[ExpressionId],
        span: &Span,
    ) -> Option<usize> {
        let allowed = match name {
            "len" | "trim" => TypeInfo::UnboundedText,
            "lenM" | "trimM" => TypeInfo::MultilineText { max_length: None },
            _ => {
                self.unsupported_expression(span);
                return None;
            }
        };
        if !self.function_arity(1, arguments.len(), span) {
            return None;
        }
        let formal = self.types.len();
        self.types.push(allowed);
        let argument = arguments[0];
        let at = self.expressions[argument.0 as usize].span.clone();
        match self.argument_fits(argument, formal, false) {
            Ok(Some(true)) => {}
            Ok(Some(false)) => {
                self.diagnostics.push(
                    "ILI-FUNCTION-ARGUMENT",
                    "The argument must match the predefined text parameter",
                    Some(at),
                );
                return None;
            }
            Ok(None) => {
                self.unsupported_expression(&at);
                return None;
            }
            Err(error) => {
                self.number_error(error, &at);
                return None;
            }
        }
        if matches!(name, "len" | "lenM") {
            let result = self.types.len();
            self.types.push(TypeInfo::Numeric(NumericRange {
                bounds: None,
                unit: None,
                reference_system: None,
                circular: false,
                direction: None,
            }));
            Some(result)
        } else {
            Some(formal)
        }
    }

    fn argument_fits(
        &mut self,
        argument: ExpressionId,
        formal: usize,
        mandatory: bool,
    ) -> Result<Option<bool>, NumberError> {
        let actual = underlying_expression(&self.expressions, argument);
        let formal_type = &self.types[formal];
        let expected = scalar_type(formal_type);
        if actual.value_type == ExpressionType::Undefined {
            return Ok(Some(!mandatory));
        }
        if actual.value_type != expected {
            // Contextual constants and object or class relations are checked by
            // their own type families as those become available.
            return Ok((expected != ExpressionType::Other
                && actual.value_type != ExpressionType::Other)
                .then_some(false));
        }
        if actual.type_id == Some(formal) {
            return Ok(Some(true));
        }
        let ty = actual.type_id.map(|id| &self.types[id]);
        match expected {
            ExpressionType::Boolean => Ok(Some(true)),
            ExpressionType::Text => Ok(text_argument_fits(actual, ty, formal_type)),
            ExpressionType::Numeric => {
                let TypeInfo::Numeric(target) = formal_type else {
                    unreachable!("the expected type is numeric");
                };
                let source = match ty {
                    Some(TypeInfo::Numeric(range)) => Some(range),
                    _ => None,
                };
                let constant = match &actual.kind {
                    BoundExpressionKind::Number { value, unit } => Some((value, unit.as_ref())),
                    _ => None,
                };
                let actual_unit = source
                    .and_then(|range| range.unit.as_ref())
                    .or_else(|| constant.and_then(|(_, unit)| unit));
                let units_match = match (actual_unit, target.unit.as_ref()) {
                    (Some(given), Some(expected)) if self.unit_is_abstract(expected) => {
                        unit_extends(&self.symbols, &self.names, given, expected, &mut self.work)?
                    }
                    (Some(given), Some(expected)) => given.target_name() == expected.target_name(),
                    (None, Some(_)) => constant.is_some(),
                    (Some(_), None) => target.bounds.is_none(),
                    (None, None) => true,
                };
                if !units_match {
                    return Ok(Some(false));
                }
                let Some(target_bounds) = &target.bounds else {
                    return Ok(Some(true));
                };
                let (lower, upper) = if let Some(source) = source {
                    let Some(bounds) = &source.bounds else {
                        return Ok(Some(false));
                    };
                    (&bounds.lower, &bounds.upper)
                } else if let Some((UnitConstant::Decimal(value), _)) = constant {
                    (value, value)
                } else {
                    return Ok(None);
                };
                let fits =
                    compare_decimal(lower, &target_bounds.lower, self.limits, &mut self.work)?
                        != Ordering::Less
                        && compare_decimal(
                            upper,
                            &target_bounds.upper,
                            self.limits,
                            &mut self.work,
                        )? != Ordering::Greater;
                Ok(Some(fits))
            }
            _ => Ok(None),
        }
    }
}

fn compare_decimal(
    left: &DecimalInfo,
    right: &DecimalInfo,
    limits: &crate::Limits,
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    let left = DecimalLiteral::parse(left.original(), limits, work)?;
    let right = DecimalLiteral::parse(right.original(), limits, work)?;
    left.compare_value(&right, work)
}

fn text_argument_fits(
    actual: &crate::BoundExpression,
    ty: Option<&TypeInfo>,
    formal: &TypeInfo,
) -> Option<bool> {
    let (required_multi, required_max, required_format) = text_details(formal)?;
    if let Some(actual) = ty {
        let (actual_multi, actual_max, actual_format) = text_details(actual)?;
        if actual_multi && !required_multi
            || required_format.is_some() && required_format != actual_format
        {
            return Some(false);
        }
        return Some(required_max.is_none_or(|max| {
            actual_max.is_some_and(|given| compare_count(given, max) != Ordering::Greater)
        }));
    }
    let BoundExpressionKind::Text { value } = &actual.kind else {
        return None;
    };
    if required_format.is_some() {
        return None;
    }
    Some(
        (required_multi || !value.contains(['\r', '\n']))
            && required_max.is_none_or(|max| {
                compare_count(&value.chars().count().to_string(), max) != Ordering::Greater
            }),
    )
}

fn text_details(ty: &TypeInfo) -> Option<(bool, Option<&str>, Option<crate::FixedTextFormat>)> {
    match ty {
        TypeInfo::Text { max_length } => Some((false, Some(max_length), None)),
        TypeInfo::UnboundedText => Some((false, None, None)),
        TypeInfo::MultilineText { max_length } => Some((true, max_length.as_deref(), None)),
        TypeInfo::FixedText { format, max_length } => {
            Some((false, Some(max_length), Some(*format)))
        }
        _ => None,
    }
}
