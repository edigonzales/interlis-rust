use super::{
    Analyzer, Effective, Target, TypeInfo, TypeSyntax, expressions::underlying_expression,
};
use crate::{
    BoundExpressionKind, ExpressionId, ExpressionType, LanguageVersion, RelatedLocation, Span,
    TemporalBound, TemporalKind, TemporalRange,
    syntax::{FormatBase, Located},
};

impl Analyzer<'_, '_> {
    pub(super) fn temporal_format(
        &mut self,
        owner: usize,
        syntax: &TypeSyntax,
        inherited: Option<Effective>,
        resolved: Option<Effective>,
    ) -> Option<Effective> {
        let TypeSyntax::Formatted {
            base,
            range: Some(range),
            span,
        } = syntax
        else {
            unreachable!("a temporal format has bound literals");
        };
        let (base, value) = match base {
            FormatBase::Domain(name) => {
                let value = if let Some(value) = resolved {
                    value
                } else {
                    match self.bind_name(owner, name)? {
                        Target::Domain(id) => {
                            if !self.check_domain_order(owner, id, &name.span) {
                                return None;
                            }
                            self.resolve_domain(id)?
                        }
                        Target::Predefined(
                            name_value @ ("XMLDate" | "XMLTime" | "XMLDateTime"),
                        ) => self.predefined_type(owner, name_value, &name.span)?,
                        Target::Predefined(_) | Target::Structure => {
                            return self.invalid_format_base(&name.span);
                        }
                    }
                };
                (self.symbols[owner].declaration.reference.clone()?, value)
            }
            FormatBase::Inherited => {
                let Some(value) = inherited else {
                    return self.invalid_format_base(span);
                };
                (
                    self.symbols[owner].declaration.base_reference.clone()?,
                    value,
                )
            }
            FormatBase::Structure { .. } => unreachable!("structure formats are gated"),
        };
        let Some((kind, language)) = self.temporal_profile(value.ty) else {
            return self.invalid_format_base(&base.span);
        };
        let lower = self.temporal_bound(kind, language, &range.lower)?;
        let upper = self.temporal_bound(kind, language, &range.upper)?;
        if lower.components > upper.components {
            self.diagnostics.push_related(
                "ILI-FORMAT-BOUNDS",
                "The lower formatted bound exceeds the upper bound",
                Some(lower.span.clone()),
                vec![RelatedLocation {
                    span: upper.span.clone(),
                    message: "Upper formatted bound".into(),
                }],
            );
            return None;
        }
        let range = TemporalRange {
            kind,
            language,
            base,
            lower,
            upper,
        };
        let ty = TypeInfo::TemporalRange(range);
        if !temporal_specializes(&ty, &self.types[value.ty]) {
            self.diagnostics.push(
                "ILI-FORMAT-BOUNDS",
                "The formatted range cannot widen the bounds of its base format",
                Some(span.clone()),
            );
            return None;
        }
        let id = self.types.len();
        self.types.push(ty);
        Some(Effective {
            ty: id,
            mandatory: false,
        })
    }

    pub(super) fn invalid_format_base(&mut self, span: &Span) -> Option<Effective> {
        self.diagnostics.push(
            "ILI-FORMAT-BASE",
            "A formatted range requires a formatted base domain",
            Some(span.clone()),
        );
        None
    }

    pub(super) fn temporal_profile(&self, ty: usize) -> Option<(TemporalKind, LanguageVersion)> {
        match &self.types[ty] {
            TypeInfo::Temporal(kind) => Some((*kind, self.temporal_languages[&ty])),
            TypeInfo::TemporalRange(range) => Some((range.kind, range.language)),
            _ => None,
        }
    }

    pub(super) fn preserves_temporal_layout(&self, own: usize, base: usize) -> bool {
        temporal_specializes(&self.types[own], &self.types[base])
            && self
                .temporal_profile(own)
                .zip(self.temporal_profile(base))
                .is_some_and(|((_, source), (_, target))| source == target)
    }

    pub(super) fn bind_temporal_literal(
        &mut self,
        expression: ExpressionId,
        ty: usize,
    ) -> Option<()> {
        let Some((kind, language)) = self.temporal_profile(ty) else {
            return Some(());
        };
        let actual = underlying_expression(&self.expressions, expression);
        let BoundExpressionKind::Text { value, .. } = &actual.kind else {
            return Some(());
        };
        if actual.value_type != ExpressionType::Text {
            return Some(());
        }
        let at = actual.span.clone();
        let value = value.clone();
        if !self.step(value.len(), &at) {
            return None;
        }
        let components = temporal_components(kind, language, &value);
        let Some(components) = components.filter(|components| match &self.types[ty] {
            TypeInfo::TemporalRange(range) => {
                range.lower.components <= *components && *components <= range.upper.components
            }
            _ => true,
        }) else {
            self.diagnostics.push(
                "ILI-FORMAT-CONSTANT",
                "The constant must match the temporal layout and the declared value range",
                Some(at),
            );
            return None;
        };
        let mut id = expression;
        loop {
            let bound = &mut self.expressions[id.0 as usize];
            bound.type_id = Some(ty);
            bound.value_type = ExpressionType::Temporal;
            if let BoundExpressionKind::Group { expression } = bound.kind {
                id = expression;
            } else {
                bound.kind = BoundExpressionKind::Temporal { value, components };
                return Some(());
            }
        }
    }

