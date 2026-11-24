use super::{
    Analyzer, BoundName, Kind, LookupError, Namespace, QualifiedName, SyntaxDecl,
    collections::structure_targets, domains::scalar_extension, reference_systems::reference_ranges,
};
use crate::{ContextMapping, RelatedLocation, Span, TopicGenericBinding, TypeInfo, TypeReference};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Alternatives {
    domains: Vec<usize>,
    origins: BTreeSet<usize>,
}

type ContextAlternatives = BTreeMap<usize, Alternatives>;
type TypeUse = (usize, Option<usize>);

#[derive(Clone, Copy)]
enum Axes {
    All,
    Index(usize),
    Direction,
}

#[derive(Clone)]
struct ReferenceUse {
    target: usize,
    axes: Axes,
    source: Option<TypeUse>,
    location: Span,
}

impl Analyzer<'_, '_> {
    pub(super) fn bind_contexts(
        &mut self,
        model_order: &[usize],
        topics: &[usize],
        viewables: &[usize],
    ) -> Option<()> {
        let mut declared = vec![Vec::new(); self.models.len()];
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::Context(syntax)) = self.symbols[id].ast else {
                continue;
            };
            if let Some(owner) = self.symbols[id].declaration.parent
                && let Some(&other) =
                    self.scope_names
                        .get(&(owner, Namespace::Type, syntax.name.value.clone()))
            {
                self.diagnostics.push_related(
                    "ILI-CONTEXT-NAME",
                    "A context name must be distinct from type names in its model",
                    Some(syntax.name.span.clone()),
                    vec![RelatedLocation {
                        span: self.symbols[other].name_span.clone(),
                        message: "Conflicting model type name".into(),
                    }],
                );
                return None;
            }
            let mut mappings = Vec::new();
            let mut used = BTreeSet::new();
            for mapping in &syntax.bindings {
                let (generic, reference) = self.context_domain(id, &mapping.generic)?;
                if !self.symbols[generic].declaration.is_generic() {
                    self.diagnostics.push(
                        "ILI-CONTEXT-GENERIC",
                        "CONTEXT requires a generic coordinate domain",
                        Some(reference.span.clone()),
                    );
                    return None;
                }
                if !used.insert(generic) {
                    self.diagnostics.push(
                        "ILI-CONTEXT-DUPLICATE",
                        "A context may bind a generic domain only once",
                        Some(reference.span.clone()),
                    );
                    return None;
                }
                let mut alternatives = Vec::new();
                let mut seen = BTreeSet::new();
                for name in &mapping.concrete {
                    let (concrete, link) = self.context_domain(id, name)?;
                    if !seen.insert(concrete) {
                        self.diagnostics.push(
                            "ILI-CONTEXT-DUPLICATE",
                            "A concrete alternative may appear only once",
                            Some(name.span.clone()),
                        );
                        return None;
                    }
                    self.check_context_alternative(id, generic, concrete, &link.span)?;
                    alternatives.push(link);
                }
                mappings.push(ContextMapping {
                    generic: reference,
                    alternatives,
                });
            }
            declared[self.symbols[id].model].push(id);
            self.symbols[id].declaration.context = Some(mappings.into_boxed_slice());
        }
        if self.generic_types.is_empty() && !topics.iter().any(|&id| {
            matches!(self.symbols[id].ast, Some(SyntaxDecl::Topic { options, .. }) if !options.deferred_generics.is_empty())
        }) {
            return Some(());
        }

        let mut alternatives = vec![ContextAlternatives::new(); self.models.len()];
        let mut visible = vec![BTreeSet::new(); self.models.len()];
        for &model in model_order {
            let span = self.selected_models[model].name.span.clone();
            let imports: Vec<_> = self.models[model]
                .imports()
                .iter()
                .filter_map(|name| self.model_names.get(name).copied())
                .collect();
            let mut effective = ContextAlternatives::new();
            visible[model].insert(model);
            for imported in imports {
                let [own, incoming] = visible
                    .get_disjoint_mut([model, imported])
                    .expect("imports are acyclic");
                if !self.step(incoming.len(), &span) {
                    return None;
                }
                own.extend(incoming.iter().copied());
                for (&generic, candidates) in &alternatives[imported] {
                    if !self.step(candidates.domains.len() + candidates.origins.len(), &span) {
                        return None;
                    }
                    if let Some(previous) = effective.get_mut(&generic) {
                        let joint: BTreeSet<_> = previous
                            .domains
                            .iter()
                            .chain(&candidates.domains)
                            .copied()
                            .collect();
                        let mut intersection = Vec::new();
                        for candidate in joint {
                            if self.refines_context(candidate, &previous.domains, &span)?
                                && self.refines_context(candidate, &candidates.domains, &span)?
                            {
                                intersection.push(candidate);
                            }
                        }
                        if intersection.is_empty() {
                            self.diagnostics.push(
                                "ILI-CONTEXT-CONFLICT",
                                "Imported contexts have no compatible alternative",
                                Some(span),
                            );
                            return None;
                        }
                        previous.domains = intersection;
                        previous.origins.extend(candidates.origins.iter().copied());
                    } else {
                        effective.insert(generic, candidates.clone());
                    }
                }
            }
            for &context in &declared[model] {
                let span = self.symbols[context].name_span.clone();
                let mappings: Vec<_> = self.symbols[context]
                    .declaration
                    .context_mappings()
                    .iter()
                    .map(|mapping| {
                        (
                            self.names[mapping.generic.target_name()],
                            mapping
                                .alternatives
                                .iter()
                                .map(|candidate| self.names[candidate.target_name()])
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                for (generic, candidates) in mappings {
                    if let Some(previous) = effective.get(&generic) {
                        for &candidate in &candidates {
                            if !self.refines_context(candidate, &previous.domains, &span)? {
                                self.diagnostics.push(
                                    "ILI-CONTEXT-EXTENSION",
                                    "A context override must narrow the existing alternatives",
                                    Some(span),
                                );
                                return None;
                            }
                        }
                    }
                    effective.insert(
                        generic,
                        Alternatives {
                            domains: candidates,
                            origins: BTreeSet::from([context]),
                        },
                    );
                }
            }
            alternatives[model] = effective;
        }
        self.bind_topic_generics(topics, viewables, &alternatives, &visible)
    }

    fn context_domain(
        &mut self,
        owner: usize,
        name: &QualifiedName,
    ) -> Option<(usize, TypeReference)> {
        match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Domain]) {
            Ok(target @ BoundName::Declared(id)) => {
                Some((id, self.bound_reference(owner, name, target)?))
            }
            Err(LookupError::Limit) => None,
            _ => {
                self.diagnostics.push(
                    "ILI-CONTEXT-DOMAIN",
                    "A context requires a supplied coordinate domain",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    fn check_context_alternative(
        &mut self,
        owner: usize,
        generic: usize,
        concrete: usize,
        span: &Span,
    ) -> Option<()> {
        let base_type = self.symbols[generic].declaration.type_id?;
        let target_type = self.symbols[concrete].declaration.type_id?;
        let TypeInfo::Coordinate(base) = &self.types[base_type] else {
            unreachable!("checked generic domain")
        };
        let TypeInfo::Coordinate(target) = &self.types[target_type] else {
            self.diagnostics.push(
                "ILI-CONTEXT-CONCRETE",
                "A context alternative must be concrete coordinates",
                Some(span.clone()),
            );
            return None;
        };
        let declaration = &self.symbols[concrete].declaration;
        if declaration.is_generic()
            || declaration.is_abstract()
            || self.types[target_type].has_unbounded_range()
        {
            self.diagnostics.push(
                "ILI-CONTEXT-CONCRETE",
                "A context alternative must be concrete coordinates",
                Some(span.clone()),
            );
            return None;
        }
        if base.axes.len() != target.axes.len() || base.kind != target.kind {
            self.diagnostics.push(
                "ILI-CONTEXT-DIMENSIONS",
                "A context alternative must preserve the generic coordinate dimensions and kind",
                Some(span.clone()),
            );
            return None;
        }
        let check = self
            .check_unit_specialization(target_type, base_type, false)
            .and_then(|units| {
                if units.is_some() {
                    Ok(units)
                } else {
                    scalar_extension(
                        &self.types[target_type],
                        &self.types[base_type],
                        self.models[self.symbols[owner].model].language,
                        self.limits,
                        &mut self.work,
                        false,
                    )
                }
            });
        match check {
            Ok(None) => Some(()),
            Ok(Some(_)) => {
                self.diagnostics.push(
                    "ILI-CONTEXT-RANGE",
                    "A context alternative must specialize the generic coordinate values",
                    Some(span.clone()),
                );
                None
            }
            Err(error) => {
                self.number_error(error, span);
                None
            }
        }
    }

    fn refines_context(&mut self, actual: usize, base: &[usize], span: &Span) -> Option<bool> {
        let mut candidate = Some(actual);
        while let Some(id) = candidate {
            if !self.step(base.len() + 1, span) {
                return None;
            }
            if base.contains(&id) {
                return Some(true);
            }
            candidate = self.symbols[id]
                .declaration
                .base_reference()
                .and_then(|reference| self.names.get(reference.target_name()))
                .copied();
        }
        Some(false)
    }

    fn bind_topic_generics(
        &mut self,
        topics: &[usize],
        viewables: &[usize],
        alternatives: &[ContextAlternatives],
        visible: &[BTreeSet<usize>],
    ) -> Option<()> {
        let mut direct_viewables = vec![Vec::new(); self.symbols.len()];
        let mut derived_structures = vec![Vec::new(); self.symbols.len()];
        for &id in viewables {
            let symbol = &self.symbols[id];
            if symbol.declaration.kind == Kind::Structure {
                if let Some(base) = symbol.base_scope {
                    derived_structures[base].push(id);
                }
            } else if let Some(topic) = self.containing_topic(id) {
                direct_viewables[topic].push(id);
            }
        }
        let mut deferred = vec![BTreeSet::new(); self.symbols.len()];
        for &topic in topics {
            let Some(SyntaxDecl::Topic { options, .. }) = self.symbols[topic].ast else {
                unreachable!()
            };
            let span = self.symbols[topic].name_span.clone();
            let model = self.symbols[topic].model;
            let used = self.topic_generics(
                topic,
                &direct_viewables,
                &derived_structures,
                &visible[model],
                &alternatives[model],
                &span,
            )?;
            if let Some(base) = self.symbols[topic].base_scope {
                if !self.step(deferred[base].len(), &span) {
                    return None;
                }
                deferred[topic] = deferred[base].clone();
            }
            let mut local_deferred = Vec::new();
            for reference in &options.deferred_generics {
                let (id, reference) = self.context_domain(topic, reference)?;
                let error = if !self.symbols[id].declaration.is_generic() {
                    Some((
                        "ILI-GENERIC-DEFERRED-DOMAIN",
                        "DEFERRED GENERICS must name a generic coordinate domain",
                    ))
                } else if !alternatives[model].contains_key(&id) {
                    Some((
                        "ILI-GENERIC-CONTEXT-MISSING",
                        "A deferred generic domain needs an effective context",
                    ))
                } else if !used.contains(&id) {
                    Some((
                        "ILI-GENERIC-DEFERRED-UNUSED",
                        "The topic does not use this deferred generic domain",
                    ))
                } else {
                    None
                };
                if let Some((code, message)) = error {
                    self.diagnostics.push(code, message, Some(reference.span));
                    return None;
                }
                deferred[topic].insert(id);
                local_deferred.push(reference);
            }
            let mut bindings = Vec::new();
            for generic in used {
                let selection = alternatives[model].get(&generic);
                let choice = selection.map_or(&[][..], |selection| selection.domains.as_slice());
                if choice.is_empty() && !self.symbols[topic].declaration.is_abstract() {
                    self.diagnostics.push(
                        "ILI-GENERIC-CONTEXT-MISSING",
                        "A concrete topic needs a context for each generic domain it uses",
                        Some(span),
                    );
                    return None;
                }
                if choice.len() > 1 && !deferred[topic].contains(&generic) {
                    self.diagnostics.push(
                        "ILI-GENERIC-DEFERRED-REQUIRED",
                        "Multiple context alternatives require DEFERRED GENERICS",
                        Some(span),
                    );
                    return None;
                }
                let bytes = choice
                    .iter()
                    .chain(std::iter::once(&generic))
                    .chain(
                        selection
                            .into_iter()
                            .flat_map(|selection| &selection.origins),
                    )
                    .map(|&id| self.symbols[id].declaration.qualified.len() + 1)
                    .sum();
                if !self.step(bytes, &span) {
                    return None;
                }
                bindings.push(TopicGenericBinding {
                    generic_domain: self.symbols[generic].declaration.qualified.clone(),
                    concrete_domains: choice
                        .iter()
                        .map(|&id| self.symbols[id].declaration.qualified.clone())
                        .collect(),
                    contexts: selection
                        .into_iter()
                        .flat_map(|selection| &selection.origins)
                        .map(|&id| self.symbols[id].declaration.qualified.clone())
                        .collect(),
                    deferred: choice.len() > 1,
                });
            }
            if !bindings.is_empty() {
                self.symbols[topic].declaration.generic_bindings =
                    Some(bindings.into_boxed_slice());
            }
            if !local_deferred.is_empty() {
                self.symbols[topic].declaration.deferred_generics =
                    Some(local_deferred.into_boxed_slice());
            }
        }
        Some(())
    }

    fn follow_context_references(
        &mut self,
        root: ReferenceUse,
        alternatives: &ContextAlternatives,
        used: &mut BTreeSet<usize>,
        topic: &Span,
    ) -> Option<()> {
        // Discover dependencies and validate them along the same branch. An
        // alternative remains selected until every axis reached from it returns.
        type Axis = (usize, usize);
        enum Walk {
            Follow(ReferenceUse),
            Read(Axis, Span),
            Leave(Axis),
            Select {
                generic: usize,
                candidate: usize,
                reference: ReferenceUse,
                remember: bool,
            },
            Unselect(usize),
        }

        let mut pending = vec![Walk::Follow(root)];
        let mut selected = BTreeMap::new();
        let mut active = BTreeSet::new();
        while let Some(event) = pending.pop() {
            match event {
                Walk::Follow(reference) => {
                    if !self.step(1, &reference.location) {
                        return None;
                    }
                    if let Some(&generic) = self.generic_types.get(&reference.target) {
                        used.insert(generic);
                        if let Some(&candidate) = selected.get(&generic) {
                            pending.push(Walk::Select {
                                generic,
                                candidate,
                                reference,
                                remember: false,
                            });
                        } else if let Some(choices) = alternatives.get(&generic) {
                            if !self.step(choices.domains.len(), &reference.location) {
                                return None;
                            }
                            for &candidate in choices.domains.iter().rev() {
                                pending.push(Walk::Select {
                                    generic,
                                    candidate,
                                    reference: reference.clone(),
                                    remember: true,
                                });
                            }
                        }
                    } else {
                        for index in
                            (0..reference_ranges(&self.types[reference.target]).len()).rev()
                        {
                            if matches!(reference.axes, Axes::All)
                                || matches!(reference.axes, Axes::Index(axis) if axis == index)
                            {
                                pending.push(Walk::Read(
                                    (reference.target, index),
                                    reference.location.clone(),
                                ));
                            }
                        }
                    }
                }
                Walk::Select {
                    generic,
                    candidate,
                    mut reference,
                    remember,
                } => {
                    if remember {
                        selected.insert(generic, candidate);
                        pending.push(Walk::Unselect(generic));
                    }
                    if let Some(source) = reference.source.take() {
                        self.validate_context_use(source, generic, candidate, topic)?;
                    }
                    reference.target = self.symbols[candidate].declaration.type_id?;
                    pending.push(Walk::Follow(reference));
                }
                Walk::Unselect(generic) => {
                    selected.remove(&generic);
                }
                Walk::Leave(node) => {
                    active.remove(&node);
                }
                Walk::Read(node, at) => {
                    if !self.step(1, &at) {
                        return None;
                    }
                    if !active.insert(node) {
                        self.diagnostics.push_related(
                            "ILI-REFSYSTEM-CYCLE",
                            "Selecting coordinate alternatives closes a reference-system cycle",
                            Some(at),
                            selected
                                .values()
                                .take(8)
                                .map(|&alternative| RelatedLocation {
                                    span: self.symbols[alternative].name_span.clone(),
                                    message: "Selected coordinate alternative".into(),
                                })
                                .collect(),
                        );
                        return None;
                    }
                    pending.push(Walk::Leave(node));
                    let Some(reference) =
                        &reference_ranges(&self.types[node.0])[node.1].reference_system
                    else {
                        continue;
                    };
                    let Some(target) = self.context_type(&reference.domain) else {
                        continue;
                    };
                    let axes = match reference.axis {
                        Some(axis) => Axes::Index(usize::from(axis - 1)),
                        None if matches!(self.types[target], TypeInfo::Numeric(_)) => Axes::All,
                        None => Axes::Direction,
                    };
                    pending.push(Walk::Follow(ReferenceUse {
                        target,
                        axes,
                        source: Some((node.0, Some(node.1))),
                        location: reference.domain.span.clone(),
                    }));
                }
            }
        }
        Some(())
    }

    fn topic_generics(
        &mut self,
        topic: usize,
        direct: &[Vec<usize>],
        derived_structures: &[Vec<usize>],
        visible_models: &BTreeSet<usize>,
        alternatives: &ContextAlternatives,
        span: &Span,
    ) -> Option<BTreeSet<usize>> {
        let mut scope = Some(topic);
        let mut containers = Vec::new();
        let mut visible_topics = BTreeSet::new();
        let mut names = BTreeSet::new();
        while let Some(current) = scope {
            visible_topics.insert(current);
            for &id in &direct[current] {
                let name = self.symbols[id].declaration.name.clone();
                if !self.step(name.len() + 1, span) {
                    return None;
                }
                if names.insert(name) {
                    containers.push(id);
                }
            }
            scope = self.symbols[current].base_scope;
        }
        let mut checked_containers = BTreeSet::new();
        let mut checked_types = BTreeSet::new();
        let mut generic_domains = BTreeSet::new();
        let mut expanded_any = false;
        while let Some(container) = containers.pop() {
            if !self.step(1, span) {
                return None;
            }
            if !checked_containers.insert(container) {
                continue;
            }
            let mut types: Vec<_> = self.symbols[container]
                .declaration
                .effective_attributes
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|&id| self.symbols[id].declaration.type_id)
                .collect();
            while let Some(id) = types.pop() {
                if !self.step(1, span) {
                    return None;
                }
                if !checked_types.insert(id) {
                    continue;
                }
                match &self.types[id] {
                    TypeInfo::Collection(collection) => types.push(collection.element),
                    TypeInfo::Structure(structure) => {
                        let targets = structure_targets(structure);
                        if targets.is_empty() && !expanded_any {
                            expanded_any = true;
                            if !self.step(self.symbols.len(), span) {
                                return None;
                            }
                            containers.extend(self.symbols.iter().enumerate().filter_map(
                                |(id, symbol)| {
                                    (symbol.declaration.kind == Kind::Structure
                                        && visible_models.contains(&symbol.model)
                                        && self
                                            .containing_topic(id)
                                            .is_none_or(|scope| visible_topics.contains(&scope)))
                                    .then_some(id)
                                },
                            ));
                        } else {
                            containers.extend(
                                targets
                                    .iter()
                                    .map(|reference| self.names[reference.target_name()]),
                            );
                        }
                    }
                    TypeInfo::Geometry(geometry) => {
                        if let Some(vertex) = &geometry.vertex
                            && let Some(target) = self.context_type(vertex)
                        {
                            self.follow_context_references(
                                ReferenceUse {
                                    target,
                                    axes: Axes::All,
                                    source: Some((id, None)),
                                    location: vertex.span.clone(),
                                },
                                alternatives,
                                &mut generic_domains,
                                span,
                            )?;
                        }
                    }
                    ty if !reference_ranges(ty).is_empty() => {
                        self.follow_context_references(
                            ReferenceUse {
                                target: id,
                                axes: Axes::All,
                                source: None,
                                location: span.clone(),
                            },
                            alternatives,
                            &mut generic_domains,
                            span,
                        )?;
                    }
                    _ => {}
                }
            }
            let owner_scope = self.containing_topic(container);
            for &specialization in &derived_structures[container] {
                if !self.step(1, span) {
                    return None;
                }
                let scope = self.containing_topic(specialization);
                if visible_models.contains(&self.symbols[specialization].model)
                    && (scope == owner_scope
                        || scope.is_none()
                        || scope.is_some_and(|id| visible_topics.contains(&id)))
                {
                    containers.push(specialization);
                }
            }
        }
        Some(generic_domains)
    }

    fn context_type(&self, reference: &TypeReference) -> Option<usize> {
        let &id = self.names.get(reference.target_name())?;
        self.symbols[id].declaration.type_id
    }
}
