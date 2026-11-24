use super::{Analyzer, SyntaxDecl, TypeInfo};
use crate::{AttributeSubdivision, RelatedLocation, Span};
use std::collections::BTreeSet;

impl Analyzer<'_, '_> {
    pub(super) fn validate_subdivisions(&mut self, viewables: &[usize]) -> Option<()> {
        let mut checked = BTreeSet::new();
        for &owner in viewables {
            let len = self.symbols[owner]
                .declaration
                .effective_attributes
                .as_ref()
                .map_or(0, |attributes| attributes.len());
            let mut predecessor = None;
            for index in 0..len {
                let attribute = self.symbols[owner]
                    .declaration
                    .effective_attributes
                    .as_ref()
                    .expect("a nonempty attribute list was checked")[index];
                let preceding = predecessor.replace(attribute);
                let Some(SyntaxDecl::Attribute {
                    subdivision: Some(marker),
                    ..
                }) = self.symbols[attribute].ast
                else {
                    continue;
                };
                if !self.step(1, &marker.span) {
                    return None;
                }
                if checked.insert((attribute, preceding)) {
                    self.validate_subdivision(
                        attribute,
                        preceding,
                        &marker.span,
                        marker.continuous.as_ref(),
                    )?;
                    self.symbols[attribute].declaration.subdivision = Some(AttributeSubdivision {
                        continuous: marker.continuous.is_some(),
                        span: marker.span.clone(),
                    });
                }
            }
        }
        Some(())
    }

    fn validate_subdivision(
        &mut self,
        attribute: usize,
        predecessor: Option<usize>,
        span: &Span,
        continuous: Option<&Span>,
    ) -> Option<()> {
        let Some(predecessor) = predecessor else {
            self.diagnostics.push(
                "ILI-SUBDIVISION-PREDECESSOR",
                "A subdivision requires a preceding attribute",
                Some(span.clone()),
            );
            return None;
        };
        let own = self.symbols[attribute].declaration.type_id?;
        let parent = self.symbols[predecessor].declaration.type_id?;
        let error = match (&self.types[own], &self.types[parent]) {
            (TypeInfo::Numeric(own_type), TypeInfo::Numeric(parent_type)) => {
                if parent_type
                    .bounds
                    .as_ref()
                    .is_some_and(|bounds| bounds.lower.scale() > 0 || bounds.upper.scale() > 0)
                {
                    Some((
                        "ILI-SUBDIVISION-INTEGER",
                        "A subdivision requires an integer-valued preceding attribute",
                    ))
                } else if own_type
                    .bounds
                    .as_ref()
                    .is_some_and(|bounds| bounds.lower.coefficient().starts_with('-'))
                {
                    Some((
                        "ILI-SUBDIVISION-RANGE",
                        "A subdivision requires a nonnegative range",
                    ))
                } else if let Some(continuous) = continuous {
                    if !own_type.circular {
                        Some((
                            "ILI-SUBDIVISION-CIRCULAR",
                            "A continuous subdivision requires CIRCULAR",
                        ))
                    } else if !self.subdivision_unit_period(parent, own, continuous)? {
                        Some((
                            "ILI-SUBDIVISION-FACTOR",
                            "The subdivision period must equal the unit conversion factor",
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            (TypeInfo::Numeric(_), _) => Some((
                "ILI-SUBDIVISION-PREDECESSOR-TYPE",
                "A subdivision requires a preceding scalar numeric attribute",
            )),
            _ => Some((
                "ILI-SUBDIVISION-TYPE",
                "A subdivision must have a scalar numeric type",
            )),
        };
        if let Some((code, message)) = error {
            self.diagnostics.push_related(
                code,
                message,
                Some(span.clone()),
                vec![RelatedLocation {
                    span: self.symbols[predecessor].name_span.clone(),
                    message: "Preceding effective attribute".into(),
                }],
            );
            return None;
        }
        Some(())
    }
}
