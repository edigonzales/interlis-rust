use super::{
    Analyzer, Kind, LookupError, Name, TypeInfo,
    collections::{compare_count, structure_targets},
    expressions::scalar_type,
    object_references::object_targets,
};
use crate::{
    BoundExpressionKind, BoundPathStep, CollectionKind, CoordinateKind, ExpressionType,
    LanguageVersion, ObjectSelection, PathSelection, Span, TypeReference,
    syntax::{ObjectPath, PathElement, PathSelector, SpecialPath},
};
use std::cmp::Ordering;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum PathContext {
    Object,
    UniquePrefix(LanguageVersion),
}

impl Analyzer<'_, '_> {
    pub(super) fn expression_path(
        &mut self,
        owner: usize,
        path: &ObjectPath,
    ) -> Option<(BoundExpressionKind, ExpressionType, Option<usize>)> {
        let elements = match path.elements.split_first() {
            Some((PathElement::Special(special), tail)) if special.value == SpecialPath::This => {
                if tail.is_empty() {
                    let type_id = self.symbols[owner].declaration.type_id;
                    let value_type =
                        type_id.map_or(ExpressionType::Object, |ty| scalar_type(&self.types[ty]));
                    return Some((BoundExpressionKind::This, value_type, type_id));
                }
                if self.symbols[owner].declaration.kind == Kind::Domain {
                    self.unsupported_expression(&path.span);
                    return None;
                }
                tail
            }
            _ => &path.elements,
        };
        if !self.step(elements.len(), &path.span) {
            return None;
        }
        let mut steps = Vec::with_capacity(elements.len());
        let mut scope = owner;
        let topic = self.containing_topic(owner);
        for (position, element) in elements.iter().enumerate() {
            let PathElement::Name { name, selector, .. } = element else {
                self.unsupported_expression(&path.span);
                return None;
            };
            if let Some(PathSelector::Association(association)) = selector {
                self.unsupported_expression(&association.span);
                return None;
            }
            let member =
                match self.visible_name(scope, &name.value, Some(&[Kind::Attribute]), &name.span) {
                    Ok(Some(member)) => member,
                    Err(LookupError::Limit) => return None,
                    _ => match self.visible_role(scope, &name.value, &name.span) {
                        Ok(Some(role)) => role,
                        Err(LookupError::Limit) => return None,
                        _ => {
                            return self.path_error(
                                "ILI-PATH-NAME",
                                "The attribute or role is absent from this object context",
                                &name.span,
                            );
                        }
                    },
                };
            let member_type = if self.symbols[member].declaration.kind == Kind::Role {
                self.role_value_type(scope, member, &name.span)?
            } else {
                self.symbols[member].declaration.type_id?
            };
            let reference = self.reference_to_symbol(owner, member, name)?;
            let (value, selection, selection_span) = match selector {
                Some(selector) => {
                    let (value, selection, origin) =
                        self.select_path_value(member_type, selector)?;
                    (value, Some(selection), Some(origin))
                }
                None => (member_type, None, None),
            };
            steps.push(BoundPathStep {
                reference,
                selection,
                selection_span,
            });
            if position + 1 == elements.len() {
                return Some((
                    BoundExpressionKind::Path { steps },
                    scalar_type(&self.types[value]),
                    Some(value),
                ));
            }
            scope = self.path_scope(value, topic, PathContext::Object, &name.span)?;
        }
        unreachable!("a parsed path contains at least one step")
    }

    pub(super) fn path_attribute(&mut self, scope: usize, name: &Name) -> Option<usize> {
        match self.visible_name(scope, &name.value, Some(&[Kind::Attribute]), &name.span) {
            Ok(Some(member)) => Some(member),
            Err(LookupError::Limit) => None,
            _ => self.path_error(
                "ILI-PATH-NAME",
                "The attribute is absent from this object context",
                &name.span,
            ),
        }
    }

