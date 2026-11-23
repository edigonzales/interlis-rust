use super::{
    Analyzer, BoundName, Kind, LookupError, QualifiedName, SyntaxDecl,
    collections::structure_targets, domains::scalar_extension,
};
use crate::{ContextMapping, Span, TopicGenericBinding, TypeInfo, TypeReference};
use std::collections::{BTreeMap, BTreeSet};

type ContextAlternatives = BTreeMap<usize, Vec<usize>>;

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
                    if !self.step(candidates.len(), &span) {
                        return None;
                    }
                    if let Some(previous) = effective.get_mut(&generic) {
                        let joint: BTreeSet<_> =
                            previous.iter().chain(candidates).copied().collect();
                        let mut intersection = Vec::new();
                        for candidate in joint {
                            if self.refines_context(candidate, previous, &span)?
                                && self.refines_context(candidate, candidates, &span)?
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
                        *previous = intersection;
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
                            if !self.refines_context(candidate, previous, &span)? {
                                self.diagnostics.push(
                                    "ILI-CONTEXT-EXTENSION",
                                    "A context override must narrow the existing alternatives",
                                    Some(span),
                                );
                                return None;
                            }
                        }
                    }
                    effective.insert(generic, candidates);
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
                &span,
            )?;
            if let Some(base) = self.symbols[topic].base_scope {
                if !self.step(deferred[base].len(), &span) {
                    return None;
                }
                deferred[topic] = deferred[base].clone();
            }
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
            }
            let mut bindings = Vec::new();
            for generic in used {
                let choice = alternatives[model]
                    .get(&generic)
                    .map_or(&[][..], Vec::as_slice);
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
                    deferred: choice.len() > 1,
                });
            }
            if !bindings.is_empty() {
                self.symbols[topic].declaration.generic_bindings =
                    Some(bindings.into_boxed_slice());
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
                if let Some(&generic) = self.generic_types.get(&id) {
                    generic_domains.insert(generic);
                }
                match &self.types[id] {
                    TypeInfo::Collection(collection) => types.push(collection.element),
                    TypeInfo::Structure(structure) => containers.extend(
                        structure_targets(structure)
                            .iter()
                            .map(|reference| self.names[reference.target_name()]),
                    ),
                    TypeInfo::Geometry(geometry) => self
                        .record_generic_reference(geometry.vertex.as_ref(), &mut generic_domains),
                    TypeInfo::Numeric(range) => self.record_generic_reference(
                        range
                            .reference_system
                            .as_ref()
                            .map(|reference| &reference.domain),
                        &mut generic_domains,
                    ),
                    TypeInfo::Coordinate(coordinate) => {
                        for axis in &coordinate.axes {
                            self.record_generic_reference(
                                axis.reference_system
                                    .as_ref()
                                    .map(|reference| &reference.domain),
                                &mut generic_domains,
                            );
                        }
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

    fn record_generic_reference(
        &self,
        reference: Option<&TypeReference>,
        used: &mut BTreeSet<usize>,
    ) {
        if let Some(id) = reference.and_then(|reference| self.names.get(reference.target_name()))
            && self.symbols[*id].declaration.is_generic()
        {
            used.insert(*id);
        }
    }
}
