use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName};
use crate::{Cardinality, Span, StructureType, TypeInfo, TypeReference, syntax};
use std::cmp::Ordering;

impl Analyzer<'_, '_> {
    pub(super) fn collection_default(&mut self, owner: usize, span: &Span) -> Option<Cardinality> {
        let inherited = self.symbols[owner]
            .base_scope
            .and_then(|base| self.symbols[base].declaration.type_id);
        if let Some(id) = inherited
            && let TypeInfo::Collection(collection) = &self.types[id]
        {
            let bytes = collection.cardinality.lower.len()
                + collection.cardinality.upper.as_ref().map_or(0, String::len);
            if !self.step(bytes, span) {
                return None;
            }
            let TypeInfo::Collection(collection) = &self.types[id] else {
                unreachable!("inherited type was checked");
            };
            return Some(collection.cardinality.clone());
        }
        Some(Cardinality {
            lower: "0".into(),
            upper: None,
        })
    }
    pub(super) fn structure_specializes(
        &mut self,
        own: usize,
        base: usize,
        span: &Span,
    ) -> Option<bool> {
        let (TypeInfo::Structure(own), TypeInfo::Structure(base)) =
            (&self.types[own], &self.types[base])
        else {
            return Some(false);
        };
        let own: Vec<_> = structure_targets(own)
            .iter()
            .map(|link| self.names[link.target_name()])
            .collect();
        let base: Vec<_> = structure_targets(base)
            .iter()
            .map(|link| self.names[link.target_name()])
            .collect();
        self.targets_specialize(&own, &base, span)
    }
    pub(super) fn effective_cardinality(
        &mut self,
        syntax: Option<&syntax::Cardinality>,
        default: Cardinality,
    ) -> Option<Cardinality> {
        use syntax::{Cardinality as Spelling, CardinalityBound};
        let (lower, upper) = match syntax {
            None => return Some(default),
            Some(Spelling::Unbounded { .. }) => {
                return Some(Cardinality {
                    lower: "0".into(),
                    upper: None,
                });
            }
            Some(Spelling::Exact { number, .. }) => {
                let count = self.literal(number)?.info.coefficient;
                (count.clone(), Some(count))
            }
            Some(Spelling::Range { lower, upper, .. }) => {
                let minimum = self.literal(lower)?.info.coefficient;
                let maximum = match upper {
                    CardinalityBound::Number(upper) => {
                        let maximum = self.literal(upper)?.info.coefficient;
                        if compare_count(&minimum, &maximum) == Ordering::Greater {
                            self.diagnostics.push(
                                "ILI-CARDINALITY-RANGE",
                                "A cardinality minimum must not exceed its maximum",
                                Some(upper.span.clone()),
                            );
                            return None;
                        }
                        Some(maximum)
                    }
                    CardinalityBound::Unbounded(_) => None,
                };
                (minimum, maximum)
            }
        };
        Some(Cardinality { lower, upper })
    }

    pub(super) fn restricted_structure(
        &mut self,
        owner: usize,
        target: &syntax::StructureTarget,
        restrictions: &[QualifiedName],
    ) -> Option<StructureType> {
        let target = match target {
            syntax::StructureTarget::Named(name) => Some(self.structure_reference(owner, name)?),
            syntax::StructureTarget::Any(_) => None,
        };
        let mut bound_restrictions = Vec::with_capacity(restrictions.len());
        for name in restrictions {
            let actual = self.structure_reference(owner, name)?;
            if let Some(base) = &target
                && !self.specializes_symbol(
                    self.names[actual.target_name()],
                    self.names[base.target_name()],
                    &name.span,
                )?
            {
                self.diagnostics.push(
                    "ILI-STRUCTURE-RESTRICTION",
                    "A restricted structure must specialize the primary structure",
                    Some(name.span.clone()),
                );
                return None;
            }
            bound_restrictions.push(actual);
        }
        Some(StructureType {
            target,
            restrictions: bound_restrictions,
        })
    }

    pub(super) fn structure_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
    ) -> Option<TypeReference> {
        match self.lookup_name(owner, &name.segments, &name.span, &[Kind::Structure]) {
            Ok(bound @ BoundName::Declared(_)) => self.bound_reference(owner, name, bound),
            Ok(bound @ BoundName::Predefined(..)) => {
                let id = self.bound_structure(owner, bound, &name.span)?;
                self.bound_reference(owner, name, BoundName::Declared(id))
            }
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Structure reference cannot be resolved",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    pub(super) fn specializes_symbol(
        &mut self,
        mut actual: usize,
        base: usize,
        span: &Span,
    ) -> Option<bool> {
        loop {
            if actual == base {
                return Some(true);
            }
            let Some(next) = self.symbols[actual].base_scope else {
                return Some(false);
            };
            if !self.step(1, span) {
                return None;
            }
            actual = next;
        }
    }

    pub(super) fn structure_can_be_required(
        &mut self,
        owner: usize,
        mut type_id: usize,
        span: &Span,
    ) -> Option<bool> {
        if let TypeInfo::Collection(collection) = &self.types[type_id] {
            type_id = collection.element;
        }
        let TypeInfo::Structure(structure) = &self.types[type_id] else {
            return Some(true);
        };
        let candidates: Vec<_> = structure_targets(structure)
            .iter()
            .map(|reference| self.names[reference.target_name()])
            .collect();
        if candidates
            .iter()
            .any(|&target| !self.symbols[target].declaration.is_abstract())
        {
            return Some(true);
        }
        if candidates.is_empty() {
            return Some(false);
        }
        let scope = self.symbols[owner]
            .declaration
            .parent
            .and_then(|parent| self.symbols[parent].declaration.parent);
        for id in 0..self.symbols.len() {
            if !self.step(1, span) {
                return None;
            }
            let declaration = &self.symbols[id].declaration;
            if declaration.kind != Kind::Structure || declaration.is_abstract() {
                continue;
            }
            let parent = declaration.parent;
            let locally_visible = match (scope, parent) {
                (Some(scope), Some(parent)) => self.specializes_symbol(scope, parent, span)?,
                _ => scope == parent,
            };
            for &base in &candidates {
                if (locally_visible || parent == self.symbols[base].declaration.parent)
                    && self.specializes_symbol(id, base, span)?
                {
                    return Some(true);
                }
            }
        }
        Some(false)
    }
}

pub(super) fn structure_targets(structure: &StructureType) -> &[TypeReference] {
    if structure.restrictions.is_empty() {
        structure.target.as_slice()
    } else {
        &structure.restrictions
    }
}

pub(super) fn compare_count(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

pub(super) fn cardinality_narrows(actual: &Cardinality, base: &Cardinality) -> bool {
    compare_count(&actual.lower, &base.lower) != Ordering::Less
        && base.upper.as_ref().is_none_or(|maximum| {
            actual
                .upper
                .as_ref()
                .is_some_and(|actual| compare_count(actual, maximum) != Ordering::Greater)
        })
}
