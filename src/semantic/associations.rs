use super::{
    Analyzer, Kind, Namespace, SyntaxDecl,
    collections::{cardinality_narrows, compare_count},
    object_references::object_targets,
};
use crate::{
    AssociationInfo, Cardinality, ObjectTarget, OidKind, RelatedLocation, RoleInfo, RoleStrength,
    Span, syntax,
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Clone, Copy)]
struct AssociationAccess {
    association: usize,
    source: usize,
    target: usize,
}

impl Analyzer<'_, '_> {
    pub(super) fn bind_associations(&mut self, viewables: &[usize]) -> Option<()> {
        for &owner in viewables {
            let Some(SyntaxDecl::Association(syntax)) = self.symbols[owner].ast else {
                continue;
            };
            let base = self.symbols[owner].base_scope;
            let base_info = base.and_then(|base| self.symbols[base].declaration.association());
            let span = self.symbols[owner].name_span.clone();
            let inherited = base_info.map_or_else(
                || Cardinality {
                    lower: "0".into(),
                    upper: Some("1".into()),
                },
                |base| base.cardinality.clone(),
            );
            let cardinality =
                self.effective_cardinality(syntax.cardinality.as_ref(), inherited.clone())?;
            if base.is_some() && !cardinality_narrows(&cardinality, &inherited) {
                self.diagnostics.push(
                    "ILI-ASSOCIATION-CARDINALITY",
                    "An extending association cannot widen the allowed number of links",
                    Some(span.clone()),
                );
                return None;
            }
            let oid_rule = self.symbols[owner]
                .declaration
                .oid_rules()
                .and_then(|rules| rules.object.as_ref());
            let identified = oid_rule.map_or_else(
                || {
                    syntax
                        .properties
                        .iter()
                        .any(|property| property.value == syntax::Property::Oid)
                        || base
                            .and_then(|base| self.symbols[base].declaration.association())
                            .is_some_and(|base| base.identified)
                },
                |rule| rule.kind != OidKind::Unstable,
            );
            if !identified
                && cardinality
                    .upper
                    .as_ref()
                    .is_none_or(|upper| compare_count(upper, "1") == Ordering::Greater)
            {
                self.diagnostics.push(
                    "ILI-ASSOCIATION-OID",
                    "Multiple links for the same participants require an association identity",
                    Some(span.clone()),
                );
                return None;
            }
            let mut roles = if let Some(base) = base {
                let inherited = self.symbols[base]
                    .declaration
                    .association()
                    .unwrap()
                    .roles
                    .len();
                if !self.step(inherited, &span) {
                    return None;
                }
                self.symbols[base]
                    .declaration
                    .association()
                    .unwrap()
                    .roles
                    .to_vec()
            } else {
                Vec::new()
            };
            let inherited_roles: BTreeMap<_, _> = roles
                .iter()
                .enumerate()
                .map(|(position, &role)| (self.symbols[role].declaration.name.clone(), position))
                .collect();
            for declaration in &syntax.roles {
                let name = &declaration.name;
                let role = self.scope_names[&(owner, Namespace::Member, name.value.clone())];
                let position = inherited_roles.get(&name.value).copied();
                let inherited = position.map(|index| roles[index]);
                if inherited.is_none() && base.is_some() {
                    self.diagnostics.push(
                        "ILI-ASSOCIATION-ROLE",
                        "An extending association cannot add a role",
                        Some(name.span.clone()),
                    );
                    return None;
                }
                self.bind_role(role, declaration, inherited)?;
                if let Some(position) = position {
                    roles[position] = role;
                } else {
                    roles.push(role);
                }
            }
            if roles.len() < 2 {
                self.diagnostics.push(
                    "ILI-ASSOCIATION-ROLES",
                    "An association requires at least two roles",
                    Some(span.clone()),
                );
                return None;
            }
            let mut hierarchical = false;
            for &role in &roles {
                if !self.symbols[owner].declaration.is_abstract()
                    && self.symbols[role].declaration.is_abstract()
                {
                    self.diagnostics.push(
                        "ILI-ASSOCIATION-ABSTRACT",
                        "A concrete association requires concrete roles",
                        Some(self.symbols[role].name_span.clone()),
                    );
                    return None;
                }
                if self.symbols[role].declaration.role().unwrap().strength
                    != RoleStrength::Association
                {
                    if hierarchical || roles.len() != 2 {
                        self.diagnostics.push(
                            "ILI-ASSOCIATION-AGGREGATION",
                            "Aggregation and composition require two roles and a single whole",
                            Some(self.symbols[role].name_span.clone()),
                        );
                        return None;
                    }
                    hierarchical = true;
                }
            }
            self.symbols[owner].declaration.association = Some(Box::new(AssociationInfo {
                cardinality,
                identified,
                roles: roles.into_boxed_slice(),
            }));
        }
        self.validate_association_targets(viewables)?;
        self.check_hidden_siblings(viewables)
    }

