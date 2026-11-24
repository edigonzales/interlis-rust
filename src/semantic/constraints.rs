use super::{
    Analyzer, BoundName, Kind, QualifiedName, SyntaxDecl, TypeInfo,
    expressions::{ExpressionContext, scalar_type},
    paths::PathContext,
};
use crate::{
    BoundConstraint, BoundExpressionKind, BoundPathStep, ConstraintRule, ExpressionId,
    ExpressionType as ValueType, Span,
    syntax::{ConstraintKind, ConstraintSyntax, ExprId, Expression, UniqueKey},
};
use std::collections::BTreeMap;

impl Analyzer<'_, '_> {
    pub(super) fn expression_topic(&self, owner: usize) -> Option<usize> {
        self.constraint_topic
            .or_else(|| self.containing_topic(owner))
    }

    pub(super) fn bind_constraints(
        &mut self,
        documents: &BTreeMap<u32, &[Expression]>,
    ) -> Option<()> {
        let mut attached = BTreeMap::new();
        let mut inherited = BTreeMap::new();
        for index in 0..self.constraint_blocks.len() {
            let (topic, block) = self.constraint_blocks[index];
            let kinds: &[_] = match self.expression_language(topic) {
                crate::LanguageVersion::V23 => &[Kind::Class, Kind::Association],
                crate::LanguageVersion::V24 => {
                    &[Kind::Class, Kind::Association, Kind::Structure, Kind::View]
                }
            };
            let local = if let [name] = block.target.segments.as_slice() {
                self.visible_name(topic, &name.value, Some(kinds), &name.span)
                    .ok()?
            } else {
                None
            };
            let target = local.map(BoundName::Declared).or_else(|| {
                self.expression_name(topic, &block.target, kinds, "ILI-CONSTRAINT-TARGET")
            });
            let Some(target) = target else {
                if self.diagnostics.limited {
                    return None;
                }
                continue;
            };
            let BoundName::Declared(target) = target else {
                self.diagnostics.push(
                    "ILI-CONSTRAINT-TARGET-TOPIC",
                    "External constraints belong in the target's topic",
                    Some(block.target.span.clone()),
                );
                continue;
            };
            let target_topic = self.containing_topic(target);
            if !self.step(block.constraints.len(), &block.span) {
                return None;
            }
            let constraints = block
                .constraints
                .iter()
                .map(|constraint| (constraint, Some(&block.target)));
            if target_topic == Some(topic) {
                attached
                    .entry(target)
                    .or_insert_with(Vec::new)
                    .extend(constraints);
            } else if let Some(base) = target_topic
                && self.specializes_symbol(topic, base, &block.target.span)?
            {
                inherited
                    .entry((topic, target))
                    .or_insert_with(Vec::new)
                    .extend(constraints);
            } else {
                self.diagnostics.push(
                    "ILI-CONSTRAINT-TARGET-TOPIC",
                    "External constraints belong in the target's topic or a topic extending it",
                    Some(block.target.span.clone()),
                );
            }
        }
        for owner in 0..self.symbols.len() {
            let external = attached.remove(&owner).unwrap_or_default();
            self.bind_local_constraints(owner, documents, external)?;
        }
        let mut by_topic = BTreeMap::new();
        for ((topic, target), constraints) in inherited {
            self.constraint_topic = Some(topic);
            let result = self.bind_named_constraints(
                target,
                documents,
                constraints.into_iter(),
                by_topic.entry(topic).or_insert_with(Vec::new),
            );
            self.constraint_topic = None;
            result?;
        }
        for (topic, mut constraints) in by_topic {
            constraints.sort_by_key(|constraint| constraint.source_span().sort_key());
            self.symbols[topic].declaration.constraints = Some(constraints.into_boxed_slice());
        }
        Some(())
    }

