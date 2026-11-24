use super::{
    Analyzer, BoundName, Effective, Kind, Symbol, TypeInfo,
    collections::{compare_count, structure_targets},
    expressions::underlying_expression,
    functions::compare_decimal,
};
use crate::{
    BoundExpressionKind, DecimalInfo, ExpressionId, ExpressionType, FormattedBound, FormattedPart,
    FormattedStructurePart, FormattedType, LanguageVersion, Limits, NumericBounds, Span,
    TemporalKind, TypeReference,
    limits::WorkBudget,
    number::{DecimalLiteral, NumberError},
    syntax::{FormatAttribute, FormatDefinition, FormatPart, Name, QualifiedName, StringRange},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

impl Analyzer<'_, '_> {
    pub(super) fn structure_format(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        syntax: &FormatDefinition,
        range: Option<&StringRange>,
        inherited: Option<Effective>,
        at: &Span,
    ) -> Option<Effective> {
        let structure = self.structure_reference(owner, name)?;
        let scope = self.names[structure.target_name()];
        let base = inherited.map(|value| value.ty);
        let mut parts = Vec::new();
        match (base, &syntax.inheritance) {
            (Some(base), Some(location)) => {
                let TypeInfo::Formatted(format) = &self.types[base] else {
                    return self.format_error(
                        "ILI-FORMAT-INHERITANCE",
                        "INHERITANCE requires a formatted base",
                        location,
                    );
                };
                let base_scope = self.names[format.structure.target_name()];
                if !self.specializes_symbol(scope, base_scope, &name.span)? {
                    return self.format_error(
                        "ILI-FORMAT-INHERITANCE",
                        "The format structure must specialize the inherited structure",
                        &name.span,
                    );
                }
                parts = self.clone_format_parts(base, location)?;
                for part in &mut parts {
                    if let FormattedPart::Numeric { attribute, .. }
                    | FormattedPart::Structure(FormattedStructurePart { attribute, .. }) = part
                    {
                        let original = self.names[attribute.target_name()];
                        let name = Name {
                            value: self.symbols[original].declaration.name.clone(),
                            span: attribute.source_span().clone(),
                        };
                        *attribute = self.format_member(scope, &name)?;
                    }
                }
            }
            (None, None) => {}
            _ => {
                return self.format_error(
                    "ILI-FORMAT-INHERITANCE",
                    "A format extension requires INHERITANCE and an inherited format",
                    syntax.inheritance.as_ref().unwrap_or(&syntax.span),
                );
            }
        }
        for part in &syntax.parts {
            match part {
                FormatPart::Literal(literal) => {
                    if !self.step(literal.value.len() + 1, &literal.span) {
                        return None;
                    }
                    parts.push(FormattedPart::Literal {
                        value: literal.value.clone(),
                        span: literal.span.clone(),
                    });
                }
                FormatPart::Attribute {
                    attribute: FormatAttribute::Numeric { name, width },
                    ..
                } => {
                    let width = if let Some(width) = width {
                        let value = width.value.trim_start_matches('0');
                        if value.is_empty() {
                            return self.format_error(
                                "ILI-FORMAT-WIDTH",
                                "FORMAT width must be positive",
                                &width.span,
                            );
                        }
                        if !self.step(value.len(), &width.span) {
                            return None;
                        }
                        Some(value.to_owned())
                    } else {
                        None
                    };
                    parts.push(FormattedPart::Numeric {
                        attribute: self.format_member(scope, name)?,
                        width,
                    });
                }
                FormatPart::Attribute {
                    attribute: FormatAttribute::Formatted { name, domain },
                    ..
                } => {
                    let attribute = self.format_member(scope, name)?;
                    let target =
                        self.expression_name(owner, domain, &[Kind::Domain], "ILI-FORMAT-DOMAIN")?;
                    let ty = match target {
                        BoundName::Declared(id) => {
                            if !self.check_domain_order(owner, id, &domain.span) {
                                return None;
                            }
                            self.resolve_domain(id)?.ty
                        }
                        BoundName::Predefined(builtin, _) => {
                            self.predefined_type(owner, builtin, &domain.span)?.ty
                        }
                    };
                    let type_id = if matches!(self.types[ty], TypeInfo::Formatted(_)) {
                        ty
                    } else if let Some((kind, language)) = self.temporal_profile(ty) {
                        self.temporal_structure_format(owner, ty, kind, language, &domain.span)?
                    } else {
                        return self.format_error(
                            "ILI-FORMAT-DOMAIN",
                            "A structure component requires a formatted domain",
                            &domain.span,
                        );
                    };
                    parts.push(FormattedPart::Structure(FormattedStructurePart {
                        attribute,
                        format: self.bound_reference(owner, domain, target)?,
                        type_id,
                    }));
                }
            }
        }
        let mut separated = true;
        for part in &parts {
            match part {
                FormattedPart::Literal { value, .. } => separated |= !value.is_empty(),
                FormattedPart::Numeric { attribute, .. }
                | FormattedPart::Structure(FormattedStructurePart { attribute, .. }) => {
                    if !separated {
                        return self.format_error(
                            "ILI-FORMAT-SEPARATOR",
                            "Format components require a nonempty separator",
                            attribute.source_span(),
                        );
                    }
                    separated = false;
                }
            }
        }
        self.store_format(structure, parts, range, base, at)
    }

    fn temporal_structure_format(
        &mut self,
        owner: usize,
        ty: usize,
        kind: TemporalKind,
        language: LanguageVersion,
        at: &Span,
    ) -> Option<usize> {
        let span = Span::predefined();
        let name = match kind {
            TemporalKind::Date => "GregorianDate",
            TemporalKind::DateTime => "GregorianDateTime",
            TemporalKind::TimeOfDay => "UTC",
        };
        let scope =
            self.bound_structure(owner, BoundName::Predefined(name, Kind::Structure), at)?;
        let mut fields = Vec::new();
        if kind != TemporalKind::TimeOfDay {
            fields.extend([("", "Year", 4), ("-", "Month", 2), ("-", "Day", 2)]);
        }
        if kind != TemporalKind::Date {
            let preceding = if kind == TemporalKind::DateTime {
                "T"
            } else {
                ""
            };
            fields.extend([
                (preceding, "Hours", 2),
                (":", "Minutes", 2),
                (":", "Seconds", 2),
            ]);
        }
        let mut parts = Vec::new();
        for (separator, field, width) in fields {
            if !separator.is_empty() {
                parts.push(FormattedPart::Literal {
                    value: separator.into(),
                    span: span.clone(),
                });
            }
            let field_name = Name {
                value: field.into(),
                span: span.clone(),
            };
            parts.push(FormattedPart::Numeric {
                attribute: self.format_member(scope, &field_name)?,
                width: (language == LanguageVersion::V24 || field == "Hours")
                    .then(|| width.to_string()),
            });
        }
        let bounds = if let TypeInfo::TemporalRange(range) = &self.types[ty] {
            let part = |bound: &crate::TemporalBound| FormattedBound {
                original: bound.original.clone(),
                components: Vec::new(),
                span: bound.span.clone(),
            };
            Some((part(&range.lower), part(&range.upper)))
        } else {
            None
        };
        let id = self.types.len();
        let mut structure = self.predefined_reference(owner, name);
        structure.predefined_language = Some(language);
        self.types.push(TypeInfo::Formatted(Box::new(FormattedType {
            structure,
            parts,
            bounds,
            base: None,
            span,
        })));
        Some(id)
    }

    pub(super) fn restrict_format(
        &mut self,
        base: Effective,
        range: &StringRange,
        at: &Span,
    ) -> Option<Effective> {
        let parts = self.clone_format_parts(base.ty, at)?;
        let TypeInfo::Formatted(format) = &self.types[base.ty] else {
            unreachable!();
        };
        let structure = format.structure.clone();
        self.store_format(structure, parts, Some(range), Some(base.ty), at)
    }

    fn clone_format_parts(&mut self, id: usize, at: &Span) -> Option<Vec<FormattedPart>> {
        let TypeInfo::Formatted(format) = &self.types[id] else {
            unreachable!();
        };
        let bytes = format
            .parts
            .iter()
            .map(|part| match part {
                FormattedPart::Literal { value, .. } => 1 + value.len(),
                FormattedPart::Numeric { attribute, width } => {
                    1 + attribute.declared_name().len()
                        + attribute.target_name().len()
                        + width.as_ref().map_or(0, String::len)
                }
                FormattedPart::Structure(FormattedStructurePart {
                    attribute, format, ..
                }) => {
                    1 + attribute.declared_name().len()
                        + attribute.target_name().len()
                        + format.declared_name().len()
                        + format.target_name().len()
                }
            })
            .sum();
        if !self.step(bytes, at) {
            return None;
        }
        let TypeInfo::Formatted(format) = &self.types[id] else {
            unreachable!();
        };
        Some(format.parts.clone())
    }

    fn format_member(&mut self, scope: usize, name: &Name) -> Option<TypeReference> {
        let Some(id) = self
            .visible_name(scope, &name.value, Some(&[Kind::Attribute]), &name.span)
            .ok()?
        else {
            return self.format_error(
                "ILI-FORMAT-ATTRIBUTE",
                "The attribute is absent from the format structure",
                &name.span,
            );
        };
        self.reference_to_symbol(scope, id, name)
    }

    fn store_format(
        &mut self,
        structure: TypeReference,
        parts: Vec<FormattedPart>,
        range: Option<&StringRange>,
        base: Option<usize>,
        at: &Span,
    ) -> Option<Effective> {
        let bounds = if let Some(range) = range {
            if !self.step(range.lower.value.len() + range.upper.value.len(), at) {
                return None;
            }
            let bound = |part: &crate::syntax::Located<String>| FormattedBound {
                original: part.value.clone(),
                components: Vec::new(),
                span: part.span.clone(),
            };
            Some((bound(&range.lower), bound(&range.upper)))
        } else {
            None
        };
        let ty = self.types.len();
        self.types.push(TypeInfo::Formatted(Box::new(FormattedType {
            structure,
            parts,
            bounds,
            base,
            span: at.clone(),
        })));
        Some(Effective {
            ty,
            mandatory: false,
        })
    }

    pub(super) fn format_extends(
        &mut self,
        mut own: usize,
        base: usize,
        at: &Span,
    ) -> Option<bool> {
        if !matches!(self.types[base], TypeInfo::Formatted(_)) {
            return Some(false);
        }
        while let TypeInfo::Formatted(format) = &self.types[own] {
            if own == base {
                return Some(true);
            }
            let Some(parent) = format.base else {
                break;
            };
            if !self.step(1, at) {
                return None;
            }
            own = parent;
        }
        Some(false)
    }

    pub(super) fn validate_formatted_types(
        &mut self,
        start: usize,
        abstract_types: &mut BTreeSet<usize>,
    ) -> Option<()> {
        for ty in start..self.types.len() {
            let TypeInfo::Formatted(format) = &self.types[ty] else {
                continue;
            };
            let at = format.span.clone();
            let parts = self.clone_format_parts(ty, &at)?;
            let mut abstract_type = false;
            for part in &parts {
                let attribute = match part {
                    FormattedPart::Literal { .. } => continue,
                    FormattedPart::Numeric { attribute, .. } => attribute,
                    FormattedPart::Structure(FormattedStructurePart {
                        attribute, type_id, ..
                    }) => {
                        self.check_formatted_component(attribute, *type_id)?;
                        abstract_type |= abstract_types.contains(type_id);
                        continue;
                    }
                };
                let member = &self.symbols[self.names[attribute.target_name()]].declaration;
                let Some(TypeInfo::Numeric(number)) = member.type_id.map(|id| &self.types[id])
                else {
                    let location = attribute.source_span().clone();
                    return self.format_error(
                        "ILI-FORMAT-ATTRIBUTE",
                        "A numeric format component requires a numeric attribute",
                        &location,
                    );
                };
                if let Some(bounds) = &number.bounds {
                    if bounds.lower.fractional_digits() != bounds.upper.fractional_digits() {
                        let location = attribute.source_span().clone();
                        return self.format_error(
                            "ILI-FORMAT-PRECISION",
                            "Numeric format components require matching bound precision",
                            &location,
                        );
                    }
                } else {
                    abstract_type = true;
                }
            }
            if abstract_type {
                abstract_types.insert(ty);
            }
            let TypeInfo::Formatted(format) = &self.types[ty] else {
                unreachable!()
            };
            let Some((lower, upper)) = &format.bounds else {
                continue;
            };
            let lower_text = lower.original.clone();
            let upper_text = upper.original.clone();
            let lower_span = lower.span.clone();
            let upper_span = upper.span.clone();
            let lower = self.format_value(ty, &lower_text, &lower_span)?;
            let upper = self.format_value(ty, &upper_text, &upper_span)?;
            let TypeInfo::Formatted(format) = &self.types[ty] else {
                unreachable!()
            };
            let at = format.span.clone();
            let result = (|| {
                if compare_components(&lower, &upper, &mut self.work)? == Ordering::Greater {
                    return Ok(false);
                }
                let mut ancestor = format.base;
                while let Some(id) = ancestor {
                    self.work.charge(1).map_err(|_| NumberError::Work)?;
                    let TypeInfo::Formatted(base) = &self.types[id] else {
                        unreachable!()
                    };
                    if let Some((min, max)) = &base.bounds
                        && (compare_components(&lower, &min.components, &mut self.work)?
                            == Ordering::Less
                            || compare_components(&upper, &max.components, &mut self.work)?
                                == Ordering::Greater)
                    {
                        return Ok(false);
                    }
                    ancestor = base.base;
                }
                Ok(true)
            })();
            match result {
                Ok(true) => {}
                Ok(false) => {
                    return self.format_error(
                        "ILI-FORMAT-BOUNDS",
                        "The format bounds must be ordered and must preserve the inherited range",
                        &at,
                    );
                }
                Err(error) => {
                    self.number_error(error, &at);
                    return None;
                }
            }
            let TypeInfo::Formatted(format) = &mut self.types[ty] else {
                unreachable!()
            };
            let (min, max) = format.bounds.as_mut()?;
            min.components = lower;
            max.components = upper;
        }
        if !abstract_types.is_empty() {
            for symbol in &self.symbols {
                let decl = &symbol.declaration;
                if decl.is_abstract() || !matches!(decl.kind, Kind::Attribute | Kind::Domain) {
                    continue;
                }
                if let Some(mut ty) = decl.type_id {
                    while let TypeInfo::Collection(collection) = &self.types[ty] {
                        ty = collection.element;
                    }
                    if abstract_types.contains(&ty) {
                        let at = decl.source_span().clone();
                        return self.format_error(
                            "ILI-FORMAT-ABSTRACT",
                            "A format with abstract components requires an abstract declaration",
                            &at,
                        );
                    }
                }
            }
        }
        Some(())
    }

    pub(super) fn bind_formatted_literal(&mut self, mut id: ExpressionId, ty: usize) -> Option<()> {
        if !matches!(self.types[ty], TypeInfo::Formatted(_)) {
            return Some(());
        }
        let actual = underlying_expression(&self.expressions, id);
        let BoundExpressionKind::Text { value, .. } = &actual.kind else {
            return Some(());
        };
        if actual.value_type != ExpressionType::Text {
            return Some(());
        }
        let at = actual.span.clone();
        let value = value.clone();
        let parsed = self.parse_formatted(ty, &value, &at)?;
        let Some(components) = parsed else {
            return self.format_constant_error(&at);
        };
        for (lower, upper) in self.formatted_ranges(ty, &at)? {
            let result = (|| {
                Ok(
                    compare_components(&components, &lower.components, &mut self.work)?
                        != Ordering::Less
                        && compare_components(&components, &upper.components, &mut self.work)?
                            != Ordering::Greater,
                )
            })();
            match result {
                Ok(true) => {}
                Ok(false) => return self.format_constant_error(&at),
                Err(error) => {
                    self.number_error(error, &at);
                    return None;
                }
            }
        }
        loop {
            let bound = &mut self.expressions[id.0 as usize];
            bound.type_id = Some(ty);
            bound.value_type = ExpressionType::Formatted;
            if let BoundExpressionKind::Group { expression } = bound.kind {
                id = expression;
            } else {
                bound.kind = BoundExpressionKind::Formatted { value, components };
                return Some(());
            }
        }
    }

    fn format_constant_error(&mut self, at: &Span) -> Option<()> {
        self.format_error(
            "ILI-FORMAT-CONSTANT",
            "The constant must match the formatted layout and its declared ranges",
            at,
        )
    }

    pub(super) fn formatted_comparable(
        &mut self,
        left: ExpressionId,
        right: ExpressionId,
        at: &Span,
    ) -> Option<bool> {
        for (typed, candidate) in [(left, right), (right, left)] {
            if let Some(ty) = self.expressions[typed.0 as usize].type_id {
                self.bind_formatted_literal(candidate, ty)?;
            }
        }
        let Some((left, right)) = self.expressions[left.0 as usize]
            .type_id
            .zip(self.expressions[right.0 as usize].type_id)
        else {
            return Some(false);
        };
        Some(
            self.format_extends(left, right, at)?
                || self.format_extends(right, left, at)?
                || formats_share_members(&self.types[left], &self.types[right], &mut self.work)
                    .map_err(|error| self.number_error(error, at))
                    .ok()?,
        )
    }

    pub(super) fn formatted_assignment(
        &mut self,
        source: usize,
        target: usize,
        at: &Span,
    ) -> Option<bool> {
        if self.format_extends(source, target, at)? {
            return Some(true);
        }
        let result = (|| {
            if !formats_share_members(&self.types[source], &self.types[target], &mut self.work)? {
                return Ok(false);
            }
            let (TypeInfo::Formatted(a), TypeInfo::Formatted(b)) =
                (&self.types[source], &self.types[target])
            else {
                unreachable!()
            };
            Ok(a.structure.target_name() == b.structure.target_name())
        })();
        match result {
            Ok(true) => {}
            Ok(false) => return Some(false),
            Err(error) => {
                self.number_error(error, at);
                return None;
            }
        }
        let source = self.formatted_interval(source, at)?;
        let target = self.formatted_interval(target, at)?;
        let result = (|| {
            Ok(
                compare_extrema(&source.lower, &target.lower, Ordering::Less, &mut self.work)?
                    != Ordering::Less
                    && compare_extrema(
                        &source.upper,
                        &target.upper,
                        Ordering::Greater,
                        &mut self.work,
                    )? != Ordering::Greater,
            )
        })();
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.number_error(error, at);
                None
            }
        }
    }

    fn formatted_interval(&mut self, ty: usize, at: &Span) -> Option<FormatInterval> {
        let ranges = self.formatted_ranges(ty, at)?;
        let result = (|| {
            let operations =
                format_operations(ty, &self.types, &self.symbols, &self.names, &mut self.work)?;
            let mut interval = FormatInterval::default();
            for operation in operations {
                self.work.charge(1).map_err(|_| NumberError::Work)?;
                match operation {
                    FormatOperation::Literal(_) => {}
                    FormatOperation::Numeric(number, _) => {
                        interval
                            .lower
                            .push(number.bounds.as_ref().map(|range| range.lower.clone()));
                        interval
                            .upper
                            .push(number.bounds.as_ref().map(|range| range.upper.clone()));
                    }
                    FormatOperation::Bounds {
                        first,
                        lower,
                        upper,
                    } => {
                        interval.narrow(first, lower, upper, &mut self.work)?;
                    }
                }
            }
            for (min, max) in &ranges {
                interval.narrow(0, &min.components, &max.components, &mut self.work)?;
            }
            Ok(interval)
        })();
        match result {
            Ok(interval) => Some(interval),
            Err(error) => {
                self.number_error(error, at);
                None
            }
        }
    }

    fn formatted_ranges(
        &mut self,
        mut ty: usize,
        at: &Span,
    ) -> Option<Vec<(FormattedBound, FormattedBound)>> {
        let mut result = Vec::new();
        while let TypeInfo::Formatted(format) = &self.types[ty] {
            let base = format.base;
            if let Some((min, max)) = &format.bounds {
                let (mut min, mut max) = (min.clone(), max.clone());
                if !self.step(min.original.len() + max.original.len(), at) {
                    return None;
                }
                if min.components.is_empty() {
                    min.components = self.format_value(ty, &min.original, &min.span)?;
                    max.components = self.format_value(ty, &max.original, &max.span)?;
                }
                result.push((min, max));
            }
            let Some(parent) = base else { break };
            if !self.step(1, at) {
                return None;
            }
            ty = parent;
        }
        Some(result)
    }

    fn check_formatted_component(
        &mut self,
        attribute: &TypeReference,
        format: usize,
    ) -> Option<()> {
        let at = attribute.source_span();
        let owner = self.names[attribute.target_name()];
        let source = self.symbols[owner]
            .declaration
            .type_id
            .map(|ty| &self.types[ty]);
        let source = match source {
            Some(TypeInfo::Collection(collection))
                if collection.cardinality.upper.as_deref() == Some("1") =>
            {
                Some(&self.types[collection.element])
            }
            value => value,
        };
        let Some(TypeInfo::Structure(structure)) = source else {
            return self.format_error(
                "ILI-FORMAT-ATTRIBUTE",
                "A nested format requires a structure-valued attribute",
                at,
            );
        };
        let TypeInfo::Formatted(format) = &self.types[format] else {
            unreachable!()
        };
        let expected = self.names[format.structure.target_name()];
        let targets: Vec<_> = structure_targets(structure)
            .iter()
            .map(|target| self.names[target.target_name()])
            .collect();
        if targets.is_empty() {
            return self.format_error(
                "ILI-FORMAT-STRUCTURE",
                "The component must have a concrete structure scope",
                at,
            );
        }
        for target in targets {
            if !self.specializes_symbol(target, expected, at)? {
                return self.format_error(
                    "ILI-FORMAT-STRUCTURE",
                    "The component structure must specialize the nested format's structure",
                    at,
                );
            }
        }
        Some(())
    }

    fn format_value(&mut self, ty: usize, text: &str, at: &Span) -> Option<Vec<DecimalInfo>> {
        match self.parse_formatted(ty, text, at)? {
            Some(value) => Some(value),
            None => self.format_error(
                "ILI-FORMAT-BOUNDS",
                "The formatted value must match the layout and the numeric component ranges",
                at,
            ),
        }
    }

    fn parse_formatted(
        &mut self,
        ty: usize,
        text: &str,
        at: &Span,
    ) -> Option<Option<Vec<DecimalInfo>>> {
        if !self.step(text.len(), at) {
            return None;
        }
        let parts =
            match format_operations(ty, &self.types, &self.symbols, &self.names, &mut self.work) {
                Ok(parts) => parts,
                Err(NumberError::Form) => {
                    return self.format_error(
                        "ILI-FORMAT-ATTRIBUTE",
                        "The formatted layout requires compatible component attributes",
                        at,
                    );
                }
                Err(error) => {
                    self.number_error(error, at);
                    return None;
                }
            };
        let result = parse_value(&parts, text, self.limits, &mut self.work);
        match result {
            Ok(values) => Some(values),
            Err(NumberError::Form) => Some(None),
            Err(error) => {
                self.number_error(error, at);
                None
            }
        }
    }

    fn format_error<T>(&mut self, code: &str, message: &str, at: &Span) -> Option<T> {
        self.diagnostics.push(code, message, Some(at.clone()));
        None
    }
}