    fn bind_role(
        &mut self,
        role: usize,
        declaration: &syntax::RoleDecl,
        inherited: Option<usize>,
    ) -> Option<()> {
        let mut properties = BTreeSet::new();
        for property in &declaration.properties {
            if !properties.insert(property.value) {
                self.diagnostics.push(
                    "ILI-DUPLICATE",
                    "Role property occurs more than once",
                    Some(property.span.clone()),
                );
                return None;
            }
        }
        let has = |property: syntax::Property| properties.contains(&property);
        let extended = has(syntax::Property::Extended);
        let hiding = has(syntax::Property::Hiding);
        let ordered = has(syntax::Property::Ordered);
        let external = has(syntax::Property::External);
        let span = &declaration.name.span;
        if inherited.is_some() != extended || hiding && inherited.is_none() {
            self.diagnostics.push(
                "ILI-ROLE-EXTENSION",
                "A role override requires EXTENDED and an inherited role",
                Some(span.clone()),
            );
            return None;
        }
        let strength = declaration.strength.value;
        let default = inherited.map_or_else(
            || Cardinality {
                lower: "0".into(),
                upper: (strength == RoleStrength::Composition).then(|| "1".into()),
            },
            |base| {
                self.symbols[base]
                    .declaration
                    .role()
                    .unwrap()
                    .cardinality
                    .clone()
            },
        );
        let cardinality = self.effective_cardinality(declaration.cardinality.as_ref(), default)?;
        if strength == RoleStrength::Composition && cardinality.upper.as_deref() != Some("1") {
            self.diagnostics.push(
                "ILI-ROLE-COMPOSITION-CARDINALITY",
                "A composition role requires cardinality {1} or {0..1}",
                Some(span.clone()),
            );
            return None;
        }
        let mut targets = Vec::new();
        let mut primary = Vec::new();
        for syntax in &declaration.targets {
            let target = self.bind_object_target(role, syntax)?;
            let root = target
                .target
                .as_ref()
                .map(|link| self.names[link.target_name()]);
            for &prior in &primary {
                let overlaps = match (root, prior) {
                    (Some(actual), Some(base)) => {
                        self.specializes_symbol(actual, base, span)?
                            || self.specializes_symbol(base, actual, span)?
                    }
                    _ => true,
                };
                if overlaps {
                    self.diagnostics.push(
                        "ILI-ROLE-ALTERNATIVES",
                        "Role alternatives must not specialize one another",
                        Some(syntax.span.clone()),
                    );
                    return None;
                }
            }
            primary.push(root);
            targets.push(target);
        }
        let has_concrete = if self.symbols[role].declaration.is_abstract() {
            true
        } else {
            let mut result = false;
            for target in &targets {
                let model_level_class = object_targets(target).iter().any(|link| {
                    let id = self.names[link.target_name()];
                    self.symbols[id].declaration.kind == Kind::Class
                        && self.containing_topic(id).is_none()
                });
                result |= model_level_class || self.has_concrete_reference_target(target, span)?;
            }
            result
        };
        if !has_concrete {
            self.diagnostics.push(
                "ILI-ROLE-ABSTRACT",
                "A concrete role requires a concrete target",
                Some(span.clone()),
            );
            return None;
        }
        if let Some(base_id) = inherited {
            let base = self.symbols[base_id].declaration.role().unwrap();
            if self.symbols[base_id].declaration.is_final()
                || !cardinality_narrows(&cardinality, &base.cardinality)
                || strength < base.strength
                || external != base.external
                || base.ordered && !ordered
            {
                self.diagnostics.push_related(
                    "ILI-ROLE-EXTENSION",
                    "The override must specialize the role cardinality and strength and preserve access rules",
                    Some(span.clone()),
                    vec![RelatedLocation { span: self.symbols[base_id].name_span.clone(), message: "Inherited role".into() }],
                );
                return None;
            }
            let base = self.role_target_ids(&base.targets);
            let actual = self.role_target_ids(&targets);
            if !self.targets_specialize(&actual, &base, span)? {
                self.diagnostics.push(
                    "ILI-ROLE-TARGET-EXTENSION",
                    "An extended role cannot widen the target set",
                    Some(span.clone()),
                );
                return None;
            }
            self.symbols[role].base_scope = Some(base_id);
            self.symbols[role].declaration.base_reference =
                Some(self.reference_to_symbol(role, base_id, &declaration.name)?);
        }
        self.symbols[role].declaration.role = Some(Box::new(RoleInfo {
            strength,
            cardinality,
            ordered,
            external,
            hiding,
            targets,
        }));
        Some(())
    }

