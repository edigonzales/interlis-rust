use super::{
    Analyzer, Effective, Kind, TypeInfo, collections::compare_count, functions::compare_decimal,
};
use crate::{
    DecimalInfo, FormattedBound, FormattedPart, FormattedType, Limits, NumericBounds, Span,
    TypeReference,
    limits::WorkBudget,
    number::{DecimalLiteral, NumberError},
    syntax::{FormatAttribute, FormatDefinition, FormatPart, Name, QualifiedName, StringRange},
};
use std::{cmp::Ordering, collections::BTreeSet};

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
                    if let FormattedPart::Numeric { attribute, .. } = part {
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
                FormatPart::Attribute { span, .. } => {
                    self.unsupported_expression(span);
                    return None;
                }
            }
        }
        let mut separated = true;
        for part in &parts {
            match part {
                FormattedPart::Literal { value, .. } => separated |= !value.is_empty(),
                FormattedPart::Numeric { attribute, .. } => {
                    if !separated {
                        return self.format_error(
                            "ILI-FORMAT-SEPARATOR",
                            "Numeric format components require a nonempty separator",
                            attribute.source_span(),
                        );
                    }
                    separated = false;
                }
            }
        }
        self.store_format(structure, parts, range, base, at)
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

    pub(super) fn validate_formatted_types(&mut self) -> Option<()> {
        let mut abstract_types = BTreeSet::new();
        for ty in 0..self.types.len() {
            let TypeInfo::Formatted(format) = &self.types[ty] else {
                continue;
            };
            let mut abstract_type = false;
            for part in &format.parts {
                let FormattedPart::Numeric { attribute, .. } = part else {
                    continue;
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

    fn format_value(&mut self, ty: usize, text: &str, at: &Span) -> Option<Vec<DecimalInfo>> {
        if !self.step(text.len(), at) {
            return None;
        }
        let TypeInfo::Formatted(format) = &self.types[ty] else {
            unreachable!()
        };
        let result = parse_value(format, text, self.limits, &mut self.work, |attribute| {
            let ty = self.symbols[self.names[attribute.target_name()]]
                .declaration
                .type_id?;
            match &self.types[ty] {
                TypeInfo::Numeric(number) => Some(number),
                _ => None,
            }
        });
        match result {
            Ok(Some(values)) => Some(values),
            Ok(None) | Err(NumberError::Form) => self.format_error(
                "ILI-FORMAT-BOUNDS",
                "The formatted value must match the layout and the numeric component ranges",
                at,
            ),
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

fn parse_value<'a>(
    format: &FormattedType,
    text: &str,
    limits: &Limits,
    work: &mut WorkBudget,
    component_type: impl Fn(&TypeReference) -> Option<&'a crate::NumericRange>,
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
        let Some(part) = format.parts.get(visit.part) else {
            if visit.remaining.is_empty() {
                return Ok(Some(values));
            }
            continue;
        };
        let part_index = visit.part + 1;
        match part {
            FormattedPart::Literal { value, .. } => {
                work.charge(u32::try_from(value.len()).map_err(|_| NumberError::Limit)?)
                    .map_err(|_| NumberError::Work)?;
                if let Some(rest) = visit.remaining.strip_prefix(value) {
                    pending.push(Visit {
                        part: part_index,
                        remaining: rest,
                        prefix_len: values.len(),
                        value: None,
                    });
                }
            }
            FormattedPart::Numeric { attribute, width } => {
                let Some(range) = component_type(attribute) else {
                    return Ok(None);
                };
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