    fn role_value_type(&mut self, scope: usize, role: usize, at: &Span) -> Option<usize> {
        let info = self.symbols[role].declaration.role()?;
        let plural = self.symbols[scope].declaration.kind != Kind::Association
            && info
                .cardinality
                .upper
                .as_deref()
                .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater);
        let bytes = info
            .targets
            .iter()
            .flat_map(|target| target.target.iter().chain(&target.restrictions))
            .map(|target| 1 + target.declared_name().len() + target.target_name().len())
            .sum();
        if !self.step(bytes, at) {
            return None;
        }
        let id = self.types.len();
        let targets = self.symbols[role].declaration.role()?.targets.clone();
        self.types.push(TypeInfo::ObjectSelection(ObjectSelection {
            plural,
            targets,
        }));
        Some(id)
    }

    fn select_path_value(
        &mut self,
        ty: usize,
        selector: &PathSelector,
    ) -> Option<(usize, PathSelection, Span)> {
        let (at, number) = match selector {
            PathSelector::First(span) | PathSelector::Last(span) => (span, None),
            PathSelector::Index(index) => {
                (&index.span, Some(self.literal(index)?.info.coefficient))
            }
            PathSelector::Association(_) => unreachable!("role qualifiers are bound separately"),
        };
        match &self.types[ty] {
            TypeInfo::Coordinate(coordinate) if coordinate.kind == CoordinateKind::Single => {
                let Some(axis) = number
                    .as_deref()
                    .and_then(|number| number.parse::<u8>().ok())
                    .filter(|&axis| axis > 0 && usize::from(axis) <= coordinate.axes.len())
                else {
                    return self.path_error(
                        "ILI-PATH-AXIS",
                        "A coordinate selector must name an existing axis",
                        at,
                    );
                };
                let index = usize::from(axis - 1);
                let range = &coordinate.axes[index];
                let numbers = range.bounds.as_ref().map_or(0, |bounds| {
                    bounds.lower.original().len()
                        + bounds.lower.coefficient().len()
                        + bounds.upper.original().len()
                        + bounds.upper.coefficient().len()
                });
                let names = range
                    .unit
                    .iter()
                    .chain(
                        range
                            .reference_system
                            .iter()
                            .map(|reference| &reference.domain),
                    )
                    .map(|reference| {
                        reference.declared_name().len() + reference.target_name().len()
                    })
                    .sum::<usize>();
                if !self.step(1 + numbers + names, at) {
                    return None;
                }
                let TypeInfo::Coordinate(coordinate) = &self.types[ty] else {
                    unreachable!()
                };
                let axis_type = self.types.len();
                self.types
                    .push(TypeInfo::Numeric(coordinate.axes[index].clone()));
                Some((axis_type, PathSelection::Axis(axis), at.clone()))
            }
            TypeInfo::Collection(collection) if collection.kind == CollectionKind::List => {
                let index = number.as_deref().unwrap_or("1");
                if index == "0"
                    || collection
                        .cardinality
                        .upper
                        .as_deref()
                        .is_some_and(|upper| compare_count(index, upper) == Ordering::Greater)
                {
                    return self.path_error(
                        "ILI-PATH-INDEX",
                        "A list index must be between one and the maximum cardinality",
                        at,
                    );
                }
                let selection = match selector {
                    PathSelector::First(_) => PathSelection::First,
                    PathSelector::Last(_) => PathSelection::Last,
                    PathSelector::Index(_) => {
                        PathSelection::Index(number.expect("index was parsed"))
                    }
                    PathSelector::Association(_) => unreachable!(),
                };
                Some((collection.element, selection, at.clone()))
            }
            _ => self.path_error(
                "ILI-PATH-SELECTOR",
                "A selector requires a coordinate or an ordered list",
                at,
            ),
        }
    }

    pub(super) fn path_scope(
        &mut self,
        mut ty: usize,
        topic: Option<usize>,
        context: PathContext,
        at: &Span,
    ) -> Option<usize> {
        while let TypeInfo::Collection(collection) = &self.types[ty] {
            if context == PathContext::Object
                && collection
                    .cardinality
                    .upper
                    .as_deref()
                    .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater)
            {
                return self.path_error(
                    "ILI-PATH-MULTIPLICITY",
                    "A path through a collection requires a single element",
                    at,
                );
            }
            ty = collection.element;
            if !self.step(1, at) {
                return None;
            }
        }
        let roots = match &self.types[ty] {
            TypeInfo::Structure(structure) => {
                self.path_targets(structure_targets(structure).iter())
            }
            TypeInfo::Geometry(geometry)
                if context == PathContext::UniquePrefix(LanguageVersion::V23)
                    && geometry.line_attributes.is_some() =>
            {
                self.path_targets(geometry.line_attributes.iter())
            }
            _ if matches!(context, PathContext::UniquePrefix(_)) => {
                return self.path_error(
                    "ILI-UNIQUE-PREFIX",
                    "A LOCAL key prefix must use structure attributes",
                    at,
                );
            }
            TypeInfo::Reference(reference) => {
                self.path_targets(object_targets(&reference.target).iter())
            }
            TypeInfo::ObjectSelection(selection) => {
                if selection.plural {
                    return self.path_error(
                        "ILI-PATH-MULTIPLICITY",
                        "A path through a role requires a single related object",
                        at,
                    );
                }
                if selection
                    .targets
                    .iter()
                    .any(|target| object_targets(target).is_empty())
                {
                    self.unsupported_expression(at);
                    return None;
                }
                self.path_targets(selection.targets.iter().flat_map(object_targets))
            }
            _ => {
                return self.path_error(
                    "ILI-PATH-TRAVERSAL",
                    "The preceding value cannot be traversed as an object",
                    at,
                );
            }
        };
        let Some(mut roots) = roots.filter(|roots| !roots.is_empty()) else {
            self.unsupported_expression(at);
            return None;
        };
        if !self.step(roots.len(), at) {
            return None;
        }
        if let Some(topic) = topic {
            for root in &mut roots {
                *root = self.contextual_path_scope(topic, *root, at)?;
            }
        }
        let mut base = Some(roots[0]);
        while let Some(candidate) = base {
            let mut common = true;
            for &other in &roots[1..] {
                if !self.specializes_symbol(other, candidate, at)? {
                    common = false;
                    break;
                }
            }
            if common {
                return Some(candidate);
            }
            base = self.symbols[candidate].base_scope;
        }
        self.path_error(
            "ILI-PATH-SCOPE",
            "The possible objects have no common declared scope for this path",
            at,
        )
    }

    fn path_targets<'a>(
        &self,
        targets: impl Iterator<Item = &'a TypeReference>,
    ) -> Option<Vec<usize>> {
        targets
            .map(|target| self.names.get(target.target_name()).copied())
            .collect()
    }

    fn contextual_path_scope(&mut self, topic: usize, root: usize, at: &Span) -> Option<usize> {
        let Some(base_topic) = self.containing_topic(root) else {
            return Some(root);
        };
        if topic == base_topic || !self.specializes_symbol(topic, base_topic, at)? {
            return Some(root);
        }
        let declaration = &self.symbols[root].declaration;
        let name = declaration.name.clone();
        let kind = declaration.kind;
        if let Some(local) = self.visible_name(topic, &name, Some(&[kind]), at).ok()?
            && self.specializes_symbol(local, root, at)?
        {
            return Some(local);
        }
        Some(root)
    }

    fn path_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}