    fn role_target_ids(&self, targets: &[ObjectTarget]) -> Vec<usize> {
        if targets
            .iter()
            .any(|target| object_targets(target).is_empty())
        {
            return Vec::new();
        }
        targets
            .iter()
            .flat_map(object_targets)
            .map(|link| self.names[link.target_name()])
            .collect()
    }

    fn validate_association_targets(&mut self, viewables: &[usize]) -> Option<()> {
        for &owner in viewables {
            let Some(association) = self.symbols[owner].declaration.association() else {
                continue;
            };
            let topic = self.containing_topic(owner);
            let span = self.symbols[owner].name_span.clone();
            let role_count = association.roles.len();
            for position in 0..role_count {
                let role = self.symbols[owner].declaration.association().unwrap().roles[position];
                let declaration = &self.symbols[role].declaration;
                let role = declaration.role().unwrap();
                let external = role.external;
                let local = declaration.parent == Some(owner);
                let targets: Vec<_> = role
                    .targets
                    .iter()
                    .flat_map(object_targets)
                    .map(|link| {
                        (
                            self.names[link.target_name()],
                            if local {
                                link.span.clone()
                            } else {
                                span.clone()
                            },
                        )
                    })
                    .collect();
                for (target, location) in targets {
                    if self.symbols[target].declaration.kind == Kind::Association
                        && self.specializes_symbol(owner, target, &location)?
                    {
                        self.diagnostics.push(
                            "ILI-ROLE-ASSOCIATION-TARGET",
                            "A role cannot target its own association or an association it extends",
                            Some(location),
                        );
                        return None;
                    }
                    self.check_object_target_scope(topic, target, external, &location)?;
                    if self.symbols[target]
                        .declaration
                        .association()
                        .is_some_and(|association| !association.identified)
                    {
                        self.diagnostics.push(
                            "ILI-ROLE-ASSOCIATION-OID",
                            "An association used as a role target requires its own identity",
                            Some(location),
                        );
                        return None;
                    }
                }
            }
        }
        Some(())
    }

    fn check_hidden_siblings(&mut self, viewables: &[usize]) -> Option<()> {
        let mut specializations = BTreeMap::<usize, Vec<usize>>::new();
        for &owner in viewables {
            if self.symbols[owner].base_scope.is_none() {
                continue;
            }
            let Some(association) = self.symbols[owner].declaration.association() else {
                continue;
            };
            let roles = association.roles.to_vec();
            for role in roles {
                if self.symbols[role].declaration.parent != Some(owner) {
                    continue;
                }
                if self.symbols[role].base_scope.is_none() {
                    continue;
                }
                let info = self.symbols[role].declaration.role().unwrap();
                let targets = self.role_target_ids(&info.targets);
                let hiding = info.hiding;
                let span = self.symbols[role].name_span.clone();
                let root = self.original_role(role, &span)?;
                let siblings = specializations.entry(root).or_default();
                for &previous in siblings.iter() {
                    let previous_owner = self.symbols[previous].declaration.parent?;
                    if self.specializes_symbol(owner, previous_owner, &span)? {
                        continue;
                    }
                    let previous = self.symbols[previous].declaration.role().unwrap();
                    if previous.hiding == hiding {
                        continue;
                    }
                    let previous_targets = self.role_target_ids(&previous.targets);
                    if self.roles_overlap(&targets, &previous_targets, &span)? {
                        self.diagnostics.push(
                            "ILI-ROLE-HIDING",
                            "Sibling roles with overlapping targets must agree on HIDING",
                            Some(span),
                        );
                        return None;
                    }
                }
                siblings.push(role);
            }
        }
        Some(())
    }

    fn roles_overlap(&mut self, left: &[usize], right: &[usize], span: &Span) -> Option<bool> {
        if left.is_empty() || right.is_empty() {
            return Some(true);
        }
        for &first in left {
            for &second in right {
                if !self.step(1, span) {
                    return None;
                }
                if self.specializes_symbol(first, second, span)?
                    || self.specializes_symbol(second, first, span)?
                {
                    return Some(true);
                }
            }
        }
        Some(false)
    }