fn formats_share_members(
    a: &TypeInfo,
    b: &TypeInfo,
    work: &mut WorkBudget,
) -> Result<bool, NumberError> {
    fn component(part: &FormattedPart) -> Option<(&str, Option<&str>)> {
        match part {
            FormattedPart::Literal { .. } => None,
            FormattedPart::Numeric { attribute, .. } => Some((attribute.target_name(), None)),
            FormattedPart::Structure(part) => Some((
                part.attribute.target_name(),
                Some(part.format.target_name()),
            )),
        }
    }
    let (TypeInfo::Formatted(a), TypeInfo::Formatted(b)) = (a, b) else {
        return Ok(false);
    };
    let work_count = u32::try_from(a.parts.len().saturating_add(b.parts.len()))
        .map_err(|_| NumberError::Limit)?;
    work.charge(work_count).map_err(|_| NumberError::Work)?;
    let left = a.parts.iter().filter_map(component);
    let right = b.parts.iter().filter_map(component);
    Ok(left.clone().next().is_some() && left.eq(right))
}

#[derive(Default)]
struct FormatInterval {
    lower: Vec<Option<DecimalInfo>>,
    upper: Vec<Option<DecimalInfo>>,
}

impl FormatInterval {
    fn narrow(
        &mut self,
        first: usize,
        lower: &[DecimalInfo],
        upper: &[DecimalInfo],
        work: &mut WorkBudget,
    ) -> Result<(), NumberError> {
        for (values, bound, unbounded) in [
            (&mut self.lower, lower, Ordering::Less),
            (&mut self.upper, upper, Ordering::Greater),
        ] {
            let prefix = &mut values[first..first + bound.len()];
            let bound: Vec<_> = bound.iter().cloned().map(Some).collect();
            if compare_extrema(prefix, &bound, unbounded, work)? == unbounded {
                prefix.clone_from_slice(&bound);
            }
        }
        Ok(())
    }
}

