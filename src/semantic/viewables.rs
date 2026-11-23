use super::{Analyzer, BoundName, Kind, LookupError, QualifiedName, SyntaxDecl};
use crate::{RelatedLocation, TypeReference, syntax};
use std::collections::BTreeSet;

impl Analyzer<'_, '_> {
    pub(super) fn bind_viewable_bases(&mut self) -> Option<Vec<usize>> {
        let mut viewables = Vec::new();
        let mut explicit = BTreeSet::new();
        let mut implicit = Vec::new();
        for id in 0..self.symbols.len() {
            let Some(SyntaxDecl::Container {
                name,
                base,
                properties,
                ..
            }) = self.symbols[id].ast
            else {
                continue;
            };
            viewables.push(id);
            let extension = properties
                .iter()
                .find(|property| property.value == syntax::Property::Extended);
            let kind = self.symbols[id].declaration.kind;
            let parent = self.symbols[id].declaration.parent?;
            let binding = match (base, extension) {
                (Some(_), Some(property)) => {
                    self.diagnostics.push(
                        "ILI-CLASS-EXTENDED",
                        "Use either EXTENDED or an explicit EXTENDS clause",
                        Some(property.span.clone()),
                    );
                    return None;
                }
                (Some(base), None) => {
                    let binding = self.viewable_base(id, base, kind)?;
                    explicit.insert((parent, binding.0));
                    binding
                }
                (None, Some(property)) => {
                    let inherited = match self.symbols[parent].base_scope {
                        Some(base) => self
                            .visible_name(base, &name.value, Some(&[kind]), &property.span)
                            .ok()?,
                        None => None,
                    };
                    let Some(inherited) = inherited else {
                        self.diagnostics.push(
                            "ILI-CLASS-EXTENDED",
                            "EXTENDED requires a same-named member of the inherited topic",
                            Some(property.span.clone()),
                        );
                        return None;
                    };
                    let original = QualifiedName {
                        segments: vec![syntax::Name {
                            value: name.value.clone(),
                            span: name.span.clone(),
                        }],
                        span: name.span.clone(),
                    };
                    implicit.push((parent, inherited, property.span.clone()));
                    (
                        inherited,
                        self.bound_reference(id, &original, BoundName::Declared(inherited))?,
                    )
                }
                (None, None) => continue,
            };
            if self.symbols[binding.0].declaration.is_final() {
                self.diagnostics.push(
                    "ILI-CLASS-FINAL",
                    "A FINAL class or structure cannot be extended",
                    Some(binding.1.span.clone()),
                );
                return None;
            }
            self.symbols[id].base_scope = Some(binding.0);
            self.symbols[id].declaration.base_reference = Some(binding.1);
        }
        let order = self.inheritance_order(viewables, "ILI-CLASS-CYCLE")?;
        for (owner, base, span) in implicit {
            let base_topic = self.symbols[base].declaration.parent?;
            if explicit.contains(&(owner, base)) || explicit.contains(&(base_topic, base)) {
                self.diagnostics.push(
                    "ILI-CLASS-EXTENDED-CONFLICT",
                    "An EXTENDED declaration conflicts with an explicit specialization of the same base",
                    Some(span),
                );
                return None;
            }
        }
        Some(order)
    }

    fn viewable_base(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kind: Kind,
    ) -> Option<(usize, TypeReference)> {
        let kinds = if kind == Kind::Class {
            &[Kind::Class, Kind::Structure][..]
        } else {
            &[Kind::Structure]
        };
        match self.lookup_name(owner, &name.segments, &name.span, kinds) {
            Ok(bound @ BoundName::Declared(id)) => {
                Some((id, self.bound_reference(owner, name, bound)?))
            }
            Ok(BoundName::Predefined(..)) => {
                self.diagnostics.push(
                    "ILI-UNSUPPORTED",
                    "Inheritance from this predefined class or structure requires its full declaration",
                    Some(name.span.clone()),
                );
                None
            }
            Err(LookupError::Limit) => None,
            Err(_) => {
                self.diagnostics.push(
                    "ILI-NAME-NOT-FOUND",
                    "Class or structure base cannot be resolved",
                    Some(name.span.clone()),
                );
                None
            }
        }
    }

    pub(super) fn inheritance_order(
        &mut self,
        nodes: Vec<usize>,
        cycle_code: &str,
    ) -> Option<Vec<usize>> {
        let mut ordered = Vec::with_capacity(nodes.len());
        let mut visited_by = vec![None; self.symbols.len()];
        let mut path = Vec::new();
        for start in nodes {
            let mut current = start;
            while visited_by[current].is_none() {
                visited_by[current] = Some(start);
                path.push(current);
                let Some(base) = self.symbols[current].base_scope else {
                    break;
                };
                let location = self.symbols[current]
                    .declaration
                    .base_reference()
                    .expect("bound inheritance has a source reference")
                    .span
                    .clone();
                if !self.step(1, &location) {
                    return None;
                }
                if visited_by[base] == Some(start) {
                    self.diagnostics
                        .push(cycle_code, "Inheritance closes a cycle", Some(location));
                    return None;
                }
                current = base;
            }
            ordered.extend(path.drain(..).rev());
        }
        Some(ordered)
    }

    pub(super) fn bind_effective_attributes(&mut self, order: &[usize]) -> Option<()> {
        let mut direct = vec![Vec::new(); self.symbols.len()];
        for (id, symbol) in self.symbols.iter().enumerate() {
            if symbol.declaration.kind == Kind::Attribute {
                direct[symbol.declaration.parent?].push(id);
            }
        }
        for &owner in order {
            let mut attributes = Vec::new();
            if let Some(base) = self.symbols[owner].base_scope {
                let inherited = &self.symbols[base].declaration.effective_attributes;
                let span = self.symbols[owner].name_span.clone();
                if !self.step(inherited.as_ref().map_or(0, |members| members.len()), &span) {
                    return None;
                }
                attributes.extend_from_slice(
                    self.symbols[base]
                        .declaration
                        .effective_attributes
                        .as_deref()
                        .unwrap_or_default(),
                );
            }
            let inherited: BTreeSet<_> = attributes
                .iter()
                .map(|&id| self.symbols[id].declaration.name.as_str())
                .collect();
            for &attribute in &direct[owner] {
                if inherited.contains(self.symbols[attribute].declaration.name.as_str()) {
                    self.diagnostics.push_related(
                        "ILI-ATTRIBUTE-OVERRIDE",
                        "Overriding an inherited attribute requires EXTENDED",
                        Some(self.symbols[attribute].name_span.clone()),
                        vec![RelatedLocation {
                            span: self.symbols[owner]
                                .declaration
                                .base_reference()
                                .unwrap()
                                .span
                                .clone(),
                            message: "Base class or structure".into(),
                        }],
                    );
                    return None;
                }
            }
            attributes.extend_from_slice(&direct[owner]);
            if !attributes.is_empty() {
                self.symbols[owner].declaration.effective_attributes =
                    Some(attributes.into_boxed_slice());
            }
        }
        Some(())
    }
}