    pub(super) fn check_role_names(&mut self, viewables: &[usize]) -> Option<()> {
        let mut accesses = BTreeMap::<(usize, String), usize>::new();
        for &owner in viewables {
            let Some(association) = self.symbols[owner].declaration.association() else {
                continue;
            };
            let topic = self.containing_topic(owner);
            let at = self.symbols[owner].name_span.clone();
            let roles = association.roles.to_vec();
            for (position, &source) in roles.iter().enumerate() {
                let source = self.symbols[source].declaration.role().unwrap();
                if source.external {
                    continue;
                }
                for &target in &self.role_target_ids(&source.targets) {
                    for (other_position, &role) in roles.iter().enumerate() {
                        if position == other_position {
                            continue;
                        }
                        if !self.step(1, &at) {
                            return None;
                        }
                        self.check_role_access(&mut accesses, topic, target, role, &at)?;
                    }
                }
            }
        }
        if accesses.is_empty() {
            return Some(());
        }
        for &target in viewables {
            let at = self.symbols[target].name_span.clone();
            let mut names = BTreeMap::<&str, usize>::new();
            let mut base = Some(target);
            while let Some(owner) = base {
                if !self.step(1, &at) {
                    return None;
                }
                for ((_, name), &role) in accesses
                    .range((owner, String::new())..)
                    .take_while(|((candidate, _), _)| *candidate == owner)
                {
                    if !self.step(name.len() + 1, &at) {
                        return None;
                    }
                    if let Some(member) = self
                        .visible_name(target, name, Some(&[Kind::Attribute]), &at)
                        .ok()?
                        && !self.later_inherited_role(owner, member, role)
                    {
                        self.role_name_conflict(role, member);
                        return None;
                    }
                    if let Some(&previous) = names.get(name.as_str()) {
                        if self.original_role(role, &at)? != self.original_role(previous, &at)? {
                            self.role_name_conflict(role, previous);
                            return None;
                        }
                    } else {
                        names.insert(name, role);
                    }
                }
                base = self.symbols[owner].base_scope;
            }
        }
        self.role_accesses = accesses;
        Some(())
    }

    pub(super) fn visible_role(
        &mut self,
        mut owner: usize,
        name: &str,
        at: &Span,
    ) -> Result<Option<usize>, super::LookupError> {
        if let Some(role) = self.visible_name(owner, name, Some(&[Kind::Role]), at)? {
            return Ok(Some(role));
        }
        let mut key = (owner, name.to_owned());
        loop {
            if !self.step(1, at) {
                return Err(super::LookupError::Limit);
            }
            if let Some(&role) = self.role_accesses.get(&key) {
                return Ok(Some(role));
            }
            let Some(base) = self.symbols[owner].base_scope else {
                return Ok(None);
            };
            owner = base;
            key.0 = base;
        }
    }

    pub(super) fn following_role(
        &mut self,
        owner: usize,
        scope: usize,
        name: &str,
        at: &Span,
    ) -> Result<Option<usize>, super::LookupError> {
        use super::LookupError;
        let owner_topic = self.containing_topic(owner);
        let object_topic = self.containing_topic(scope);
        let mut candidates = Vec::new();
        for association in 0..self.symbols.len() {
            if !self.step(1, at) {
                return Err(LookupError::Limit);
            }
            let Some(info) = self.symbols[association].declaration.association() else {
                continue;
            };
            if !self.association_precedes_path(association, owner, at) {
                continue;
            }
            let topic = self.containing_topic(association);
            if topic.is_none()
                && self.symbols[association].model != self.symbols[owner].model
                && self.symbols[association].model != self.symbols[scope].model
            {
                continue;
            }
            let roles = info.roles.to_vec();
            if !self.path_topic_available(owner_topic, topic, at)?
                && !self.path_topic_available(object_topic, topic, at)?
            {
                continue;
            }
            for &target in &roles {
                if self.symbols[target].declaration.name != name {
                    continue;
                }
                for &source in &roles {
                    if source == target {
                        continue;
                    }
                    if !self.step(1, at) {
                        return Err(LookupError::Limit);
                    }
                    let targets = self
                        .role_target_ids(&self.symbols[source].declaration.role().unwrap().targets);
                    let mut accepts = targets.is_empty();
                    for root in targets {
                        if self
                            .specializes_symbol(scope, root, at)
                            .ok_or(LookupError::Limit)?
                        {
                            accepts = true;
                            break;
                        }
                    }
                    if accepts {
                        candidates.push(AssociationAccess {
                            association,
                            source,
                            target,
                        });
                    }
                }
            }
        }
        let mut found = None;
        for candidate in &candidates {
            let mut hidden = false;
            for other in &candidates {
                if !self.step(1, at) {
                    return Err(LookupError::Limit);
                }
                if candidate.association == other.association {
                    continue;
                }
                if self
                    .specializes_symbol(other.association, candidate.association, at)
                    .ok_or(LookupError::Limit)?
                    && self.replaces_role_access(*other)
                    || self
                        .specializes_symbol(candidate.association, other.association, at)
                        .ok_or(LookupError::Limit)?
                        && !self.replaces_role_access(*candidate)
                {
                    hidden = true;
                    break;
                }
            }
            if !hidden && let Some(previous) = found.replace(*candidate) {
                return Err(LookupError::Ambiguous([previous.source, candidate.source]));
            }
        }
        Ok(found.map(|candidate| candidate.target))
    }