fn compare_extrema(
    left: &[Option<DecimalInfo>],
    right: &[Option<DecimalInfo>],
    unbounded: Ordering,
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    for (left, right) in left.iter().zip(right) {
        work.charge(1).map_err(|_| NumberError::Work)?;
        let result = match (left, right) {
            (None, None) => return Ok(Ordering::Equal),
            (None, Some(_)) => unbounded,
            (Some(_), None) => unbounded.reverse(),
            (Some(left), Some(right)) => compare_decimal(left, right, work)?,
        };
        if result != Ordering::Equal {
            return Ok(result);
        }
    }
    Ok(Ordering::Equal)
}

enum FormatOperation<'a> {
    Literal(&'a str),
    Numeric(&'a crate::NumericRange, Option<&'a str>),
    Bounds {
        first: usize,
        lower: &'a [DecimalInfo],
        upper: &'a [DecimalInfo],
    },
}

fn format_operations<'a>(
    root: usize,
    types: &'a [TypeInfo],
    symbols: &[Symbol],
    names: &BTreeMap<String, usize>,
    work: &mut WorkBudget,
) -> Result<Vec<FormatOperation<'a>>, NumberError> {
    let mut pending = vec![(root, 0, 0, None)];
    let mut operations = Vec::new();
    let mut components = 0;
    while let Some((ty, position, _, structure)) = pending.last_mut() {
        work.charge(1).map_err(|_| NumberError::Work)?;
        let TypeInfo::Formatted(format) = &types[*ty] else {
            unreachable!()
        };
        match format.parts.get(*position) {
            Some(FormattedPart::Literal { value, .. }) => {
                *position += 1;
                operations.push(FormatOperation::Literal(value));
            }
            Some(FormattedPart::Numeric { attribute, width }) => {
                *position += 1;
                components += 1;
                let ty = format_attribute_type(attribute, *structure, symbols, names, work)?
                    .ok_or(NumberError::Form)?;
                let TypeInfo::Numeric(range) = &types[ty] else {
                    return Err(NumberError::Form);
                };
                operations.push(FormatOperation::Numeric(range, width.as_deref()));
            }
            Some(FormattedPart::Structure(part)) => {
                *position += 1;
                let ty = format_attribute_type(&part.attribute, *structure, symbols, names, work)?
                    .ok_or(NumberError::Form)?;
                let ty = match &types[ty] {
                    TypeInfo::Collection(collection) => collection.element,
                    _ => ty,
                };
                let TypeInfo::Structure(actual) = &types[ty] else {
                    return Err(NumberError::Form);
                };
                let scope = match structure_targets(actual) {
                    [single] => Some(names[single.target_name()]),
                    _ => None,
                };
                pending.push((part.type_id, 0, components, scope));
            }
            None => {
                let (ty, _, first, _) = pending.pop().expect("frame is active");
                // Root bounds are handled by the caller. Nested bounds narrow
                // each occurrence independently.
                if !pending.is_empty() {
                    let mut ancestor = Some(ty);
                    while let Some(id) = ancestor {
                        work.charge(1).map_err(|_| NumberError::Work)?;
                        let TypeInfo::Formatted(domain) = &types[id] else {
                            unreachable!()
                        };
                        if let Some((min, max)) = &domain.bounds {
                            operations.push(FormatOperation::Bounds {
                                first,
                                lower: &min.components,
                                upper: &max.components,
                            });
                        }
                        ancestor = domain.base;
                    }
                }
            }
        }
    }
    Ok(operations)
}

