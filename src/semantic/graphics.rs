use super::{
    Analyzer, BoundName, Kind, Namespace, QualifiedName, SyntaxDecl,
    expressions::ExpressionContext, functions::AssignmentContext,
};
use crate::{
    DrawingAssignment, DrawingCase, DrawingClause, DrawingRuleInfo, DrawingValue, ExpressionId,
    ExpressionType, GraphicInfo, Span, TypeInfo, TypeReference,
    syntax::{
        DrawingRule, EnumAssignmentValue, ExprId, Expression, ExpressionKind, GraphicDecl,
        LiteralSyntax, Property, SignParameterValue,
    },
};
use std::collections::{BTreeMap, BTreeSet};

impl Analyzer<'_, '_> {
    pub(super) fn bind_graphics(&mut self, documents: &BTreeMap<u32, &[Expression]>) -> Option<()> {
        let mut graphics = Vec::new();
        for owner in 0..self.symbols.len() {
            let Some(SyntaxDecl::Graphic(graphic)) = self.symbols[owner].ast else {
                continue;
            };
            graphics.push(owner);
            if let Some(name) = &graphic.base {
                let (base, reference) = self.graphic_reference(owner, name, &[Kind::Graphic])?;
                if self.symbols[base].declaration.is_final() {
                    return self.graphic_error(
                        "ILI-GRAPHIC-FINAL",
                        "A FINAL graphic cannot be extended",
                        &name.span,
                    );
                }
                self.symbols[owner].base_scope = Some(base);
                self.symbols[owner].declaration.base_reference = Some(reference);
            }
        }
        for owner in self.inheritance_order(graphics, "ILI-GRAPHIC-CYCLE")? {
            let Some(SyntaxDecl::Graphic(graphic)) = self.symbols[owner].ast else {
                unreachable!("graphic order contains only graphics");
            };
            let initial = self.constraint_topic;
            self.constraint_topic = self.containing_topic(owner);
            let result = self.bind_graphic(owner, graphic, documents[&graphic.span.sort_key().0]);
            self.constraint_topic = initial;
            result?;
        }
        Some(())
    }

    fn bind_graphic(
        &mut self,
        owner: usize,
        syntax: &GraphicDecl,
        statements: &[Expression],
    ) -> Option<()> {
        let inherited = self.symbols[owner].base_scope;
        let parent = inherited.and_then(|parent| self.symbols[parent].declaration.graphic());
        let mut effective = parent.map_or_else(Vec::new, |parent| parent.effective_rules.to_vec());
        let mut selections = parent.map_or_else(Vec::new, |parent| parent.selections.clone());
        let inherited_basis = parent.map(|parent| parent.basis.clone());
        let basis = if let Some(name) = &syntax.viewable {
            self.graphic_reference(owner, name, &[Kind::Class, Kind::View])?
                .1
        } else if let Some(basis) = &inherited_basis {
            basis.clone()
        } else {
            return self.graphic_error(
                "ILI-GRAPHIC-BASIS",
                "A graphic requires a class or view basis",
                &syntax.name.span,
            );
        };
        let scope = self.names[basis.target_name()];
        if let Some(parent) = inherited_basis
            && !self.specializes_symbol(
                scope,
                self.names[parent.target_name()],
                basis.source_span(),
            )?
        {
            return self.graphic_error(
                "ILI-GRAPHIC-BASIS",
                "A graphic basis must specialize the inherited basis",
                basis.source_span(),
            );
        }
        for dependency in [Some(scope), inherited].into_iter().flatten() {
            if let (Some(current), Some(other)) = (
                self.containing_topic(owner),
                self.containing_topic(dependency),
            ) && !self.specializes_symbol(current, other, &syntax.name.span)?
                && !self.topic_has_dependency(current, other, &syntax.name.span)?
            {
                return self.graphic_error(
                    "ILI-GRAPHIC-TOPIC-DEPENDENCY",
                    "A graphic accessing another topic requires DEPENDS ON",
                    basis.source_span(),
                );
            }
        }
        if !self.step(effective.len() + selections.len(), &syntax.name.span) {
            return None;
        }
        for selection in &syntax.selections {
            selections.push(self.graphic_condition(scope, statements, selection.expression)?);
        }
        let positions: BTreeMap<_, _> = effective
            .iter()
            .enumerate()
            .map(|(position, &id)| (self.symbols[id].declaration.name.clone(), position))
            .collect();
        let mut rules = Vec::with_capacity(syntax.rules.len());
        for rule in &syntax.rules {
            let id = self.scope_names[&(owner, Namespace::Member, rule.name.value.clone())];
            let position = positions.get(&rule.name.value).copied();
            self.bind_drawing_rule(
                id,
                scope,
                statements,
                rule,
                position.map(|position| effective[position]),
            )?;
            if let Some(position) = position {
                effective[position] = id;
            } else {
                effective.push(id);
            }
            rules.push(id);
        }
        if !self.symbols[owner].declaration.is_abstract()
            && effective
                .iter()
                .any(|&id| self.symbols[id].declaration.is_abstract())
        {
            return self.graphic_error(
                "ILI-GRAPHIC-ABSTRACT-RULE",
                "A concrete graphic cannot retain an abstract drawing rule",
                &syntax.name.span,
            );
        }
        self.symbols[owner].declaration.graphic = Some(Box::new(GraphicInfo {
            basis,
            selections,
            rules: rules.into_boxed_slice(),
            effective_rules: effective.into_boxed_slice(),
        }));
        Some(())
    }

