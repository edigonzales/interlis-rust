use super::{
    Analyzer, BoundName, Kind, Namespace, QualifiedName, SyntaxDecl, TypeInfo,
    collections::{compare_count, structure_targets},
    expressions::{scalar_type, underlying_expression},
    object_references::object_targets,
    units::{rational_conversion, unit_extends, units_convertible},
};
use crate::{
    AttributePathOwner, AttributePathType, BoundExpressionKind, ClassValueKind, ClassValueType,
    DecimalInfo, ExpressionId, ExpressionType, FunctionInfo, GeometryKind, NumericRange,
    ObjectSelection, ObjectTarget, RelatedLocation, Span, UnitConstant,
    limits::WorkBudget,
    model::ArgumentBinding,
    number::{DecimalLiteral, NumberError},
    syntax::ArgumentType,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

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
            let argument_order = self.function_argument_order(owner, &arguments, &function.span)?;
            self.symbols[owner].declaration.function = Some(Box::new(FunctionInfo {
                arguments: arguments.into_boxed_slice(),
                explanation: function.explanation.as_ref().map(|text| text.value.clone()),
                explanation_span: function.explanation.as_ref().map(|text| text.span.clone()),
                argument_order,
            }));
        }
        Some(())
    }

    fn argument_dependency(&self, ty: usize) -> Option<usize> {
        let TypeInfo::AttributePath(ty) = &self.types[ty] else {
            return None;
        };
        let Some(AttributePathOwner::Argument(argument)) = &ty.owner else {
            return None;
        };
        self.names.get(argument.target_name()).copied()
    }

    fn function_argument_order(
        &mut self,
        function: usize,
        arguments: &[usize],
        at: &Span,
    ) -> Option<Box<[ArgumentBinding]>> {
        if !self.step(arguments.len(), at) {
            return None;
        }
        let positions: BTreeMap<_, _> = arguments
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();
        let mut dependencies = Vec::with_capacity(arguments.len());
        let mut direct = Vec::with_capacity(arguments.len());
        for &arg in arguments.iter().chain(std::iter::once(&function)) {
            let ty = self.symbols[arg].declaration.type_id?;
            let at = self.symbols[arg].name_span.clone();
            let mut nested = vec![ty];
            let mut visited = BTreeSet::new();
            let mut links = BTreeSet::new();
            while let Some(id) = nested.pop() {
                if !visited.insert(id) {
                    continue;
                }
                if !self.step(1, &at) {
                    return None;
                }
                match &self.types[id] {
                    TypeInfo::Collection(collection) => nested.push(collection.element),
                    TypeInfo::AttributePath(path) => {
                        nested.extend(path.restrictions.iter().map(|restriction| restriction.ty));
                        if let Some(dep) = self.argument_dependency(id) {
                            let mut scope_type = self.symbols[dep].declaration.type_id?;
                            while let TypeInfo::Collection(collection) = &self.types[scope_type] {
                                scope_type = collection.element;
                            }
                            if !matches!(
                                self.types[scope_type],
                                TypeInfo::ClassValue(_)
                                    | TypeInfo::ObjectSelection(_)
                                    | TypeInfo::Reference(_)
                                    | TypeInfo::Structure(_)
                                    | TypeInfo::AttributePath(_)
                            ) {
                                self.diagnostics.push(
                                    "ILI-FUNCTION-ATTRIBUTE-OWNER",
                                    "The dependent formal must provide a class or structure scope",
                                    Some(at.clone()),
                                );
                                return None;
                            }
                            links.insert(positions[&dep]);
                        }
                    }
                    _ => {}
                }
            }
            if arg != function {
                direct.push(self.argument_dependency(ty).map(|arg| positions[&arg]));
                dependencies.push(links.into_iter().collect::<Vec<_>>());
            }
        }
        let mut order = Vec::with_capacity(arguments.len());
        let mut visited = vec![0; arguments.len()];
        let mut pending = Vec::new();
        for start in 0..arguments.len() {
            if visited[start] != 0 {
                continue;
            }
            visited[start] = 1;
            pending.push((start, dependencies[start].iter()));
            while let Some((current, deps)) = pending.last_mut() {
                if let Some(&dep) = deps.next() {
                    match visited[dep] {
                        0 => {
                            visited[dep] = 1;
                            pending.push((dep, dependencies[dep].iter()));
                        }
                        1 => {
                            self.diagnostics.push(
                                "ILI-FUNCTION-ARGUMENT-CYCLE",
                                "Attribute-path arguments cannot depend on one another cyclically",
                                Some(self.symbols[arguments[dep]].name_span.clone()),
                            );
                            return None;
                        }
                        _ => {}
                    }
                    continue;
                }
                let current = *current;
                pending.pop();
                visited[current] = 2;
                order.push(ArgumentBinding {
                    position: current,
                    dependency: direct[current],
                });
            }
        }
        Some(order.into_boxed_slice())
    }

    pub(super) fn attribute_argument_roots(
        &mut self,
        argument: ExpressionId,
        at: &Span,
    ) -> Option<BTreeSet<usize>> {
        let mut pending: Vec<_> = self.expressions[argument.0 as usize]
            .type_id
            .into_iter()
            .collect();
        let mut scopes = BTreeSet::new();
        let mut seen = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty) {
                continue;
            }
            if !self.step(1, at) {
                return None;
            }
            let mut insert = |links: &[crate::TypeReference]| -> Option<()> {
                for link in links {
                    let Some(&id) = self.names.get(link.target_name()) else {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "This argument needs the full predefined object scope",
                            Some(at.clone()),
                        );
                        return None;
                    };
                    scopes.insert(id);
                }
                Some(())
            };
            match &self.types[ty] {
                TypeInfo::ObjectSelection(objects) => {
                    for target in &objects.targets {
                        insert(object_targets(target))?;
                    }
                }
                TypeInfo::Reference(reference) => insert(object_targets(&reference.target))?,
                TypeInfo::Structure(structure) => insert(structure_targets(structure))?,
                TypeInfo::ClassValue(value) => insert(&value.restrictions)?,
                TypeInfo::Collection(collection) => pending.push(collection.element),
                TypeInfo::AttributePath(attribute) => pending.extend(
                    attribute
                        .restrictions
                        .iter()
                        .map(|restriction| restriction.ty),
                ),
                _ => {}
            }
        }
        Some(scopes)
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
                let target = self.bind_object_target(
                    owner,
                    target,
                    &[Kind::Class, Kind::Association, Kind::View],
                )?;
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
                    let ty = self.instantiate_attribute_argument(
                        owner,
                        target,
                        ty,
                        &arguments,
                        Some(argument),
                        &at,
                    )?;
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
                let result = self.symbols[target].declaration.type_id?;
                self.instantiate_attribute_argument(
                    owner,
                    target,
                    result,
                    &arguments,
                    None,
                    &function.span,
                )?
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

    fn instantiate_attribute_argument(
        &mut self,
        owner: usize,
        function: usize,
        ty: usize,
        arguments: &[ExpressionId],
        checking: Option<ExpressionId>,
        at: &Span,
    ) -> Option<usize> {
        let Some(dependency) = self.argument_dependency(ty) else {
            return Some(ty);
        };
        if checking.is_some_and(|id| {
            self.expressions[id.0 as usize].value_type == ExpressionType::Undefined
        }) {
            return Some(ty);
        }
        let parameters = &self.symbols[function].declaration.function()?.arguments;
        if !self.step(parameters.len(), at) {
            return None;
        }
        let parameters = &self.symbols[function].declaration.function()?.arguments;
        let position = parameters.iter().position(|&param| param == dependency)?;
        let dependency = arguments[position];
        let supplied = underlying_expression(&self.expressions, dependency);
        let dynamic = supplied.value_type == ExpressionType::Class
            && !matches!(supplied.kind, BoundExpressionKind::Class { .. })
            && checking.is_none_or(|id| matches!(
                self.expressions[id.0 as usize].type_id.map(|id| &self.types[id]),
                Some(TypeInfo::AttributePath(p)) if matches!(p.owner, Some(AttributePathOwner::ClassAttribute(_)))
            ));
        let context = if dynamic {
            Some(AttributePathOwner::ClassAttribute(dependency))
        } else {
            let scopes = self.attribute_argument_roots(dependency, at)?;
            match scopes.len() {
                1 => {
                    let scope = *scopes.first()?;
                    let name = crate::syntax::Name {
                        value: self.symbols[scope].declaration.name.clone(),
                        span: self.symbols[scope].name_span.clone(),
                    };
                    Some(AttributePathOwner::Viewable(
                        self.reference_to_symbol(owner, scope, &name)?,
                    ))
                }
                0 if checking.is_some() => None,
                _ if checking.is_none() => return Some(ty),
                _ => {
                    self.diagnostics.push(
                        "ILI-FUNCTION-ATTRIBUTE-OWNER",
                        "The actual argument does not identify a single attribute scope",
                        Some(at.clone()),
                    );
                    return None;
                }
            }
        };
        let TypeInfo::AttributePath(formal) = &self.types[ty] else {
            unreachable!()
        };
        if !self.step(formal.restrictions.len(), at) {
            return None;
        }
        let TypeInfo::AttributePath(formal) = &self.types[ty] else {
            unreachable!()
        };
        let result = self.types.len();
        self.types.push(TypeInfo::AttributePath(AttributePathType {
            owner: context,
            restrictions: formal.restrictions.clone(),
        }));
        Some(result)
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
                "len" | "lenM" | "trim" | "trimM" | "objectCount" | "elementCount" | "myClass"
                | "convertUnit",
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
        if name == "convertUnit" {
            let value = underlying_expression(&self.expressions, argument);
            return match value.value_type {
                ExpressionType::Numeric => value
                    .type_id
                    .or_else(|| Some(self.unbounded_numeric_type())),
                ExpressionType::Undefined => Some(self.unbounded_numeric_type()),
                _ => {
                    self.diagnostics.push(
                        "ILI-FUNCTION-ARGUMENT",
                        "convertUnit expects a numeric argument",
                        Some(at),
                    );
                    None
                }
            };
        }
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
        if path_value && let Some(topic) = self.expression_topic(owner) {
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
        if self.contextual_convert_unit(expression, target, mandatory)? {
            return Some(true);
        }
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
            self.bind_formatted_literal(expression, target)?;
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
        let actual = underlying_expression(&self.expressions, expression);
        if let Some(source) = actual.type_id {
            let at = actual.span.clone();
            match (&self.types[source], &self.types[target]) {
                (TypeInfo::Formatted(_), TypeInfo::Formatted(_)) => {
                    return self.formatted_assignment(source, target, &at);
                }
                (TypeInfo::Geometry(_), TypeInfo::Geometry(_)) => {
                    return self.geometry_value_fits(source, target, &at);
                }
                (TypeInfo::Coordinate(_), TypeInfo::Coordinate(_)) => {
                    return self.coordinate_value_fits(
                        source,
                        target,
                        self.expression_language(owner),
                        &at,
                    );
                }
                _ => {}
            }
        }
        if let Some(source) = actual.type_id
            && matches!(
                (&self.types[source], &self.types[target]),
                (TypeInfo::Collection(_), TypeInfo::Collection(_))
            )
        {
            let at = actual.span.clone();
            return self.attribute_type_match(owner, source, target, &at, Some(expression));
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

    fn contextual_convert_unit(
        &mut self,
        expression: ExpressionId,
        target: usize,
        mandatory: bool,
    ) -> Option<bool> {
        let TypeInfo::Numeric(destination) = &self.types[target] else {
            return Some(false);
        };
        let actual = underlying_expression(&self.expressions, expression);
        let BoundExpressionKind::Call {
            function,
            arguments,
        } = &actual.kind
        else {
            return Some(false);
        };
        if function.target_name() != "INTERLIS.convertUnit" {
            return Some(false);
        }
        let argument = arguments[0];
        let at = actual.span.clone();
        let operand = underlying_expression(&self.expressions, argument);
        if operand.value_type == ExpressionType::Undefined {
            if mandatory {
                self.diagnostics.push(
                    "ILI-CONVERT-UNIT-UNDEFINED",
                    "An undefined conversion cannot supply a mandatory value",
                    Some(at),
                );
                return None;
            }
            return Some(true);
        }
        if destination.unit.is_none() && destination.bounds.is_none() {
            return Some(false);
        }
        let origin = super::expressions::numeric_operand(&self.expressions, &self.types, argument);
        let checked = (|| {
            use num_bigint::BigInt;
            let ratio = match (origin.unit, destination.unit.as_ref()) {
                (Some(origin), Some(destination)) => {
                    if !units_convertible(
                        &self.symbols,
                        &self.names,
                        origin,
                        destination,
                        &mut self.work,
                    )? {
                        return Ok(Some("ILI-CONVERT-UNIT"));
                    }
                    if self.types[target].has_unbounded_range() {
                        return Ok(None);
                    }
                    rational_conversion(
                        &self.symbols,
                        &self.names,
                        origin,
                        destination,
                        &mut self.work,
                    )?
                }
                (None, None) => Some((BigInt::from(1), BigInt::from(1))),
                _ => return Ok(Some("ILI-CONVERT-UNIT")),
            };
            if let Some(destination) = &destination.bounds {
                let (Some((lower, upper)), Some((numerator, denominator))) = (origin.bounds, ratio)
                else {
                    return Ok(Some("ILI-UNSUPPORTED"));
                };
                let (lower, upper) = if numerator.sign() == num_bigint::Sign::Minus {
                    (upper, lower)
                } else {
                    (lower, upper)
                };
                if compare_converted(
                    lower,
                    &destination.lower,
                    &numerator,
                    &denominator,
                    &mut self.work,
                )? == Ordering::Less
                    || compare_converted(
                        upper,
                        &destination.upper,
                        &numerator,
                        &denominator,
                        &mut self.work,
                    )? == Ordering::Greater
                {
                    return Ok(Some("ILI-CONVERT-UNIT-RANGE"));
                }
            }
            Ok(None)
        })();
        let checked = match checked {
            Ok(result) => result,
            Err(error) => {
                self.number_error(error, &at);
                return None;
            }
        };
        if let Some(code) = checked {
            let message = match code {
                "ILI-CONVERT-UNIT" => "The source and destination must share a conversion unit",
                "ILI-CONVERT-UNIT-RANGE" => {
                    "The converted value can fall outside its destination range"
                }
                _ => "The converted range cannot be established exactly",
            };
            self.diagnostics.push(code, message, Some(at));
            return None;
        }
        if actual.type_id == operand.type_id
            || operand.type_id.is_none()
                && actual
                    .type_id
                    .is_some_and(|ty| self.types[ty].has_unbounded_range())
        {
            let mut id = expression;
            loop {
                let value = &mut self.expressions[id.0 as usize];
                value.type_id = Some(target);
                if let BoundExpressionKind::Group { expression } = value.kind {
                    id = expression;
                } else {
                    break;
                }
            }
        }
        Some(true)
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
                && let Some(topic) = self.expression_topic(owner)
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

fn compare_converted(
    source: &DecimalInfo,
    target: &DecimalInfo,
    numerator: &num_bigint::BigInt,
    denominator: &num_bigint::BigInt,
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    use num_bigint::BigInt;
    let places = source.scale().abs_diff(target.scale());
    let bytes = u64::from(places)
        + numerator.bits().max(denominator.bits()).div_ceil(8)
        + (source.coefficient().len() + target.coefficient().len()) as u64;
    work.charge(u32::try_from(bytes).map_err(|_| NumberError::Limit)?)
        .map_err(|_| NumberError::Work)?;
    let coefficient = |number: &DecimalInfo| {
        BigInt::parse_bytes(number.coefficient().as_bytes(), 10).ok_or(NumberError::Form)
    };
    let mut left = coefficient(source)? * numerator;
    let mut right = coefficient(target)? * denominator;
    if places > 0 {
        let scale = BigInt::from(10).pow(places);
        if source.scale() < target.scale() {
            left *= scale;
        } else {
            right *= scale;
        }
    }
    Ok(left.cmp(&right))
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
