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
                } else if !used.contains_key(&id) {
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
            for (generic, type_uses) in used {
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
                for &alternative in choice {
                    let selected = self.symbols[alternative].declaration.type_id?;
                    self.check_context_reference_cycles(
                        selected,
                        (generic, alternative),
                        &alternatives[model],
                        &span,
                    )?;
                    for &type_use in &type_uses {
                        self.validate_context_use(type_use, generic, alternative, &span)?;
                        self.check_context_reference_cycles(
                            type_use.0,
                            (generic, alternative),
                            &alternatives[model],
                            &span,
                        )?;
                    }
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

    fn check_context_reference_cycles(
        &mut self,
        source_type: usize,
        (generic, alternative): (usize, usize),
        alternatives: &ContextAlternatives,
        topic: &Span,
    ) -> Option<()> {
        // A branch uses a single choice for every generic, even when its
        // reference path reaches different axes of that generic.
        type Axis = (usize, usize);
        enum Walk {
            Enter(Axis, Span),
            Leave(Axis),
            Select {
                generic: usize,
                axis: Axis,
                at: Span,
            },
            Unselect(usize),
        }

        let mut pending = Vec::new();
        for index in (0..reference_ranges(&self.types[source_type]).len()).rev() {
            pending.push(Walk::Enter((source_type, index), topic.clone()));
        }
        let mut selected =
            BTreeMap::from([(generic, self.symbols[alternative].declaration.type_id?)]);
        let mut active = BTreeSet::new();
        while let Some(next) = pending.pop() {
            let (node, at) = match next {
                Walk::Enter(node, at) => (node, at),
                Walk::Leave(node) => {
                    active.remove(&node);
                    continue;
                }
                Walk::Select { generic, axis, at } => {
                    selected.insert(generic, axis.0);
                    pending.push(Walk::Unselect(generic));
                    (axis, at)
                }
                Walk::Unselect(generic) => {
                    selected.remove(&generic);
                    continue;
                }
            };
            if !self.step(1, &at) {
                return None;
            }
            if !active.insert(node) {
                self.diagnostics.push_related(
                    "ILI-REFSYSTEM-CYCLE",
                    "Selecting coordinate alternatives closes a reference-system cycle",
                    Some(at),
                    vec![RelatedLocation {
                        span: self.symbols[alternative].name_span.clone(),
                        message: "Selected coordinate alternative".into(),
                    }],
                );
                return None;
            }
            pending.push(Walk::Leave(node));
            let Some(reference) = reference_ranges(&self.types[node.0])
                .get(node.1)
                .and_then(|range| range.reference_system.as_ref())
            else {
                continue;
            };
            let Some(&target) = self.names.get(reference.domain.target_name()) else {
                continue;
            };
            let at = reference.domain.span.clone();
            let ty = self.symbols[target].declaration.type_id?;
            let axis = match reference.axis {
                Some(axis) => usize::from(axis - 1),
                None if matches!(self.types[ty], TypeInfo::Numeric(_)) => 0,
                None => continue,
            };
            if self.symbols[target].declaration.is_generic() {
                if let Some(&chosen) = selected.get(&target) {
                    pending.push(Walk::Enter((chosen, axis), at));
                } else if let Some(choices) = alternatives.get(&target) {
                    if !self.step(choices.domains.len(), &at) {
                        return None;
                    }
                    for &candidate in choices.domains.iter().rev() {
                        let ty = self.symbols[candidate].declaration.type_id?;
                        pending.push(Walk::Select {
                            generic: target,
                            axis: (ty, axis),
                            at: at.clone(),
                        });
                    }
                }
            } else {
                pending.push(Walk::Enter((ty, axis), at));
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
    ) -> Option<BTreeMap<usize, BTreeSet<TypeUse>>> {
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
        let mut generic_domains = BTreeMap::<usize, BTreeSet<TypeUse>>::new();
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
                .map(|id| (id, None))
                .collect();
            while let Some((id, axis)) = types.pop() {
                if !self.step(1, span) {
                    return None;
                }
                if !checked_types.insert((id, axis)) {
                    continue;
                }
                if let Some(&generic) = self.generic_types.get(&id) {
                    generic_domains.entry(generic).or_default();
                    if let Some(choices) = alternatives.get(&generic) {
                        if !self.step(choices.domains.len(), span) {
                            return None;
                        }
                        types.extend(choices.domains.iter().filter_map(|&candidate| {
                            self.symbols[candidate]
                                .declaration
                                .type_id
                                .map(|ty| (ty, axis))
                        }));
                    }
                    continue;
                }
                match &self.types[id] {
                    TypeInfo::Collection(collection) => types.push((collection.element, None)),
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
                            && let Some(ty) = self.referenced_context_type(
                                (id, None),
                                vertex,
                                &mut generic_domains,
                            )
                        {
                            types.push((ty, None));
                        }
                    }
                    ty => {
                        for (index, range) in reference_ranges(ty).iter().enumerate() {
                            if axis.is_some_and(|axis| axis != index) {
                                continue;
                            }
                            let Some(reference) = &range.reference_system else {
                                continue;
                            };
                            let Some(ty) = self.referenced_context_type(
                                (id, axis),
                                &reference.domain,
                                &mut generic_domains,
                            ) else {
                                continue;
                            };
                            if let Some(axis) = reference.axis {
                                types.push((ty, Some(usize::from(axis - 1))));
                            } else if matches!(self.types[ty], TypeInfo::Numeric(_)) {
                                types.push((ty, None));
                            }
                        }
                    }
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

    fn referenced_context_type(
        &self,
        use_type: TypeUse,
        reference: &TypeReference,
        used: &mut BTreeMap<usize, BTreeSet<TypeUse>>,
    ) -> Option<usize> {
        let &id = self.names.get(reference.target_name())?;
        let declaration = &self.symbols[id].declaration;
        if declaration.is_generic() {
            used.entry(id).or_default().insert(use_type);
        }
        declaration.type_id
    }
}