    pub(super) fn bind_local_constraints<'syntax>(
        &mut self,
        owner: usize,
        documents: &BTreeMap<u32, &[Expression]>,
        external: Vec<(&'syntax ConstraintSyntax, Option<&'syntax QualifiedName>)>,
    ) -> Option<()> {
        let mut bound = Vec::new();
        let inline: &[ConstraintSyntax] = match self.symbols[owner].ast {
            Some(SyntaxDecl::Container { constraints, .. }) => constraints,
            Some(SyntaxDecl::Association(association)) => &association.constraints,
            Some(SyntaxDecl::View(view)) => &view.constraints,
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
                        Some(name),
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
                &[]
            }
            _ => &[],
        };
        if !inline.is_empty() || !external.is_empty() {
            self.bind_named_constraints(
                owner,
                documents,
                inline
                    .iter()
                    .map(|constraint| (constraint, None))
                    .chain(external),
                &mut bound,
            )?;
        }
        if !bound.is_empty() {
            self.symbols[owner].declaration.constraints = Some(bound.into_boxed_slice());
        }
        Some(())
    }

    fn bind_named_constraints<'syntax>(
        &mut self,
        owner: usize,
        documents: &BTreeMap<u32, &[Expression]>,
        syntax: impl Iterator<Item = (&'syntax ConstraintSyntax, Option<&'syntax QualifiedName>)>,
        bound: &mut Vec<BoundConstraint>,
    ) -> Option<()> {
        let mut names = BTreeMap::new();
        for (constraint, external_target) in syntax {
            if let Some(name) = &constraint.name {
                if !self.step(1 + name.value.len(), &name.span) {
                    return None;
                }
                if let Some(first) = names.insert(name.value.as_str(), &name.span) {
                    self.duplicate(&name.span, first);
                    continue;
                }
            }
            let target = if let Some(name) = external_target {
                Some(self.bound_reference(
                    self.constraint_topic.unwrap_or(owner),
                    name,
                    BoundName::Declared(owner),
                )?)
            } else {
                None
            };
            let expressions = documents[&constraint.span.sort_key().0];
            let (rule, root) = match &constraint.kind {
                ConstraintKind::Existence { path, required } => {
                    if let Some(mut existence) =
                        self.bind_existence(owner, constraint, path, required)
                    {
                        existence.target = target;
                        bound.push(existence);
                    }
                    if self.diagnostics.limited {
                        return None;
                    }
                    continue;
                }
                ConstraintKind::Unique {
                    basket,
                    filter,
                    key,
                } => {
                    if let Some(mut unique) = self.bind_unique(
                        owner,
                        expressions,
                        constraint,
                        basket.as_ref(),
                        *filter,
                        key,
                    ) {
                        unique.target = target;
                        bound.push(unique);
                    }
                    if self.diagnostics.limited {
                        return None;
                    }
                    continue;
                }
                ConstraintKind::Set {
                    basket,
                    filter,
                    expression,
                } => {
                    if !matches!(
                        self.symbols[owner].declaration.kind,
                        Kind::Class | Kind::Association
                    ) {
                        self.diagnostics.push(
                            "ILI-SET-CONTEXT",
                            "A SET constraint requires a class or association",
                            Some(constraint.span.clone()),
                        );
                        continue;
                    }
                    let filter = if let Some(root) = filter {
                        let Some(filter) = self.bind_boolean_expression(
                            owner,
                            expressions,
                            *root,
                            ExpressionContext::Value,
                        ) else {
                            if self.diagnostics.limited {
                                return None;
                            }
                            continue;
                        };
                        Some(filter)
                    } else {
                        None
                    };
                    (
                        ConstraintRule::Set {
                            basket: basket.is_some(),
                            filter,
                        },
                        *expression,
                    )
                }
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
            };
            if let Some(mut constraint) = self.bind_constraint(
                owner,
                expressions,
                constraint.name.as_ref(),
                rule,
                root,
                &constraint.span,
            ) {
                constraint.target = target;
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
        name: Option<&crate::syntax::Name>,
        rule: ConstraintRule,
        root: ExprId,
        span: &Span,
    ) -> Option<BoundConstraint> {
        let context = match rule {
            ConstraintRule::Set { .. } => ExpressionContext::Set,
            ConstraintRule::Mandatory | ConstraintRule::Plausibility { .. } => {
                ExpressionContext::IndividualConstraint
            }
            _ => ExpressionContext::Value,
        };
        let expression = self.bind_boolean_expression(owner, expressions, root, context)?;
        Some(BoundConstraint {
            name: name.map(|name| name.value.clone()),
            target: None,
            rule,
            expression: Some(expression),
            span: span.clone(),
            source_metadata: self.source_metadata(
                span,
                name.map_or(span, |name| &name.span),
                false,
            )?,
        })
    }

    fn bind_boolean_expression(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        root: ExprId,
        context: ExpressionContext,
    ) -> Option<ExpressionId> {
        let expression = self.bind_expression(owner, expressions, root, context)?;
        let bound = &self.expressions[expression.0 as usize];
        if bound.value_type != ValueType::Boolean {
            self.diagnostics.push(
                "ILI-CONSTRAINT-BOOLEAN",
                "A constraint requires a Boolean expression",
                Some(bound.span.clone()),
            );
            return None;
        }
        Some(expression)
    }

    fn bind_unique(
        &mut self,
        owner: usize,
        expressions: &[Expression],
        constraint: &ConstraintSyntax,
        basket: Option<&Span>,
        filter: Option<ExprId>,
        key: &UniqueKey,
    ) -> Option<BoundConstraint> {
        let mut keys = Vec::new();
        let local_prefix = match key {
            UniqueKey::Global(paths) => {
                if self.symbols[owner].declaration.kind == Kind::Structure {
                    self.diagnostics.push(
                        "ILI-UNIQUE-SCOPE",
                        "A global UNIQUE constraint requires an identifiable object",
                        Some(constraint.span.clone()),
                    );
                    return None;
                }
                for path in paths {
                    let (kind, value_type, ty) = self.expression_path(owner, path)?;
                    self.reject_structure_key(ty, &path.span)?;
                    keys.push(self.record_expression(kind, value_type, ty, &path.span));
                }
                None
            }
            UniqueKey::Local {
                prefix,
                attributes,
                prefix_span,
            } => {
                if let Some(basket) = basket {
                    self.diagnostics.push(
                        "ILI-UNIQUE-SCOPE",
                        "A LOCAL unique key cannot also be BASKET scoped",
                        Some(basket.clone()),
                    );
                    return None;
                }
                let topic = self.expression_topic(owner);
                let language = self.expression_language(owner);
                let mut scope = self.expression_object(owner, &constraint.span)?;
                let mut steps = Vec::with_capacity(prefix.len());
                let mut prefix_type = None;
                for name in prefix {
                    let member = self.path_attribute(scope, name)?;
                    let ty = self.symbols[member].declaration.type_id?;
                    let reference = self.reference_to_symbol(owner, member, name)?;
                    steps.push(BoundPathStep {
                        reference,
                        selection: None,
                        selection_span: None,
                        association_access: None,
                    });
                    prefix_type = Some(ty);
                    scope = self.path_scope(
                        ty,
                        topic,
                        PathContext::UniquePrefix(language),
                        &name.span,
                    )?;
                }
                let value_type = scalar_type(&self.types[prefix_type?]);
                let path = self.record_expression(
                    BoundExpressionKind::Path { steps },
                    value_type,
                    prefix_type,
                    prefix_span,
                );
                for name in attributes {
                    let member = self.path_attribute(scope, name)?;
                    let ty = self.symbols[member].declaration.type_id?;
                    self.reject_structure_key(Some(ty), &name.span)?;
                    let reference = self.reference_to_symbol(owner, member, name)?;
                    let step = BoundPathStep {
                        reference,
                        selection: None,
                        selection_span: None,
                        association_access: None,
                    };
                    keys.push(self.record_expression(
                        BoundExpressionKind::Path { steps: vec![step] },
                        scalar_type(&self.types[ty]),
                        Some(ty),
                        &name.span,
                    ));
                }
                Some(path)
            }
        };
        let expression = match filter {
            Some(filter) => Some(self.bind_boolean_expression(
                owner,
                expressions,
                filter,
                ExpressionContext::Value,
            )?),
            None => None,
        };
        Some(BoundConstraint {
            name: constraint.name.as_ref().map(|name| name.value.clone()),
            target: None,
            rule: ConstraintRule::Unique {
                basket: basket.is_some(),
                local_prefix,
                keys,
            },
            expression,
            span: constraint.span.clone(),
            source_metadata: self.source_metadata(
                &constraint.span,
                constraint
                    .name
                    .as_ref()
                    .map_or(&constraint.span, |name| &name.span),
                false,
            )?,
        })
    }

    fn reject_structure_key(&mut self, ty: Option<usize>, at: &Span) -> Option<()> {
        if ty.is_some_and(|ty| {
            matches!(
                self.types[ty],
                TypeInfo::Structure(_) | TypeInfo::Collection(_)
            ) || matches!(&self.types[ty], TypeInfo::ObjectSelection(selection) if selection.plural)
        }) {
            self.diagnostics.push(
                "ILI-UNIQUE-KEY",
                "A UNIQUE key must select a single value",
                Some(at.clone()),
            );
            return None;
        }
        Some(())
    }
}