    fn graphic_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kinds: &[Kind],
    ) -> Option<(usize, TypeReference)> {
        let target = self.expression_name(owner, name, kinds, "ILI-GRAPHIC-NAME")?;
        let id = match target {
            BoundName::Declared(id) => id,
            BoundName::Predefined(builtin, Kind::Class) => {
                self.predefined_metadata(owner, builtin, &name.span)?
            }
            BoundName::Predefined(..) => {
                self.unsupported_expression(&name.span);
                return None;
            }
        };
        Some((
            id,
            self.bound_reference(owner, name, BoundName::Declared(id))?,
        ))
    }

    fn graphic_condition(
        &mut self,
        scope: usize,
        statements: &[Expression],
        expression: ExprId,
    ) -> Option<ExpressionId> {
        let bound =
            self.bind_expression(scope, statements, expression, ExpressionContext::Value)?;
        let value = &self.expressions[bound.0 as usize];
        if value.value_type == ExpressionType::Boolean {
            Some(bound)
        } else {
            self.graphic_error(
                "ILI-GRAPHIC-CONDITION",
                "A graphic condition must be Boolean",
                &value.span.clone(),
            )
        }
    }

    fn bind_drawing_rule(
        &mut self,
        id: usize,
        scope: usize,
        statements: &[Expression],
        syntax: &DrawingRule,
        inherited: Option<usize>,
    ) -> Option<()> {
        let at = &syntax.name.span;
        let extended = syntax
            .properties
            .iter()
            .any(|property| property.value == Property::Extended);
        if extended != inherited.is_some() {
            return self.graphic_error(
                "ILI-GRAPHIC-RULE-OVERRIDE",
                "EXTENDED requires an inherited drawing rule of the same name",
                at,
            );
        }
        let inherited_signature = if let Some(base) = inherited {
            if self.symbols[base].declaration.is_final() {
                return self.graphic_error(
                    "ILI-GRAPHIC-RULE-FINAL",
                    "A FINAL rule cannot be overridden",
                    at,
                );
            }
            self.symbols[id].base_scope = Some(base);
            self.symbols[id].declaration.base_reference =
                Some(self.reference_to_symbol(id, base, &syntax.name)?);
            self.symbols[base]
                .declaration
                .drawing_rule()?
                .signature_class
                .clone()
        } else {
            None
        };
        let signature = if let Some(name) = &syntax.class {
            let (class, reference) = self.graphic_reference(id, name, &[Kind::Class])?;
            self.predefined_metadata(id, "SIGN", &name.span)?;
            if !self.specializes_metadata_family(class, "INTERLIS.SIGN", &name.span)? {
                return self.graphic_error(
                    "ILI-GRAPHIC-SIGNATURE",
                    "A drawing rule requires a SIGN class",
                    &name.span,
                );
            }
            if let Some(base) = &inherited_signature
                && !self.specializes_symbol(class, self.names[base.target_name()], &name.span)?
            {
                return self.graphic_error(
                    "ILI-GRAPHIC-SIGNATURE",
                    "A drawing signature must specialize the inherited signature",
                    &name.span,
                );
            }
            Some(reference)
        } else {
            inherited_signature
        };
        let Some(class) = signature
            .as_ref()
            .map(|class| self.names[class.target_name()])
        else {
            return self.graphic_error(
                "ILI-GRAPHIC-SIGNATURE",
                "A drawing rule requires a signature",
                at,
            );
        };
        let mut clauses = Vec::with_capacity(syntax.clauses.len());
        for clause in &syntax.clauses {
            let condition = if let Some(condition) = clause.condition {
                Some(self.graphic_condition(scope, statements, condition)?)
            } else {
                None
            };
            let mut assignments = Vec::with_capacity(clause.assignments.len());
            for assignment in &clause.assignments {
                let name = &assignment.parameter;
                let Some(parameter) = self
                    .visible_name(class, &name.value, Some(&[Kind::Parameter]), &name.span)
                    .ok()?
                else {
                    return self.graphic_error(
                        "ILI-GRAPHIC-PARAMETER",
                        "Unknown signature parameter",
                        &name.span,
                    );
                };
                let value = self.bind_drawing_value(id, scope, statements, &assignment.value)?;
                self.check_drawing_value(scope, parameter, &value, &assignment.span)?;
                assignments.push(DrawingAssignment {
                    parameter: self.reference_to_symbol(id, parameter, name)?,
                    value,
                    span: assignment.span.clone(),
                });
            }
            clauses.push(DrawingClause {
                condition,
                assignments,
                span: clause.span.clone(),
            });
        }
        self.symbols[id].declaration.drawing_rule = Some(Box::new(DrawingRuleInfo {
            signature_class: signature,
            clauses: clauses.into_boxed_slice(),
        }));
        if !self.symbols[id].declaration.is_abstract() {
            self.check_graphic_signature(id, scope, class, at)?;
        }
        Some(())
    }

    fn bind_drawing_value(
        &mut self,
        owner: usize,
        scope: usize,
        statements: &[Expression],
        value: &SignParameterValue,
    ) -> Option<DrawingValue> {
        match value {
            SignParameterValue::MetaObject(object) => Some(DrawingValue::MetaObject {
                reference: self.metaobject_reference(owner, &object.value)?,
            }),
            SignParameterValue::Factor(root) => Some(DrawingValue::Expression {
                expression: self.bind_expression(
                    scope,
                    statements,
                    *root,
                    ExpressionContext::Value,
                )?,
            }),
            SignParameterValue::According { path, cases, span } => {
                let (kind, value, ty) = self.expression_path(scope, path)?;
                let Some(ty) = ty.filter(|&ty| self.is_enum_type(ty)) else {
                    return self.graphic_error(
                        "ILI-GRAPHIC-ACCORDING",
                        "ACCORDING requires an enumeration selector",
                        &path.span,
                    );
                };
                let selector = self.record_expression(kind, value, Some(ty), &path.span);
                let endpoint_type = if matches!(self.types[ty], TypeInfo::Enumeration { .. }) {
                    self.new_enum_tree(ty, None)
                } else {
                    ty
                };
                let mut values = Vec::with_capacity(cases.len());
                let mut others = false;
                for case in cases {
                    let first_syntax = &statements[case.range.first.0 as usize];
                    let first = if matches!(
                        &first_syntax.kind,
                        ExpressionKind::Literal(LiteralSyntax::Enumeration {
                            others: Some(_),
                            ..
                        },)
                    ) {
                        if others || case.range.last.is_some() {
                            return self.graphic_error(
                                "ILI-GRAPHIC-ENUM-RANGE",
                                "OTHERS must be a single fallback",
                                &case.range.span,
                            );
                        }
                        others = true;
                        None
                    } else {
                        Some(self.drawing_enum_endpoint(
                            scope,
                            statements,
                            case.range.first,
                            endpoint_type,
                        )?)
                    };
                    let last = if let Some(last) = case.range.last {
                        let last =
                            self.drawing_enum_endpoint(scope, statements, last, endpoint_type)?;
                        self.drawing_enum_range(ty, first?, last, &case.range.span)?;
                        Some(last)
                    } else {
                        None
                    };
                    let value = match &case.value {
                        EnumAssignmentValue::MetaObject(object) => DrawingValue::MetaObject {
                            reference: self.metaobject_reference(owner, &object.value)?,
                        },
                        EnumAssignmentValue::Constant(root) => DrawingValue::Expression {
                            expression: self.bind_expression(
                                scope,
                                statements,
                                *root,
                                ExpressionContext::Value,
                            )?,
                        },
                    };
                    values.push(DrawingCase {
                        first,
                        last,
                        value,
                        span: case.span.clone(),
                    });
                }
                if !self.step(values.len(), span) {
                    return None;
                }
                Some(DrawingValue::According {
                    selector,
                    cases: values,
                })
            }
        }
    }

    fn drawing_enum_endpoint(
        &mut self,
        scope: usize,
        statements: &[Expression],
        root: ExprId,
        ty: usize,
    ) -> Option<ExpressionId> {
        let endpoint = self.bind_expression(scope, statements, root, ExpressionContext::Value)?;
        self.bind_enum_literal(endpoint, ty)?;
        Some(endpoint)
    }

    fn drawing_enum_range(
        &mut self,
        ty: usize,
        first: ExpressionId,
        last: ExpressionId,
        at: &Span,
    ) -> Option<()> {
        let endpoints = [first, last].map(|id| match &self.expressions[id.0 as usize].kind {
            crate::BoundExpressionKind::Enumeration { path } => path.clone(),
            crate::BoundExpressionKind::Boolean { value } => vec![value.to_string()],
            _ => Vec::new(),
        });
        if self.enum_is_ordered(ty)
            && self.ordered_enum_siblings(ty, &endpoints[0], &endpoints[1], at)?
        {
            Some(())
        } else {
            self.graphic_error(
                "ILI-GRAPHIC-ENUM-RANGE",
                "The range must select ordered sibling elements",
                at,
            )
        }
    }

    fn check_graphic_signature(
        &mut self,
        rule: usize,
        scope: usize,
        declared: usize,
        at: &Span,
    ) -> Option<()> {
        let mut inherited = Vec::new();
        let mut current = Some(rule);
        while let Some(rule) = current {
            if !self.step(1, at) {
                return None;
            }
            inherited.push(rule);
            current = self.symbols[rule].base_scope;
        }
        let mut clauses = Vec::new();
        for rule in inherited.into_iter().rev() {
            let authored = &self.symbols[rule].declaration.drawing_rule()?.clauses;
            let count = authored
                .iter()
                .map(|clause| clause.assignments.len() + 1)
                .sum();
            if !self.step(count, at) {
                return None;
            }
            clauses.extend_from_slice(&self.symbols[rule].declaration.drawing_rule()?.clauses);
        }
        let assigned: BTreeSet<_> = clauses
            .iter()
            .flat_map(|clause| &clause.assignments)
            .map(|assignment| assignment.parameter.declared_name())
            .collect();
        if !assigned.contains("Sign") {
            return self.graphic_error(
                "ILI-GRAPHIC-PARAMETER-MISSING",
                "A drawing rule must assign Sign",
                at,
            );
        }
        let mut contexts = vec![(None, None)];
        let mut earlier_sign_clauses = Vec::new();
        for (position, clause) in clauses.iter().enumerate() {
            if clause.condition.is_none() {
                continue;
            }
            contexts.push((Some(position), None));
            if clause
                .assignments
                .iter()
                .any(|assignment| assignment.parameter.declared_name() == "Sign")
            {
                earlier_sign_clauses.push(position);
            } else {
                if !self.step(earlier_sign_clauses.len(), at) {
                    return None;
                }
                contexts.extend(
                    earlier_sign_clauses
                        .iter()
                        .map(|&first| (Some(first), Some(position))),
                );
            }
        }
        for (first, second) in contexts {
            let mut values = BTreeMap::new();
            for (position, clause) in clauses.iter().enumerate() {
                if !self.step(1, &clause.span) {
                    return None;
                }
                if clause.condition.is_none() || first == Some(position) || second == Some(position)
                {
                    for assignment in &clause.assignments {
                        if !self.step(1, &assignment.span) {
                            return None;
                        }
                        values.insert(assignment.parameter.declared_name(), assignment);
                    }
                }
            }
            self.check_graphic_context(scope, declared, &values, &assigned, at)?;
        }
        Some(())
    }

    fn check_graphic_context(
        &mut self,
        scope: usize,
        declared: usize,
        values: &BTreeMap<&str, &DrawingAssignment>,
        assigned: &BTreeSet<&str>,
        at: &Span,
    ) -> Option<()> {
        let mut signs = Vec::new();
        if let Some(sign) = values.get("Sign") {
            self.drawing_objects(&sign.value, &mut signs, &sign.span)?;
        }
        let mut responsible = BTreeSet::new();
        if let Some(sign) = values.get("Sign")
            && signs.is_empty()
        {
            self.step(1, &sign.span).then_some(())?;
            responsible.insert(declared);
        }
        for actual in signs {
            let mut actual = self.metadata_object_class(actual)?;
            if !self.specializes_symbol(actual, declared, at)?
                && !self.specializes_symbol(declared, actual, at)?
            {
                return self.graphic_error(
                    "ILI-GRAPHIC-METAOBJECT-TYPE",
                    "The sign object class must belong to the declared signature's inheritance chain",
                    &values["Sign"].span,
                );
            }
            for assignment in values.values() {
                let parameter = self.names[assignment.parameter.target_name()];
                let owner = self.symbols[parameter].declaration.parent?;
                if self.specializes_symbol(actual, owner, &assignment.span)? {
                    continue;
                }
                if self.specializes_symbol(owner, actual, &assignment.span)?
                    && self.specializes_symbol(declared, owner, &assignment.span)?
                {
                    actual = owner;
                } else {
                    return self.graphic_error(
                        "ILI-GRAPHIC-SIGNATURE-RESPONSIBILITY",
                        "The assigned parameter requires an incompatible signature class",
                        &assignment.span,
                    );
                }
            }
            responsible.insert(actual);
        }
        for (name, assignment) in values {
            if let Some(parameter) = self
                .visible_name(declared, name, Some(&[Kind::Parameter]), &assignment.span)
                .ok()?
                && parameter != self.names[assignment.parameter.target_name()]
                && self.symbols[parameter]
                    .declaration
                    .type_id
                    .is_some_and(|ty| matches!(self.types[ty], TypeInfo::MetaObject(_)))
            {
                self.check_drawing_value(scope, parameter, &assignment.value, &assignment.span)?;
            }
        }
        for signature in responsible {
            let parameters = self.signature_parameters(signature, at)?;
            for (name, id) in &parameters {
                if self.symbols[*id].declaration.mandatory() == Some(true)
                    && !assigned.contains(name.as_str())
                {
                    let message =
                        format!("The drawing rule must assign the signature parameter {name}");
                    return self.graphic_error("ILI-GRAPHIC-PARAMETER-MISSING", &message, at);
                }
            }
            for (&name, assignment) in values {
                let original = self.names[assignment.parameter.target_name()];
                let parameter = parameters.get(name).copied().unwrap_or(original);
                if parameter != original {
                    self.check_drawing_value(
                        scope,
                        parameter,
                        &assignment.value,
                        &assignment.span,
                    )?;
                }
                if name != "Sign"
                    && self.symbols[parameter]
                        .declaration
                        .type_id
                        .is_some_and(|ty| matches!(self.types[ty], TypeInfo::MetaObject(None)))
                {
                    let mut objects = Vec::new();
                    self.drawing_objects(&assignment.value, &mut objects, &assignment.span)?;
                    for object in objects {
                        if !self.specializes_symbol(
                            self.metadata_object_class(object)?,
                            signature,
                            object.source_span(),
                        )? {
                            return self.graphic_error(
                                "ILI-GRAPHIC-METAOBJECT-TYPE",
                                "The metadata object must specialize the responsible signature class",
                                object.source_span(),
                            );
                        }
                    }
                }
            }
        }
        Some(())
    }

    fn signature_parameters(
        &mut self,
        signature: usize,
        at: &Span,
    ) -> Option<BTreeMap<String, usize>> {
        let mut result = BTreeMap::new();
        let mut owner = Some(signature);
        while let Some(id) = owner {
            let members = self
                .scope_names
                .range((id, Namespace::Member, String::new())..)
                .take_while(|((scope, space, _), _)| *scope == id && *space == Namespace::Member);
            let count = members.clone().count();
            if self
                .work
                .charge(u32::try_from(count + 1).unwrap_or(u32::MAX))
                .is_err()
            {
                self.diagnostics
                    .limit("Semantic work budget exceeded", Some(at.clone()));
                return None;
            }
            for ((_, _, name), &member) in members {
                if self.symbols[member].declaration.kind == Kind::Parameter {
                    result.entry(name.clone()).or_insert(member);
                }
            }
            owner = self.symbols[id].base_scope;
        }
        Some(result)
    }

    fn drawing_objects<'a>(
        &mut self,
        value: &'a DrawingValue,
        result: &mut Vec<&'a TypeReference>,
        at: &Span,
    ) -> Option<()> {
        if !self.step(1, at) {
            return None;
        }
        match value {
            DrawingValue::MetaObject { reference } => result.push(reference),
            DrawingValue::According { cases, .. } => {
                for case in cases {
                    self.drawing_objects(&case.value, result, &case.span)?;
                }
            }
            DrawingValue::Expression { .. } => {}
        }
        Some(())
    }

    fn check_drawing_value(
        &mut self,
        scope: usize,
        parameter: usize,
        value: &DrawingValue,
        at: &Span,
    ) -> Option<()> {
        let declaration = &self.symbols[parameter].declaration;
        let ty = declaration.type_id?;
        let mandatory = declaration.mandatory() == Some(true);
        let fits = match value {
            DrawingValue::Expression { expression } => self.value_fits(
                scope,
                *expression,
                ty,
                mandatory,
                AssignmentContext::Attribute,
            )?,
            DrawingValue::MetaObject { reference } => {
                if let TypeInfo::MetaObject(target) = &self.types[ty] {
                    let actual = self.metadata_object_class(reference)?;
                    if let Some(target) = target {
                        self.specializes_symbol(
                            actual,
                            self.names[target.target_name()],
                            reference.source_span(),
                        )?
                    } else {
                        true
                    }
                } else {
                    false
                }
            }
            DrawingValue::According { cases, .. } => {
                for case in cases {
                    self.check_drawing_value(scope, parameter, &case.value, &case.span)?;
                }
                true
            }
        };
        if fits {
            Some(())
        } else {
            self.graphic_error(
                "ILI-GRAPHIC-PARAMETER-TYPE",
                "The value cannot be assigned to the signature parameter",
                at,
            )
        }
    }

    fn metadata_object_class(&self, object: &TypeReference) -> Option<usize> {
        let id = self.names[object.target_name()];
        let class = self.symbols[id].declaration.type_reference()?;
        self.names.get(class.target_name()).copied()
    }

    fn graphic_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}
