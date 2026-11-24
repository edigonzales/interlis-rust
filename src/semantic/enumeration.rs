use super::{Analyzer, Effective};
use crate::{
    AlignmentKind, BoundExpressionKind, Enumeration, EnumerationElement, EnumerationOrder,
    ExpressionId, ExpressionType, RelatedLocation, Span, TypeInfo, syntax::EnumerationSyntax,
};
use std::collections::BTreeMap;

struct Extension<'a> {
    range: &'a Span,
    path: String,
    declared: BTreeMap<String, Span>,
}

fn copy_cost(tree: &Enumeration) -> usize {
    tree.elements().iter().fold(1, |cost, element| {
        cost.saturating_add(element.name().len() + 1)
            .saturating_add(element.children().map_or(0, copy_cost))
    })
}

fn declared_in(span: &Span, range: &Span) -> bool {
    let (source, start, end) = span.sort_key();
    let (owner, lower, upper) = range.sort_key();
    source == owner && lower <= start && end <= upper
}

impl Analyzer<'_, '_> {
    pub(super) fn new_enumeration(&mut self, syntax: &EnumerationSyntax) -> Option<Enumeration> {
        // Both editions include (FINAL) in the grammar. It also defines an
        // empty closed enumeration in ili2c and interlis-java.
        let mut result = Enumeration {
            elements: Vec::new(),
            final_subtree: syntax.final_marker.is_some(),
        };
        let mut seen = BTreeMap::new();
        for element in &syntax.elements {
            let name = &element.path[0];
            if !self.step(name.value.len() + 1, &name.span) {
                return None;
            }
            if element.path.len() > 1 {
                self.invalid_enum_path(&element.path[1].span);
                return None;
            }
            if let Some(previous) = seen.insert(&name.value, &name.span) {
                self.enum_duplicate(&name.span, previous.clone());
                return None;
            }
            let children = match &element.children {
                Some(nested) => Some(self.new_enumeration(nested)?),
                None => None,
            };
            result.elements.push(EnumerationElement {
                name: name.value.clone(),
                span: name.span.clone(),
                children,
                source_metadata: self.source_metadata(&name.span, &name.span, false)?,
            });
        }
        Some(result)
    }

    pub(super) fn extend_enumeration(
        &mut self,
        base: Effective,
        syntax: &EnumerationSyntax,
        declared_order: Option<EnumerationOrder>,
    ) -> Option<Effective> {
        let TypeInfo::Enumeration { values, order, .. } = &self.types[base.ty] else {
            self.diagnostics.push(
                "ILI-ENUM-BASE",
                "An enumeration extension requires an enumeration definition as its base",
                Some(syntax.span.clone()),
            );
            return None;
        };
        let order = *order;
        if order == Some(EnumerationOrder::Circular)
            || declared_order == Some(EnumerationOrder::Circular)
        {
            self.diagnostics.push(
                "ILI-ENUM-CIRCULAR",
                "A circular enumeration cannot be extended or introduced by an extension",
                Some(syntax.span.clone()),
            );
            return None;
        }
        if !self.step(copy_cost(values), &syntax.span) {
            return None;
        }
        let TypeInfo::Enumeration { values, .. } = &self.types[base.ty] else {
            unreachable!("enumeration base already identified");
        };
        let mut values = values.clone();
        let mut extension = Extension {
            range: &syntax.span,
            path: String::new(),
            declared: BTreeMap::new(),
        };
        self.merge_enumeration(&mut values, syntax, &mut extension, 0)?;
        let ty = self.types.len();
        self.enum_bases.insert(ty, base.ty);
        self.types.push(TypeInfo::Enumeration {
            values,
            order: declared_order.or(order),
            declared_order,
        });
        Some(Effective {
            ty,
            mandatory: base.mandatory,
        })
    }

