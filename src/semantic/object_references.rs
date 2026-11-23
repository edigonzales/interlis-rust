use super::{
    Analyzer, BoundName, Kind, LookupError, QualifiedName, collections::structure_targets,
};
use crate::{LanguageVersion, ObjectTarget, ReferenceType, Span, TypeInfo, TypeReference, syntax};
use std::collections::BTreeSet;

impl Analyzer<'_, '_> {
    pub(super) fn reference_type(
        &mut self,
        owner: usize,
        properties: &[syntax::Located<syntax::Property>],
        target: &syntax::RestrictedTarget,
    ) -> Option<ReferenceType> {
        let symbol = &self.symbols[owner];
        if self.models[symbol.model].language == LanguageVersion::V23
            && symbol.declaration.kind == Kind::Attribute
            && let Some(parent) = symbol.declaration.parent
            && self.symbols[parent].declaration.kind != Kind::Structure
        {
            self.diagnostics.push(
                "ILI-REFERENCE-ATTRIBUTE-SCOPE",
                "INTERLIS 2.3 reference attributes may only be declared in structures",
                Some(symbol.name_span.clone()),
            );
            return None;
        }
        if let [first, repeated, ..] = properties {
            self.duplicate(&repeated.span, &first.span);
            return None;
        }
        let target = self.bind_object_target(owner, target)?;
        if !self.symbols[owner].declaration.is_abstract()
            && !self
                .has_concrete_reference_target(&target, &self.symbols[owner].name_span.clone())?
        {
            self.diagnostics.push(
                "ILI-REFERENCE-ABSTRACT",
                "A concrete reference attribute requires a concrete target class",
                Some(self.symbols[owner].name_span.clone()),
            );
            return None;
        }
        Some(ReferenceType {
            external: !properties.is_empty(),
            target,
        })
    }

    pub(super) fn bind_object_target(
        &mut self,
        owner: usize,
        syntax: &syntax::RestrictedTarget,
    ) -> Option<ObjectTarget> {
        let target = match &syntax.target {
            syntax::TargetRef::Named(name) => Some(self.object_reference(owner, name)?),
            syntax::TargetRef::AnyClass(_) => None,
        };
        let mut restrictions = Vec::with_capacity(syntax.restrictions.len());
        for name in &syntax.restrictions {
            let restriction = self.object_reference(owner, name)?;
            if let Some(base) = &target
                && !self.specializes_symbol(
                    self.names[restriction.target_name()],
                    self.names[base.target_name()],
                    &name.span,
                )?
            {
                self.diagnostics.push(
                    "ILI-REFERENCE-RESTRICTION",
                    "A restricted target must specialize the primary class or association",
                    Some(name.span.clone()),
                );
                return None;
            }
            restrictions.push(restriction);
        }
        Some(ObjectTarget {
            target,
            restrictions,
        })
    }