    pub(super) fn temporal_comparable(
        &mut self,
        left: ExpressionId,
        right: ExpressionId,
    ) -> Option<bool> {
        for (typed, candidate) in [(left, right), (right, left)] {
            if let Some(ty) = self.expressions[typed.0 as usize].type_id {
                self.bind_temporal_literal(candidate, ty)?;
            }
        }
        let kind = |id: ExpressionId| {
            let ty = self.expressions[id.0 as usize].type_id?;
            self.temporal_profile(ty).map(|(kind, _)| kind)
        };
        let Some((left, right)) = kind(left).zip(kind(right)) else {
            return Some(false);
        };
        use TemporalKind::{Date, DateTime};
        Some(left == right || matches!((left, right), (Date, DateTime) | (DateTime, Date)))
    }

    fn temporal_bound(
        &mut self,
        kind: TemporalKind,
        language: LanguageVersion,
        source: &Located<String>,
    ) -> Option<TemporalBound> {
        if !self.step(source.value.len(), &source.span) {
            return None;
        }
        let Some(components) = temporal_components(kind, language, &source.value) else {
            self.diagnostics.push(
                "ILI-FORMAT-BOUNDS",
                "The bound must match the temporal layout and the ranges of its components",
                Some(source.span.clone()),
            );
            return None;
        };
        Some(TemporalBound {
            original: source.value.clone(),
            components,
            span: source.span.clone(),
        })
    }
}

fn temporal_components(
    kind: TemporalKind,
    language: LanguageVersion,
    text: &str,
) -> Option<Vec<u16>> {
    let (date, time) = match kind {
        TemporalKind::Date => (Some(text), None),
        TemporalKind::TimeOfDay => (None, Some(text)),
        TemporalKind::DateTime => {
            let (date, time) = text.split_once('T')?;
            (Some(date), Some(time))
        }
    };
    let padded = language == LanguageVersion::V24;
    let mut components = Vec::with_capacity(7);
    if let Some(date) = date {
        let (year, rest) = date.split_once('-')?;
        let (month, day) = rest.split_once('-')?;
        components.push(field(year, if padded { 4 } else { 1 }, 1582, 2999)?);
        components.push(field(month, if padded { 2 } else { 1 }, 1, 12)?);
        components.push(field(day, if padded { 2 } else { 1 }, 1, 31)?);
    }
    if let Some(time) = time {
        let (hour, rest) = time.split_once(':')?;
        let (minute, rest) = rest.split_once(':')?;
        let (second, millis) = rest.split_once('.')?;
        if millis.len() != 3 {
            return None;
        }
        components.push(field(hour, 2, 0, 23)?);
        components.push(field(minute, if padded { 2 } else { 1 }, 0, 59)?);
        let millis = field(millis, 3, 0, 999)?;
        if second.starts_with('-') && millis != 0 {
            return None;
        }
        components.push(field(second, if padded { 2 } else { 1 }, 0, 59)?);
        components.push(millis);
    }
    Some(components)
}

fn field(text: &str, width: usize, lower: u16, upper: u16) -> Option<u16> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.len() < width || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse().ok()?;
    ((lower..=upper).contains(&value) && (!text.starts_with('-') || value == 0)).then_some(value)
}

pub(super) fn temporal_specializes(own: &TypeInfo, base: &TypeInfo) -> bool {
    use TemporalKind::{Date, DateTime};
    let (own_kind, own_range) = match own {
        TypeInfo::Temporal(kind) => (*kind, None),
        TypeInfo::TemporalRange(range) => (range.kind, Some(range)),
        _ => return false,
    };
    let (base_kind, base_range) = match base {
        TypeInfo::Temporal(kind) => (*kind, None),
        TypeInfo::TemporalRange(range) => (range.kind, Some(range)),
        _ => return false,
    };
    if own_kind != base_kind && (own_kind, base_kind) != (DateTime, Date) {
        return false;
    }
    let Some(base_range) = base_range else {
        return true;
    };
    let (lower, upper): (&[u16], &[u16]) = if let Some(own_range) = own_range {
        (&own_range.lower.components, &own_range.upper.components)
    } else {
        match own_kind {
            Date => (&[1582, 1, 1], &[2999, 12, 31]),
            TemporalKind::TimeOfDay => (&[0, 0, 0, 0], &[23, 59, 59, 999]),
            DateTime => (&[1582, 1, 1, 0, 0, 0, 0], &[2999, 12, 31, 23, 59, 59, 999]),
        }
    };
    let len = base_range.lower.components.len();
    lower[..len] >= base_range.lower.components[..]
        && upper[..len] <= base_range.upper.components[..]
}