    fn merge_enumeration(
        &mut self,
        tree: &mut Enumeration,
        syntax: &EnumerationSyntax,
        extension: &mut Extension<'_>,
        depth: usize,
    ) -> Option<()> {
        for item in &syntax.elements {
            let (last, qualifiers) = item
                .path
                .split_last()
                .expect("enumeration element has a name");
            let depth = depth.saturating_add(item.path.len());
            if depth > self.limits.max_nesting as usize {
                self.diagnostics.limit(
                    "Effective enumeration nesting exceeds the configured limit",
                    Some(item.span.clone()),
                );
                return None;
            }
            let path_start = extension.path.len();
            let bytes = item
                .path
                .iter()
                .map(|name| name.value.len() + 1)
                .sum::<usize>();
            if !self.step(bytes.saturating_add(path_start), &item.span) {
                return None;
            }
            for part in &item.path {
                if !extension.path.is_empty() {
                    extension.path.push('.');
                }
                extension.path.push_str(&part.value);
            }
            let mut siblings = &mut *tree;
            for qualifier in qualifiers {
                if !self.step(siblings.elements.len(), &qualifier.span) {
                    return None;
                }
                let Some(children) = siblings
                    .elements
                    .iter_mut()
                    .find(|element| element.name == qualifier.value)
                    .and_then(|element| element.children.as_mut())
                else {
                    self.invalid_enum_path(&qualifier.span);
                    return None;
                };
                siblings = children;
            }
            if !self.step(siblings.elements.len() + 1, &last.span) {
                return None;
            }
            let existing = siblings
                .elements
                .iter()
                .position(|element| element.name == last.value);
            if !qualifiers.is_empty()
                && existing.is_none_or(|index| {
                    declared_in(&siblings.elements[index].span, extension.range)
                })
            {
                self.invalid_enum_path(&last.span);
                return None;
            }
            if let Some(prior) = extension
                .declared
                .insert(extension.path.clone(), last.span.clone())
            {
                self.enum_duplicate(&last.span, prior);
                return None;
            }

            let element = if let Some(index) = existing {
                let element = &mut siblings.elements[index];
                if item.children.is_none() {
                    self.enum_duplicate(&last.span, element.span.clone());
                    return None;
                }
                element
            } else {
                if siblings.final_subtree {
                    self.diagnostics.push(
                        "ILI-ENUM-FINAL",
                        "Cannot add an element to a FINAL enumeration",
                        Some(last.span.clone()),
                    );
                    return None;
                }
                siblings.elements.push(EnumerationElement {
                    name: last.value.clone(),
                    span: last.span.clone(),
                    children: None,
                    source_metadata: self.source_metadata(&item.path[0].span, &last.span, false)?,
                });
                siblings.elements.last_mut().expect("element just appended")
            };
            if let Some(patch) = &item.children {
                let children = element.children.get_or_insert_with(|| Enumeration {
                    elements: Vec::new(),
                    final_subtree: false,
                });
                self.merge_enumeration(children, patch, extension, depth)?;
            }
            extension.path.truncate(path_start);
        }
        tree.final_subtree |= syntax.final_marker.is_some();
        Some(())
    }

    fn invalid_enum_path(&mut self, span: &Span) {
        self.diagnostics.push(
            "ILI-ENUM-PATH",
            "A qualified enumeration name must refer to an inherited element",
            Some(span.clone()),
        );
    }

    fn enum_duplicate(&mut self, span: &Span, previous: Span) {
        self.diagnostics.push_related(
            "ILI-ENUM-DUPLICATE",
            "Duplicate element in the enumeration",
            Some(span.clone()),
            vec![RelatedLocation {
                span: previous,
                message: "Earlier enumeration element".into(),
            }],
        );
    }

    pub(super) fn enum_tree_type(
        &mut self,
        owner: usize,
        base: Effective,
        span: &Span,
    ) -> Option<Effective> {
        // Appendix A defines BOOLEAN and the alignments as enumerations.
        match &self.types[base.ty] {
            TypeInfo::Enumeration { .. } | TypeInfo::Boolean | TypeInfo::Alignment(_) => {}
            _ => {
                self.diagnostics.push(
                    "ILI-ENUM-TARGET",
                    "ALL OF requires an enumeration domain",
                    Some(span.clone()),
                );
                return None;
            }
        }
        let reference = self.symbols[owner]
            .declaration
            .reference
            .as_ref()
            .expect("enumeration domain reference has been bound");
        let domain = reference.target.clone();
        let ty = self.new_enum_tree(base.ty, Some(domain));
        Some(Effective {
            ty,
            mandatory: false,
        })
    }

    pub(super) fn new_enum_tree(&mut self, base: usize, domain: Option<String>) -> usize {
        let ty = self.types.len();
        self.types.push(TypeInfo::EnumTree { domain, base });
        ty
    }

    fn enum_root(&self, ty: usize) -> (usize, bool) {
        if let TypeInfo::EnumTree { base, .. } = self.types[ty] {
            (base, true)
        } else {
            (ty, false)
        }
    }

    pub(super) fn is_enum_type(&self, ty: usize) -> bool {
        matches!(
            self.types[ty],
            TypeInfo::Enumeration { .. }
                | TypeInfo::EnumTree { .. }
                | TypeInfo::Alignment(_)
                | TypeInfo::Boolean
        )
    }

    pub(super) fn enum_is_ordered(&self, ty: usize) -> bool {
        matches!(
            self.types[self.enum_root(ty).0],
            TypeInfo::Enumeration {
                order: Some(EnumerationOrder::Ordered),
                ..
            } | TypeInfo::Alignment(_)
                | TypeInfo::Boolean
        )
    }

    pub(super) fn enum_specializes(
        &mut self,
        source: usize,
        target: usize,
        at: &Span,
    ) -> Option<bool> {
        let (mut source, source_tree) = self.enum_root(source);
        let (target, target_tree) = self.enum_root(target);
        if source_tree && !target_tree {
            return Some(false);
        }
        loop {
            if !self.step(1, at) {
                return None;
            }
            if source == target
                || match (&self.types[source], &self.types[target]) {
                    (TypeInfo::Boolean, TypeInfo::Boolean) => true,
                    (TypeInfo::Alignment(a), TypeInfo::Alignment(b)) => a == b,
                    _ => false,
                }
            {
                return Some(true);
            }
            let Some(&base) = self.enum_bases.get(&source) else {
                return Some(false);
            };
            source = base;
        }
    }