    fn object_reference(&mut self, owner: usize, name: &QualifiedName) -> Option<TypeReference> {
        match self.lookup_name(
            owner,
            &name.segments,
            &name.span,
            &[Kind::Class, Kind::Association],
        ) {
            Ok(bound @ BoundName::Declared(_)) => self.bound_reference(owner, name, bound),
            Ok(BoundName::Predefined(..)) => {
                self.diagnostics.push(
                    "ILI-UNSUPPORTED",
                    "A reference to this predefined class requires its full declaration",
                    Some(name.span.clone()),
                );
                None
            }
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Reference target cannot be resolved to a class or association",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    pub(super) fn has_concrete_reference_target(
        &mut self,
        target: &ObjectTarget,
        span: &Span,
    ) -> Option<bool> {
        let ids: Vec<_> = object_targets(target)
            .iter()
            .map(|link| self.names[link.target_name()])
            .collect();
        if ids.is_empty()
            || ids
                .iter()
                .any(|&id| !self.symbols[id].declaration.is_abstract())
        {
            return Some(true);
        }
        for id in 0..self.symbols.len() {
            if !self.step(1, span) {
                return None;
            }
            let declaration = &self.symbols[id].declaration;
            if !matches!(declaration.kind, Kind::Class | Kind::Association)
                || declaration.is_abstract()
            {
                continue;
            }
            for &base in &ids {
                if self.specializes_symbol(id, base, span)? {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    pub(super) fn reference_specializes(
        &mut self,
        actual: usize,
        base: usize,
        span: &Span,
    ) -> Option<bool> {
        let (TypeInfo::Reference(actual), TypeInfo::Reference(base)) =
            (&self.types[actual], &self.types[base])
        else {
            return Some(false);
        };
        if actual.external && !base.external {
            return Some(false);
        }
        let actual: Vec<_> = object_targets(&actual.target)
            .iter()
            .map(|link| self.names[link.target_name()])
            .collect();
        let base: Vec<_> = object_targets(&base.target)
            .iter()
            .map(|link| self.names[link.target_name()])
            .collect();
        self.targets_specialize(&actual, &base, span)
    }

    pub(super) fn targets_specialize(
        &mut self,
        actual: &[usize],
        base: &[usize],
        span: &Span,
    ) -> Option<bool> {
        if base.is_empty() {
            return Some(true);
        }
        for &candidate in actual {
            let mut matches = false;
            for &target in base {
                if !self.step(1, span) {
                    return None;
                }
                if self.specializes_symbol(candidate, target, span)? {
                    matches = true;
                    break;
                }
            }
            if !matches {
                return Some(false);
            }
        }
        Some(!actual.is_empty())
    }

    pub(super) fn validate_reference_types(&mut self, viewables: &[usize]) -> Option<()> {
        if !self
            .types
            .iter()
            .any(|ty| matches!(ty, TypeInfo::Reference(_)))
        {
            return Some(());
        }
        for ty in &self.types {
            if let TypeInfo::Reference(reference) = ty {
                for link in object_targets(&reference.target) {
                    let target = self.names[link.target_name()];
                    if self.symbols[target]
                        .declaration
                        .association()
                        .is_some_and(|association| !association.identified)
                    {
                        self.diagnostics.push(
                            "ILI-REFERENCE-ASSOCIATION-OID",
                            "A referenced association requires its own identity",
                            Some(link.span.clone()),
                        );
                        return None;
                    }
                }
            }
        }
        for &owner in viewables {
            let topic = self.containing_topic(owner);
            let span = self.symbols[owner].name_span.clone();
            let owner_is_class_23 = self.symbols[owner].declaration.kind == Kind::Class
                && self.models[self.symbols[owner].model].language == LanguageVersion::V23;
            let inherited_structure = self.symbols[owner]
                .base_scope
                .filter(|&base| self.symbols[base].declaration.kind == Kind::Structure);
            if owner_is_class_23 && let Some(base) = inherited_structure {
                self.validate_contained_references(base, topic, &span, true)?;
            }
            if topic.is_some() {
                self.validate_contained_references(owner, topic, &span, false)?;
            }
        }
        Some(())
    }

    fn validate_contained_references(
        &mut self,
        owner: usize,
        topic: Option<usize>,
        span: &Span,
        forbid: bool,
    ) -> Option<()> {
        let mut pending = vec![owner];
        let mut seen = BTreeSet::new();
        let no_direct = self.symbols[owner].declaration.kind == Kind::Class
            && self.models[self.symbols[owner].model].language == LanguageVersion::V23;
        while let Some(id) = pending.pop() {
            if !self.step(1, span) {
                return None;
            }
            if !seen.insert(id) {
                continue;
            }
            let Some(members) = &self.symbols[id].declaration.effective_attributes else {
                continue;
            };
            if self
                .work
                .charge(u32::try_from(members.len()).unwrap_or(u32::MAX))
                .is_err()
            {
                self.diagnostics
                    .limit("Semantic work budget exceeded", Some(span.clone()));
                return None;
            }
            for index in 0..members.len() {
                let attribute = self.symbols[id]
                    .declaration
                    .effective_attributes
                    .as_ref()
                    .unwrap()[index];
                let type_id = self.symbols[attribute].declaration.type_id?;
                let ty = match &self.types[type_id] {
                    TypeInfo::Collection(collection) => &self.types[collection.element],
                    ty => ty,
                };
                match ty {
                    TypeInfo::Structure(structure) => {
                        pending.extend(
                            structure_targets(structure)
                                .iter()
                                .map(|link| self.names[link.target_name()]),
                        );
                    }
                    TypeInfo::Reference(_) if forbid || id == owner && no_direct => {
                        self.diagnostics.push(
                            "ILI-REFERENCE-ATTRIBUTE-SCOPE",
                            "An INTERLIS 2.3 class cannot extend a structure containing reference attributes",
                            Some(span.clone()),
                        );
                        return None;
                    }
                    TypeInfo::Reference(reference) => {
                        let external = reference.external;
                        let direct = id == owner
                            && self.symbols[attribute].declaration.parent == Some(owner);
                        let targets: Vec<_> = object_targets(&reference.target)
                            .iter()
                            .map(|link| {
                                (
                                    self.names[link.target_name()],
                                    if direct {
                                        link.span.clone()
                                    } else {
                                        span.clone()
                                    },
                                )
                            })
                            .collect();
                        for (target, location) in targets {
                            self.check_object_target_scope(topic, target, external, &location)?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Some(())
    }

    pub(super) fn containing_topic(&self, owner: usize) -> Option<usize> {
        let mut current = Some(owner);
        while let Some(id) = current {
            let declaration = &self.symbols[id].declaration;
            if declaration.kind == Kind::Topic {
                return Some(id);
            }
            current = declaration.parent;
        }
        None
    }

    pub(super) fn check_object_target_scope(
        &mut self,
        source: Option<usize>,
        target: usize,
        external: bool,
        location: &Span,
    ) -> Option<()> {
        if !self.step(1, location) {
            return None;
        }
        let (Some(source), Some(target)) = (source, self.containing_topic(target)) else {
            return Some(());
        };
        if self.specializes_symbol(source, target, location)? {
            return Some(());
        }
        if !external {
            self.diagnostics.push(
                "ILI-REFERENCE-EXTERNAL",
                "A reference to another topic requires EXTERNAL",
                Some(location.clone()),
            );
            return None;
        }
        let dependencies = self.symbols[source].declaration.topic_dependencies().len();
        if !self.step(dependencies, location) {
            return None;
        }
        if !self.symbols[source]
            .declaration
            .topic_dependencies()
            .iter()
            .any(|dependency| self.names[dependency.target_name()] == target)
        {
            self.diagnostics.push(
                "ILI-REFERENCE-DEPENDS-ON",
                "A reference to another topic requires a DEPENDS ON declaration",
                Some(location.clone()),
            );
            return None;
        }
        Some(())
    }
}

pub(super) fn object_targets(target: &ObjectTarget) -> &[TypeReference] {
    if target.restrictions.is_empty() {
        target.target.as_slice()
    } else {
        &target.restrictions
    }
}