    fn replaces_role_access(&self, access: AssociationAccess) -> bool {
        self.symbols[access.source]
            .declaration
            .role()
            .unwrap()
            .hiding
            || matches!(self.symbols[access.association].ast,
                Some(SyntaxDecl::Association(association)) if association.properties.iter()
                    .any(|property| property.value == syntax::Property::Extended))
    }

    fn path_topic_available(
        &mut self,
        from: Option<usize>,
        target: Option<usize>,
        at: &Span,
    ) -> Result<bool, super::LookupError> {
        let Some(target) = target else {
            return Ok(true);
        };
        let Some(from) = from else {
            return Ok(false);
        };
        let ancestor = self
            .specializes_symbol(from, target, at)
            .ok_or(super::LookupError::Limit)?;
        Ok(ancestor
            || self
                .topic_has_dependency(from, target, at)
                .ok_or(super::LookupError::Limit)?)
    }

    pub(super) fn association_precedes_path(
        &self,
        association: usize,
        owner: usize,
        at: &Span,
    ) -> bool {
        let declaration = &self.symbols[association];
        let location = declaration.declaration.source_span();
        declaration.model != self.symbols[owner].model
            || location.sort_key().0 != at.sort_key().0
            || location.sort_key() <= at.sort_key()
    }

    fn check_role_access(
        &mut self,
        accesses: &mut BTreeMap<(usize, String), usize>,
        topic: Option<usize>,
        target: usize,
        role: usize,
        at: &Span,
    ) -> Option<()> {
        let declaration = &self.symbols[target].declaration;
        let name = declaration.name.clone();
        let target = match topic {
            Some(topic) if Some(topic) != self.containing_topic(target) => {
                let local = self
                    .scope_names
                    .get(&(topic, Namespace::Type, name))
                    .copied();
                let Some(local) = local else {
                    return Some(());
                };
                if !self.specializes_symbol(local, target, at)? {
                    return Some(());
                }
                local
            }
            _ => target,
        };
        let name = self.symbols[role].declaration.name.clone();
        if let Some(member) = self
            .visible_name(target, &name, Some(&[Kind::Attribute]), at)
            .ok()?
        {
            self.role_name_conflict(role, member);
            return None;
        }
        let key = (target, name);
        if let Some(&previous) = accesses.get(&key) {
            if self.original_role(role, at)? != self.original_role(previous, at)? {
                self.role_name_conflict(role, previous);
                return None;
            }
        } else {
            accesses.insert(key, role);
        }
        Some(())
    }

    fn original_role(&mut self, mut role: usize, location: &Span) -> Option<usize> {
        while let Some(base) = self.symbols[role].base_scope {
            if !self.step(1, location) {
                return None;
            }
            role = base;
        }
        Some(role)
    }

    fn later_inherited_role(&self, direct_target: usize, attribute: usize, role: usize) -> bool {
        let Some(container) = self.symbols[attribute].declaration.parent else {
            return false;
        };
        let Some(association) = self.symbols[role].declaration.parent else {
            return false;
        };
        let (association_source, start, _) = self.symbols[association].declaration.span.sort_key();
        let (owner_source, owner_start, _) = self.symbols[container].declaration.span.sort_key();
        container != direct_target && association_source == owner_source && start > owner_start
    }

    fn role_name_conflict(&mut self, role: usize, previous: usize) {
        self.diagnostics.push_related(
            "ILI-ROLE-NAMESPACE",
            "A role access conflicts with an existing member name",
            Some(self.symbols[role].name_span.clone()),
            vec![RelatedLocation {
                span: self.symbols[previous].name_span.clone(),
                message: "Conflicting member".into(),
            }],
        );
    }
}