fn format_attribute_type(
    reference: &TypeReference,
    structure: Option<usize>,
    symbols: &[Symbol],
    names: &BTreeMap<String, usize>,
    work: &mut WorkBudget,
) -> Result<Option<usize>, NumberError> {
    let declared = &symbols[names[reference.target_name()]].declaration;
    if let Some(scope) = structure.filter(|scope| Some(*scope) != declared.parent) {
        for &id in symbols[scope]
            .declaration
            .effective_attributes
            .as_deref()
            .unwrap_or_default()
        {
            work.charge(1).map_err(|_| NumberError::Work)?;
            let member = &symbols[id].declaration;
            if member.name == declared.name {
                return Ok(member.type_id);
            }
        }
    }
    Ok(declared.type_id)
}

fn parse_value(
    operations: &[FormatOperation<'_>],
    text: &str,
    limits: &Limits,
    work: &mut WorkBudget,
) -> Result<Option<Vec<DecimalInfo>>, NumberError> {
    struct Visit<'a> {
        part: usize,
        remaining: &'a str,
        prefix_len: usize,
        value: Option<DecimalInfo>,
    }
    let mut pending = vec![Visit {
        part: 0,
        remaining: text,
        prefix_len: 0,
        value: None,
    }];
    let mut values = Vec::new();
    let mut over_limit = false;
    while let Some(visit) = pending.pop() {
        work.charge(1).map_err(|_| NumberError::Work)?;
        values.truncate(visit.prefix_len);
        if let Some(value) = visit.value {
            values.push(value);
        }
        let Some(part) = operations.get(visit.part) else {
            if visit.remaining.is_empty() {
                return Ok(Some(values));
            }
            continue;
        };
        let part_index = visit.part + 1;
        match part {
            FormatOperation::Literal(value) => {
                work.charge(u32::try_from(value.len()).map_err(|_| NumberError::Limit)?)
                    .map_err(|_| NumberError::Work)?;
                if let Some(rest) = visit.remaining.strip_prefix(*value) {
                    pending.push(Visit {
                        part: part_index,
                        remaining: rest,
                        prefix_len: values.len(),
                        value: None,
                    });
                }
            }
            FormatOperation::Bounds {
                first,
                lower,
                upper,
            } => {
                if compare_components(&values[*first..], lower, work)? != Ordering::Less
                    && compare_components(&values[*first..], upper, work)? != Ordering::Greater
                {
                    pending.push(Visit {
                        part: part_index,
                        remaining: visit.remaining,
                        prefix_len: values.len(),
                        value: None,
                    });
                }
            }
            FormatOperation::Numeric(range, width) => {
                let Some((digits, ends)) =
                    component_prefixes(visit.remaining, range.bounds.as_ref())
                else {
                    continue;
                };
                if width
                    .as_ref()
                    .is_some_and(|w| compare_count(&digits.to_string(), w) == Ordering::Less)
                {
                    continue;
                }
                // Push the shorter forms first so that the longest valid component
                // wins when an abstract number can end at more than one separator.
                for end in ends.into_iter().flatten() {
                    let value = match DecimalLiteral::parse_formatted(
                        &visit.remaining[..end],
                        limits,
                        work,
                    ) {
                        Ok(value) => value.info,
                        Err(NumberError::Form) => continue,
                        Err(NumberError::Limit) => {
                            over_limit = true;
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    if let Some(bounds) = &range.bounds
                        && (compare_decimal(&value, &bounds.lower, work)? == Ordering::Less
                            || compare_decimal(&value, &bounds.upper, work)? == Ordering::Greater)
                    {
                        continue;
                    }
                    pending.push(Visit {
                        part: part_index,
                        remaining: &visit.remaining[end..],
                        prefix_len: values.len(),
                        value: Some(value),
                    });
                }
            }
        }
    }
    if over_limit {
        Err(NumberError::Limit)
    } else {
        Ok(None)
    }
}

fn component_prefixes(
    text: &str,
    bounds: Option<&NumericBounds>,
) -> Option<(usize, [Option<usize>; 3])> {
    let signed = usize::from(text.starts_with(['+', '-']));
    let digits = text[signed..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    if digits == 0 {
        return None;
    }
    let mut end = signed + digits;
    if let Some(bounds) = bounds {
        let precision = bounds.lower.fractional_digits() as usize;
        let scaled = bounds.lower.scaled || bounds.upper.scaled;
        if precision > 0 {
            let fraction = text[end..].strip_prefix('.')?.as_bytes().get(..precision)?;
            if !fraction.iter().all(u8::is_ascii_digit) {
                return None;
            }
            if scaled
                && (!text[signed..end].bytes().all(|b| b == b'0')
                    || fraction[0] == b'0' && !fraction.iter().all(|&b| b == b'0'))
            {
                return None;
            }
            end += 1 + precision;
        }
        if scaled {
            end += exponent_length(&text[end..])?;
        }
        return Some((digits, [Some(end), None, None]));
    }
    let mut ends = [Some(end), None, None];
    if let Some(fraction) = text[end..].strip_prefix('.') {
        let length = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if length > 0 {
            end += 1 + length;
            ends[1] = Some(end);
        }
    }
    ends[2] = exponent_length(&text[end..]).map(|length| end + length);
    Some((digits, ends))
}

fn exponent_length(text: &str) -> Option<usize> {
    let exponent = text.strip_prefix(['e', 'E'])?;
    let signed = usize::from(exponent.starts_with(['+', '-']));
    let digits = exponent[signed..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    (digits > 0).then_some(1 + signed + digits)
}

pub(super) fn compare_components(
    left: &[DecimalInfo],
    right: &[DecimalInfo],
    work: &mut WorkBudget,
) -> Result<Ordering, NumberError> {
    for (left, right) in left.iter().zip(right) {
        let result = compare_decimal(left, right, work)?;
        if result != Ordering::Equal {
            return Ok(result);
        }
    }
    Ok(Ordering::Equal)
}
