use super::{
    Analyzer, BoundName, Effective, Kind, LookupError, QualifiedName, SyntaxDecl, TypeSyntax,
};
use crate::{RelatedLocation, TypeReference, syntax};
use std::collections::{BTreeMap, BTreeSet};

impl Analyzer<'_, '_> {
    pub(super) fn bind_viewable_bases(&mut self) -> Option<Vec<usize>> {
        let mut viewables = Vec::new();
        let mut explicit = BTreeSet::new();
        let mut implicit = Vec::new();
        for id in 0..self.symbols.len() {
            let (base, properties) = match self.symbols[id].ast {
                Some(SyntaxDecl::Container {
                    base, properties, ..
                }) => (base, properties),
                Some(SyntaxDecl::Association(association)) => {
                    (&association.base, &association.properties)
                }
                _ => continue,
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
                    let name = syntax::Name {
                        value: self.symbols[id].declaration.name.clone(),
                        span: self.symbols[id].name_span.clone(),
                    };
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
                    implicit.push((parent, inherited, property.span.clone()));
                    (
                        inherited,
                        self.implicit_base_reference(id, inherited, &name)?,
                    )
                }
                (None, None) => continue,
            };
            if self.symbols[binding.0].declaration.is_final() {
                self.diagnostics.push(
                    "ILI-CLASS-FINAL",
                    "A FINAL class, structure or association cannot be extended",
                    Some(binding.1.span.clone()),
                );
                return None;
            }
            self.symbols[id].base_scope = Some(binding.0);
            self.symbols[id].declaration.base_reference = Some(binding.1);
        }
        let order = self.inheritance_order(viewables, "ILI-CLASS-CYCLE")?;
        for (owner, base, span) in implicit {
            let mut topic = Some(owner);
            while let Some(id) = topic {
                if !self.step(1, &span) {
                    return None;
                }
                if explicit.contains(&(id, base)) {
                    self.diagnostics.push(
                        "ILI-CLASS-EXTENDED-CONFLICT",
                        "An EXTENDED declaration conflicts with an explicit specialization of the same base",
                        Some(span),
                    );
                    return None;
                }
                topic = self.symbols[id].base_scope;
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
        let kinds = match kind {
            Kind::Class => &[Kind::Class, Kind::Structure][..],
            Kind::Association => &[Kind::Association],
            _ => &[Kind::Structure],
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
                    "Class, structure or association base cannot be resolved",
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

    pub(super) fn bind_effective_attributes(&mut self, order: &[usize]) -> Option<Vec<usize>> {
        let mut direct = vec![Vec::new(); self.symbols.len()];
        for (id, symbol) in self.symbols.iter().enumerate() {
            if symbol.declaration.kind == Kind::Attribute {
                direct[symbol.declaration.parent?].push(id);
            }
        }
        let mut declared = Vec::new();
        for &owner in order {
            let mut attributes = Vec::new();
            if let Some(base) = self.symbols[owner].base_scope {
                let inherited = &self.symbols[base].declaration.effective_attributes;
                let span = self.symbols[owner].name_span.clone();
                let work = inherited
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|&member| self.symbols[member].declaration.name.len() + 1)
                    .sum();
                if !self.step(work, &span) {
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
            let inherited: BTreeMap<_, _> = attributes
                .iter()
                .enumerate()
                .map(|(index, &id)| (self.symbols[id].declaration.name.clone(), index))
                .collect();
            for &attribute in &direct[owner] {
                let Some(SyntaxDecl::Attribute {
                    name, properties, ..
                }) = self.symbols[attribute].ast
                else {
                    unreachable!("direct attributes have attribute syntax");
                };
                let extended = properties
                    .iter()
                    .any(|property| property.value == syntax::Property::Extended);
                match inherited.get(&name.value) {
                    Some(&position) => {
                        let base = attributes[position];
                        if !extended || self.symbols[base].declaration.is_final() {
                            let message = if extended {
                                "A FINAL attribute cannot be extended"
                            } else {
                                "Overriding an inherited attribute requires EXTENDED"
                            };
                            self.diagnostics.push_related(
                                "ILI-ATTRIBUTE-OVERRIDE",
                                message,
                                Some(name.span.clone()),
                                vec![RelatedLocation {
                                    span: self.symbols[base].name_span.clone(),
                                    message: "Base attribute".into(),
                                }],
                            );
                            return None;
                        }
                        self.symbols[attribute].base_scope = Some(base);
                        self.symbols[attribute].declaration.base_reference =
                            Some(self.implicit_base_reference(attribute, base, name)?);
                        attributes[position] = attribute;
                    }
                    None if extended => {
                        self.diagnostics.push(
                            "ILI-ATTRIBUTE-OVERRIDE",
                            "EXTENDED requires an inherited attribute",
                            Some(name.span.clone()),
                        );
                        return None;
                    }
                    None => attributes.push(attribute),
                }
            }
            if !self.symbols[owner].declaration.is_abstract()
                && let Some(attribute) = attributes
                    .iter()
                    .copied()
                    .find(|&attribute| self.symbols[attribute].declaration.is_abstract())
            {
                self.diagnostics.push_related(
                    "ILI-CLASS-ABSTRACT-ATTRIBUTE",
                    "A concrete class or structure requires concrete attributes",
                    Some(self.symbols[owner].name_span.clone()),
                    vec![RelatedLocation {
                        span: self.symbols[attribute].name_span.clone(),
                        message: "Abstract attribute".into(),
                    }],
                );
                return None;
            }
            declared.extend_from_slice(&direct[owner]);
            if !attributes.is_empty() {
                self.symbols[owner].declaration.effective_attributes =
                    Some(attributes.into_boxed_slice());
            }
        }
        Some(declared)
    }

    pub(super) fn bind_attribute(&mut self, id: usize) -> Option<()> {
        let base_id = self.symbols[id].base_scope;
        let inherited = base_id.and_then(|base| {
            let declaration = &self.symbols[base].declaration;
            Some(Effective {
                ty: declaration.type_id?,
                mandatory: declaration.mandatory()?,
            })
        });
        let syntax = self.symbols[id].syntax;
        let effective = match (syntax, inherited) {
            (
                Some(TypeSyntax::Enumeration {
                    enumeration,
                    order,
                    span,
                }),
                Some(base),
            ) => {
                if !self.attribute_preserves_mandatory(id, base, false, span) {
                    return None;
                }
                self.extend_enumeration(base, enumeration, order.as_ref().map(|order| order.value))?
            }
            (Some(ty), Some(base)) => self.specialize_scalar(id, base, ty)?,
            (Some(ty), None) => self.concrete_type(id, ty)?,
            (None, Some(base)) => base,
            (None, None) => {
                self.diagnostics.push(
                    "ILI-ATTRIBUTE-TYPE",
                    "An attribute without an inherited type requires a type declaration",
                    Some(self.symbols[id].name_span.clone()),
                );
                return None;
            }
        };
        let declaration = &self.symbols[id].declaration;
        let location = declaration.span.clone();
        let mandatory = effective.mandatory || declaration.declared_mandatory.unwrap_or(false);
        let value_type = match &self.types[effective.ty] {
            crate::TypeInfo::Collection(collection) => collection.element,
            _ => effective.ty,
        };
        if !declaration.is_abstract()
            && (self.types[value_type].has_unbounded_range()
                || self.attribute_uses_abstract_domain(id, syntax.is_none(), &location)?
                || mandatory && !self.structure_can_be_required(id, effective.ty, &location)?)
        {
            self.diagnostics.push(
                "ILI-ATTRIBUTE-ABSTRACT",
                "An attribute with an abstract value domain must be ABSTRACT",
                Some(location),
            );
            return None;
        }
        let declaration = &mut self.symbols[id].declaration;
        declaration.type_id = Some(effective.ty);
        declaration.effective_mandatory = Some(mandatory);
        Some(())
    }

    pub(super) fn attribute_preserves_mandatory(
        &mut self,
        owner: usize,
        base: Effective,
        type_requires_value: bool,
        span: &crate::Span,
    ) -> bool {
        if base.mandatory
            && !type_requires_value
            && self.symbols[owner].declaration.declared_mandatory() != Some(true)
        {
            self.report_type_override(
                owner,
                "ILI-ATTRIBUTE-CARDINALITY",
                "An override cannot make a mandatory attribute optional",
                span,
            );
            return false;
        }
        true
    }

    fn attribute_uses_abstract_domain(
        &mut self,
        mut id: usize,
        inherits_type: bool,
        location: &crate::Span,
    ) -> Option<bool> {
        if inherits_type {
            while let Some(base) = self.symbols[id].base_scope {
                if !self.step(1, location) {
                    return None;
                }
                id = base;
                if self.symbols[id].syntax.is_some() {
                    break;
                }
            }
        }
        Some(
            self.symbols[id]
                .declaration
                .reference
                .as_ref()
                .and_then(|reference| self.names.get(reference.target_name()))
                .is_some_and(|&target| {
                    self.symbols[target].declaration.kind == Kind::Domain
                        && self.symbols[target].declaration.is_abstract()
                }),
        )
    }
}
