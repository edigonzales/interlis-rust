use super::{Analyzer, Effective};
use crate::{
    Enumeration, EnumerationElement, RelatedLocation, Span, TypeInfo, syntax::EnumerationSyntax,
};
use std::collections::BTreeMap;

impl Analyzer<'_, '_> {
    pub(super) fn new_enumeration(&mut self, syntax: &EnumerationSyntax) -> Option<Enumeration> {
        if syntax.elements.is_empty() {
            self.diagnostics.push(
                "ILI-ENUM-EMPTY",
                "A new enumeration must declare an element",
                Some(syntax.span.clone()),
            );
            return None;
        }
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
                self.diagnostics.push(
                    "ILI-ENUM-PATH",
                    "A qualified enumeration name must refer to an inherited element",
                    Some(element.path[1].span.clone()),
                );
                return None;
            }
            if let Some(previous) = seen.insert(&name.value, &name.span) {
                self.diagnostics.push_related(
                    "ILI-ENUM-DUPLICATE",
                    "Duplicate element in the enumeration",
                    Some(name.span.clone()),
                    vec![RelatedLocation {
                        span: previous.clone(),
                        message: "Earlier enumeration element".into(),
                    }],
                );
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
