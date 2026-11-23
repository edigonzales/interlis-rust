use super::{Analyzer, Effective};
use crate::{
    Enumeration, EnumerationElement, EnumerationOrder, RelatedLocation, Span, TypeInfo,
    syntax::EnumerationSyntax,
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
        let ty = self.types.len();
        self.types.push(TypeInfo::EnumTree {
            domain: reference.target.clone(),
        });
        Some(Effective {
            ty,
            mandatory: false,
        })
    }
}