    pub(super) fn enum_existence_compatible(
        &mut self,
        source: usize,
        target: usize,
        at: &Span,
    ) -> Option<bool> {
        if self.enum_specializes(source, target, at)? {
            return Some(true);
        }
        let (source, source_tree) = self.enum_root(source);
        let (target, target_tree) = self.enum_root(target);
        if source_tree && !target_tree {
            return Some(false);
        }
        let (
            TypeInfo::Enumeration {
                values: source,
                order: a,
                ..
            },
            TypeInfo::Enumeration {
                values: target,
                order: b,
                ..
            },
        ) = (&self.types[source], &self.types[target])
        else {
            return Some(false);
        };
        if a != b {
            return Some(false);
        }
        match same_enum_values(source, target, &mut self.work) {
            Ok(equal) => Some(equal),
            Err(()) => {
                self.diagnostics
                    .limit("Semantic work budget exceeded", Some(at.clone()));
                None
            }
        }
    }

    pub(super) fn bind_enum_literal(&mut self, expression: ExpressionId, ty: usize) -> Option<()> {
        let actual = super::expressions::underlying_expression(&self.expressions, expression);
        if actual.type_id.is_some() {
            return Some(());
        }
        let path = match &actual.kind {
            BoundExpressionKind::Enumeration { path } => path.clone(),
            BoundExpressionKind::Boolean { value } => vec![value.to_string()],
            _ => return Some(()),
        };
        let at = actual.span.clone();
        let matches = self.enum_contains(ty, &path, &at)?;
        if !matches {
            self.diagnostics.push(
                "ILI-ENUM-CONSTANT",
                "The constant is not a member of the expected enumeration",
                Some(at),
            );
            return None;
        }
        self.set_enum_literal(expression, path, Some(ty));
        Some(())
    }

    pub(super) fn retain_symbolic_enum(&mut self, expression: ExpressionId) {
        let actual = super::expressions::underlying_expression(&self.expressions, expression);
        if actual.type_id.is_none()
            && let BoundExpressionKind::Boolean { value } = actual.kind
        {
            self.set_enum_literal(expression, vec![value.to_string()], None);
        }
    }

    fn set_enum_literal(&mut self, expression: ExpressionId, path: Vec<String>, ty: Option<usize>) {
        let value_type = ty.map_or(ExpressionType::Enumeration, |id| {
            super::expressions::scalar_type(&self.types[id])
        });
        let mut id = expression;
        loop {
            let value = &mut self.expressions[id.0 as usize];
            value.type_id = ty;
            value.value_type = value_type;
            if let BoundExpressionKind::Group { expression } = value.kind {
                id = expression;
            } else {
                if value_type == ExpressionType::Boolean {
                    value.kind = BoundExpressionKind::Boolean {
                        value: path[0] == "true",
                    };
                } else {
                    value.kind = BoundExpressionKind::Enumeration { path };
                }
                return;
            }
        }
    }

    pub(super) fn enum_contains(&mut self, ty: usize, path: &[String], at: &Span) -> Option<bool> {
        let (base, tree_values) = self.enum_root(ty);
        let names: &[&str] = match &self.types[base] {
            TypeInfo::Enumeration { values, .. } => {
                let mut tree = values;
                for (index, name) in path.iter().enumerate() {
                    let comparisons = tree.elements.len().saturating_mul(name.len() + 1);
                    if self
                        .work
                        .charge(u32::try_from(comparisons).unwrap_or(u32::MAX))
                        .is_err()
                    {
                        self.diagnostics
                            .limit("Semantic work budget exceeded", Some(at.clone()));
                        return None;
                    }
                    let Some(element) = tree.elements.iter().find(|element| element.name == *name)
                    else {
                        return Some(false);
                    };
                    if index + 1 == path.len() {
                        return Some(tree_values || element.children.is_none());
                    }
                    let Some(children) = &element.children else {
                        return Some(false);
                    };
                    tree = children;
                }
                return Some(false);
            }
            TypeInfo::Boolean => &["false", "true"],
            TypeInfo::Alignment(AlignmentKind::Horizontal) => &["Left", "Center", "Right"],
            TypeInfo::Alignment(AlignmentKind::Vertical) => {
                &["Top", "Cap", "Half", "Base", "Bottom"]
            }
            _ => return Some(false),
        };
        Some(path.len() == 1 && names.contains(&path[0].as_str()))
    }
}

fn same_enum_values(
    left: &Enumeration,
    right: &Enumeration,
    work: &mut crate::limits::WorkBudget,
) -> Result<bool, ()> {
    if left.elements.len() != right.elements.len() {
        return Ok(false);
    }
    for (left, right) in left.elements.iter().zip(&right.elements) {
        work.charge((left.name.len() + right.name.len()) as u32)?;
        if left.name != right.name {
            return Ok(false);
        }
        match (&left.children, &right.children) {
            (None, None) => {}
            (Some(left), Some(right)) if same_enum_values(left, right, work)? => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}
