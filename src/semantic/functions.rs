use super::{
    Analyzer, BoundName, Kind, Namespace, QualifiedName, SyntaxDecl, TypeInfo,
    collections::{compare_count, structure_targets},
    expressions::{scalar_type, underlying_expression},
    object_references::object_targets,
    units::unit_extends,
};
use crate::{
    BoundExpressionKind, ClassValueKind, ClassValueType, DecimalInfo, ExpressionId, ExpressionType,
    FunctionInfo, GeometryKind, NumericRange, ObjectSelection, ObjectTarget, RelatedLocation, Span,
    UnitConstant,
    limits::WorkBudget,
    number::{DecimalLiteral, NumberError},
    syntax::ArgumentType,
};
use std::cmp::Ordering;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum AssignmentContext {
    Attribute,
    Function,
}

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
        let (type_id, declared, mandatory) = match argument {
            ArgumentType::Attribute(ty) => {
                let resolved = self.concrete_type(
                    owner,
                    ty.ty.as_ref().expect("a concrete signature was checked"),
                )?;
                (
                    resolved.ty,
                    ty.mandatory,
                    ty.mandatory || resolved.mandatory,
                )
            }
            ArgumentType::Objects { plural, target, .. } => {
                let target = self.bind_object_target(owner, target)?;
                (self.object_selection_type(*plural, target), false, false)
            }
            ArgumentType::EnumValue(_) | ArgumentType::EnumTreeValue(_) => {
                let id = self.types.len();
                self.types.push(TypeInfo::EnumArgument {
                    tree_values: matches!(argument, ArgumentType::EnumTreeValue(_)),
                });
                (id, false, false)
            }
        };
        let declaration = &mut self.symbols[owner].declaration;
        declaration.type_id = Some(type_id);
        declaration.declared_mandatory = Some(declared);
        declaration.effective_mandatory = Some(mandatory);
        Some(())
    }

    pub(super) fn object_selection_type(&mut self, plural: bool, target: ObjectTarget) -> usize {
        let id = self.types.len();
        self.types.push(TypeInfo::ObjectSelection(ObjectSelection {
            plural,
            targets: vec![target],
        }));
        id
    }

    pub(super) fn has_objects_parameter(
        &mut self,
        function: BoundName,
        span: &Span,
    ) -> Option<bool> {
        match function {
            BoundName::Predefined(name, _) => Some(name == "objectCount"),
            BoundName::Declared(id) => {
                let arguments = &self.symbols[id].declaration.function()?.arguments;
                let has_objects = arguments.iter().any(|&id| {
                    self.symbols[id].declaration.type_id
                        .is_some_and(|ty| matches!(&self.types[ty], TypeInfo::ObjectSelection(selection) if selection.plural))
                });
                self.step(arguments.len(), span).then_some(has_objects)
            }
        }
    }

    pub(super) fn bind_call(
        &mut self,
        owner: usize,
        target: BoundName,
        function: &QualifiedName,
        arguments: Vec<ExpressionId>,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        let result = match target {
            BoundName::Declared(target) => {
                for (position, argument) in arguments.iter().copied().enumerate() {
                    let formal = self.symbols[target].declaration.function()?.arguments[position];
                    let declaration = &self.symbols[formal].declaration;
                    let ty = declaration.type_id?;
                    let mandatory = declaration.mandatory() == Some(true);
                    let at = self.expressions[argument.0 as usize].span.clone();
                    if !self.value_fits(
                        owner,
                        argument,
                        ty,
                        mandatory,
                        AssignmentContext::Function,
                    )? {
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
                }
                self.symbols[target].declaration.type_id?
            }
            BoundName::Predefined(name, _) => self.bind_predefined_call(owner, name, &arguments)?,
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

    pub(super) fn resolve_call(
        &mut self,
        owner: usize,
        function: &QualifiedName,
        given: usize,
        span: &Span,
    ) -> Option<BoundName> {
        let target =
            self.expression_name(owner, function, &[Kind::Function], "ILI-FUNCTION-NAME")?;
        let expected = match target {
            BoundName::Declared(id) => self.symbols[id].declaration.function()?.arguments.len(),
            BoundName::Predefined(
                "len" | "lenM" | "trim" | "trimM" | "objectCount" | "elementCount" | "myClass",
                _,
            ) => 1,
            BoundName::Predefined("length" | "multilength" | "surface" | "multisurface", _) => 1,
            BoundName::Predefined("isEnumSubVal" | "isSubClass" | "isOfClass", _) => 2,
            BoundName::Predefined("inEnumRange", _) => 3,
            BoundName::Predefined(..) => {
                self.unsupported_expression(span);
                return None;
            }
        };
        if expected != given {
            self.diagnostics.push(
                "ILI-FUNCTION-ARITY",
                &format!("Function expects {expected} arguments but received {given}"),
                Some(span.clone()),
            );
            return None;
        }
        Some(target)
    }

    fn bind_predefined_call(
        &mut self,
        owner: usize,
        name: &str,
        arguments: &[ExpressionId],
    ) -> Option<usize> {
        if matches!(name, "isEnumSubVal" | "inEnumRange") {
            return self.bind_enum_builtin(name, arguments);
        }
        if matches!(name, "myClass" | "isSubClass" | "isOfClass") {
            return self.bind_class_builtin(owner, name, arguments);
        }
        let argument = arguments[0];
        let at = self.expressions[argument.0 as usize].span.clone();
        if matches!(name, "length" | "multilength" | "surface" | "multisurface") {
            let value = &self.expressions[argument.0 as usize];
            let accepted = match value.type_id.map(|ty| &self.types[ty]) {
                Some(TypeInfo::Geometry(geometry)) => {
                    use GeometryKind::*;
                    matches!(
                        (name, geometry.geometry_kind),
                        ("length", Polyline)
                            | ("multilength", MultiPolyline)
                            | ("surface", Surface | Area)
                            | ("multisurface", MultiSurface | MultiArea)
                    )
                }
                _ => value.value_type == ExpressionType::Undefined,
            };
            if !accepted {
                self.diagnostics.push(
                    "ILI-FUNCTION-ARGUMENT",
                    "The argument must have the geometry family and multiplicity declared by the measurement function",
                    Some(at),
                );
                return None;
            }
            return Some(self.unbounded_numeric_type());
        }
        if name == "elementCount" {
            let value = &self.expressions[argument.0 as usize];
            if value.value_type != ExpressionType::Undefined
                && !value.type_id.is_some_and(|ty| {
                    matches!(&self.types[ty], TypeInfo::Collection(collection)
                        if matches!(self.types[collection.element], TypeInfo::Structure(_)))
                })
            {
                self.diagnostics.push(
                    "ILI-FUNCTION-ARGUMENT",
                    "elementCount expects a BAG or LIST of structures",
                    Some(at),
                );
                return None;
            }
            return Some(self.unbounded_numeric_type());
        }
        let allowed = match name {
            "len" | "trim" => TypeInfo::UnboundedText,
            "lenM" | "trimM" => TypeInfo::MultilineText { max_length: None },
            "objectCount" => TypeInfo::ObjectSelection(ObjectSelection {
                plural: true,
                targets: vec![ObjectTarget {
                    target: None,
                    restrictions: Vec::new(),
                }],
            }),
            _ => unreachable!("unsupported predefined functions are gated"),
        };
        let formal = self.types.len();
        self.types.push(allowed);
        if !self.value_fits(owner, argument, formal, false, AssignmentContext::Function)? {
            self.diagnostics.push(
                "ILI-FUNCTION-ARGUMENT",
                "The argument cannot be assigned to the predefined parameter",
                Some(at),
            );
            return None;
        }
        if matches!(name, "len" | "lenM" | "objectCount") {
            Some(self.unbounded_numeric_type())
        } else {
            Some(formal)
        }
    }

    fn bind_class_builtin(
        &mut self,
        owner: usize,
        name: &str,
        arguments: &[ExpressionId],
    ) -> Option<usize> {
        for (index, &argument) in arguments.iter().enumerate() {
            let value = &self.expressions[argument.0 as usize];
            let expected = if index == 0 && name != "isSubClass" {
                ExpressionType::Object
            } else {
                ExpressionType::Class
            };
            if value.value_type != expected && value.value_type != ExpressionType::Undefined {
                self.diagnostics.push(
                    "ILI-FUNCTION-ARGUMENT",
                    if expected == ExpressionType::Object {
                        "This predefined parameter expects a single object or structure"
                    } else {
                        "This predefined parameter expects a class or structure value"
                    },
                    Some(value.span.clone()),
                );
                return None;
            }
        }
        let id = self.types.len();
        if name != "myClass" {
            self.types.push(TypeInfo::Boolean);
            return Some(id);
        }
        let value = underlying_expression(&self.expressions, arguments[0]);
        let path_value = matches!(value.kind, BoundExpressionKind::Path { .. });
        let (kind, restrictions): (_, Vec<_>) = match value.type_id.map(|id| &self.types[id]) {
            Some(TypeInfo::Structure(structure)) => (
                ClassValueKind::Structure,
                structure_targets(structure).iter().collect(),
            ),
            Some(TypeInfo::Reference(reference)) => (
                ClassValueKind::Class,
                object_targets(&reference.target).iter().collect(),
            ),
            Some(TypeInfo::ObjectSelection(selection)) => {
                let restrictions = if selection
                    .targets
                    .iter()
                    .any(|target| object_targets(target).is_empty())
                {
                    Vec::new()
                } else {
                    selection.targets.iter().flat_map(object_targets).collect()
                };
                (ClassValueKind::Class, restrictions)
            }
            _ => (ClassValueKind::Structure, Vec::new()),
        };
        let at = value.span.clone();
        let bytes: usize = restrictions
            .iter()
            .map(|reference| 1 + reference.declared_name().len() + reference.target_name().len())
            .sum();
        // No names or declarations change when this budget is charged.
        if u32::try_from(bytes)
            .map_err(|_| ())
            .and_then(|bytes| self.work.charge(bytes))
            .is_err()
        {
            self.diagnostics
                .limit("Semantic work budget exceeded", Some(at));
            return None;
        }
        let mut restrictions: Vec<_> = restrictions.into_iter().cloned().collect();
        if path_value && let Some(topic) = self.containing_topic(owner) {
            for target in &mut restrictions {
                let Some(&root) = self.names.get(target.target_name()) else {
                    continue;
                };
                let effective = self.contextual_path_scope(topic, root, &at)?;
                if effective != root {
                    let bytes = self.symbols[effective].declaration.qualified.len();
                    if !self.step(bytes, &at) {
                        return None;
                    }
                    target
                        .target
                        .clone_from(&self.symbols[effective].declaration.qualified);
                }
            }
        }
        self.types
            .push(TypeInfo::ClassValue(ClassValueType { kind, restrictions }));
        Some(id)
    }

    fn bind_enum_builtin(&mut self, name: &str, arguments: &[ExpressionId]) -> Option<usize> {
        let anchor = arguments.iter().find_map(|&id| {
            self.expressions[id.0 as usize]
                .type_id
                .filter(|&ty| self.is_enum_type(ty))
        });
        let mut tree = None;
        for (index, &argument) in arguments.iter().enumerate() {
            let allow_tree = name == "isEnumSubVal" || index > 0;
            let value = &self.expressions[argument.0 as usize];
            if value.value_type == ExpressionType::Undefined {
                continue;
            }
            let at = value.span.clone();
            if !self.enum_argument_fits(argument, allow_tree) {
                self.diagnostics.push(
                    "ILI-FUNCTION-ARGUMENT",
                    "The predefined function expects an enumeration value of the declared kind",
                    Some(at),
                );
                return None;
            }
            let Some(anchor) = anchor else {
                self.retain_symbolic_enum(argument);
                continue;
            };
            if let Some(actual) = value.type_id {
                if self.is_enum_type(actual)
                    && (self.enum_specializes(anchor, actual, &at)?
                        || self.enum_specializes(actual, anchor, &at)?)
                {
                    continue;
                }
                self.diagnostics.push(
                    "ILI-ENUM-ARGUMENT-TYPES",
                    "The arguments must refer to the same enumeration or an extension of it",
                    Some(at),
                );
                return None;
            }
            let context = match (allow_tree, &self.types[anchor]) {
                (true, TypeInfo::Enumeration { .. }) => {
                    *tree.get_or_insert_with(|| self.new_enum_tree(anchor, None))
                }
                (false, TypeInfo::EnumTree { base, .. }) => *base,
                _ => anchor,
            };
            self.bind_enum_literal(argument, context)?;
        }
        let result = self.types.len();
        self.types.push(TypeInfo::Boolean);
        Some(result)
    }

    fn enum_argument_fits(&self, argument: ExpressionId, allow_tree: bool) -> bool {
        let actual = underlying_expression(&self.expressions, argument);
        match actual.type_id {
            Some(ty) => match self.types[ty] {
                TypeInfo::EnumTree { .. } => allow_tree,
                TypeInfo::EnumArgument { tree_values } => allow_tree || !tree_values,
                _ => self.is_enum_type(ty),
            },
            None => matches!(
                actual.kind,
                BoundExpressionKind::Boolean { .. } | BoundExpressionKind::Enumeration { .. }
            ),
        }
    }

    pub(super) fn value_fits(
        &mut self,
        owner: usize,
        expression: ExpressionId,
        target: usize,
        mandatory: bool,
        context: AssignmentContext,
    ) -> Option<bool> {
        if self.is_enum_type(target) {
            self.bind_enum_literal(expression, target)?;
        } else if matches!(self.types[target], TypeInfo::EnumArgument { .. }) {
            self.retain_symbolic_enum(expression);
        } else if let TypeInfo::Numeric(range) = &self.types[target] {
            if let Some(unit) = range
                .unit
                .as_ref()
                .filter(|unit| !self.unit_is_abstract(unit))
            {
                self.bind_numeric_literal_unit(expression, unit.clone())?;
            }
        } else {
            self.bind_temporal_literal(expression, target)?;
        }
        if matches!(self.types[target], TypeInfo::AttributePath(_)) {
            let actual = underlying_expression(&self.expressions, expression);
            if actual.value_type != ExpressionType::Undefined {
                let Some(source) = actual.type_id else {
                    return Some(false);
                };
                let at = actual.span.clone();
                return self.attribute_path_specializes(
                    owner,
                    source,
                    target,
                    &at,
                    Some(expression),
                );
            }
        }
        match self.argument_fits(owner, expression, target, mandatory, context) {
            Ok(Some(result)) => Some(result),
            Ok(None) => {
                let at = self.expressions[expression.0 as usize].span.clone();
                self.unsupported_expression(&at);
                None
            }
            Err(error) => {
                let at = self.expressions[expression.0 as usize].span.clone();
                self.number_error(error, &at);
                None
            }
        }
    }

    fn unbounded_numeric_type(&mut self) -> usize {
        let result = self.types.len();
        self.types.push(TypeInfo::Numeric(NumericRange {
            bounds: None,
            unit: None,
            reference_system: None,
            circular: false,
            direction: None,
        }));
        result
    }

    fn argument_fits(
        &mut self,
        owner: usize,
        argument: ExpressionId,
        formal: usize,
        mandatory: bool,
        context: AssignmentContext,
    ) -> Result<Option<bool>, NumberError> {
        let actual = underlying_expression(&self.expressions, argument);
        let formal_type = &self.types[formal];
        let expected = scalar_type(formal_type);
        if actual.value_type == ExpressionType::Undefined {
            return Ok(Some(!mandatory));
        }
        if let TypeInfo::EnumArgument { tree_values } = formal_type {
            return Ok(Some(self.enum_argument_fits(argument, *tree_values)));
        }
        let ty = actual.type_id.map(|id| &self.types[id]);
        if let Some(source) = actual.type_id {
            let at = actual.span.clone();
            if matches!(
                (ty, formal_type),
                (Some(TypeInfo::ClassValue(_)), TypeInfo::ClassValue(_))
            ) {
                return self
                    .class_value_specializes(source, formal, &at, false)
                    .map(Some)
                    .ok_or(NumberError::Work);
            }
            if actual.value_type == ExpressionType::Temporal && expected == ExpressionType::Temporal
            {
                return Ok(Some(self.preserves_temporal_layout(source, formal)));
            }
            if matches!(
                (ty, formal_type),
                (Some(TypeInfo::Structure(_)), TypeInfo::Structure(_))
            ) {
                return self
                    .structure_specializes(source, formal, &at)
                    .map(Some)
                    .ok_or(NumberError::Work);
            }
        }
        let object_sources = match ty {
            Some(TypeInfo::Reference(reference)) => {
                Some((false, std::slice::from_ref(&reference.target)))
            }
            Some(TypeInfo::ObjectSelection(source)) => {
                Some((source.plural, source.targets.as_slice()))
            }
            _ => None,
        };
        let object_formal = match formal_type {
            TypeInfo::ObjectSelection(target) => {
                Some((target.plural, object_targets(&target.targets[0])))
            }
            TypeInfo::Structure(target) if object_sources.is_some() => {
                if context != AssignmentContext::Function {
                    return Ok(Some(false));
                }
                Some((false, structure_targets(target)))
            }
            _ => None,
        };
        if let Some((accepts_many, expected)) = object_formal {
            let Some((source_many, sources)) = object_sources else {
                return Ok(Some(false));
            };
            if source_many && !accepts_many {
                return Ok(Some(false));
            }
            if expected.is_empty() {
                return Ok(Some(true));
            }
            let mut source_ids = Vec::new();
            for source in sources {
                let roots = object_targets(source);
                if roots.is_empty() {
                    return Ok(Some(false));
                }
                source_ids.extend(roots.iter().map(|link| self.names[link.target_name()]));
            }
            let target_ids: Vec<_> = expected
                .iter()
                .map(|link| self.names[link.target_name()])
                .collect();
            let at = actual.span.clone();
            if matches!(actual.kind, BoundExpressionKind::Path { .. })
                && let Some(topic) = self.containing_topic(owner)
            {
                for id in &mut source_ids {
                    *id = self
                        .contextual_path_scope(topic, *id, &at)
                        .ok_or(NumberError::Work)?;
                }
            }
            return self
                .targets_specialize(&source_ids, &target_ids, &at)
                .map(Some)
                .ok_or(NumberError::Work);
        }
        if let Some(source) = actual.type_id
            && self.is_enum_type(formal)
            && self.is_enum_type(source)
        {
            let at = actual.span.clone();
            return self
                .enum_specializes(source, formal, &at)
                .map(Some)
                .ok_or(NumberError::Work);
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
        match expected {
            ExpressionType::Boolean => Ok(Some(true)),
            ExpressionType::Text => Ok(text_argument_fits(actual, ty, formal_type)),
            ExpressionType::Enumeration => Ok(Some(false)),
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
                        return Ok((!matches!(
                            actual.kind,
                            BoundExpressionKind::Binary { .. } | BoundExpressionKind::Number { .. }
                        ))
                        .then_some(false));
                    };
                    (&bounds.lower, &bounds.upper)
                } else if let Some((UnitConstant::Decimal(value), _)) = constant {
                    (value, value)
                } else {
                    return Ok(None);
                };
                let fits = compare_decimal(lower, &target_bounds.lower, &mut self.work)?
                    != Ordering::Less
                    && compare_decimal(upper, &target_bounds.upper, &mut self.work)?
                        != Ordering::Greater;
                Ok(Some(fits))
            }
            _ => Ok(None),
        }
    }
}

pub(super) fn compare_decimal(
    left: &DecimalInfo,
    right: &DecimalInfo,
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    let left = DecimalLiteral::from_info(left, work)?;
    let right = DecimalLiteral::from_info(right, work)?;
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
    let BoundExpressionKind::Text {
        value,
        canonical_length,
    } = &actual.kind
    else {
        return None;
    };
    if required_format.is_some() {
        return None;
    }
    Some(
        (required_multi || !value.contains(['\r', '\n']))
            && required_max.is_none_or(|max| {
                compare_count(&canonical_length.to_string(), max) != Ordering::Greater
            }),
    )
}

pub(super) fn text_details(
    ty: &TypeInfo,
) -> Option<(bool, Option<&str>, Option<crate::FixedTextFormat>)> {
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
