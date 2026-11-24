use super::{
    Analyzer, Kind, SyntaxDecl,
    expressions::{numeric_operand, underlying_expression},
    functions::compare_decimal,
    paths::equivalent_selector,
};
use crate::{
    AttributePathOwner, BoundConstraint, BoundExpression, BoundExpressionKind, Cardinality,
    ConstraintRule, DecimalInfo, Enumeration, ExpressionId, GeometryType, LanguageVersion,
    LineForm, NumericRange, ObjectTarget, OidRule, OidType, OverlapTolerance, RelatedLocation,
    TypeInfo, TypeReference, UnitConstant, UnitDefinition, UnitInfo, UnitScale, limits::WorkBudget,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

impl Analyzer<'_, '_> {
    pub(super) fn bind_translations(&mut self) -> Option<()> {
        if self
            .selected_models
            .iter()
            .all(|model| model.translation.is_none())
        {
            return Some(());
        }
        let mut children = vec![Vec::new(); self.symbols.len()];
        for (id, symbol) in self.symbols.iter().enumerate() {
            if let Some(parent) = symbol.declaration.parent {
                children[parent].push(id);
            }
        }
        let mut originals: Vec<_> = (0..self.symbols.len()).collect();
        let mut groups = Vec::new();
        for model in 0..self.selected_models.len() {
            let Some(translation) = &self.selected_models[model].translation else {
                continue;
            };
            let translated = self.names[&self.models[model].name];
            let original = self.names[&translation.original.value];
            let mut pending = vec![(original, translated)];
            let mut pairs = Vec::new();
            while let Some((original, translated)) = pending.pop() {
                let at = self.symbols[translated].name_span.clone();
                if !self.step(1 + children[translated].len(), &at) {
                    return None;
                }
                if self.symbols[original].declaration.kind
                    != self.symbols[translated].declaration.kind
                    || children[original].len() != children[translated].len()
                {
                    self.translation_error(original, translated,
                        "ILI-TRANSLATION-STRUCTURE",
                        "Translated declarations must correspond to the original declarations in order");
                    continue;
                }
                originals[translated] = original;
                pairs.push((original, translated));
                pending.extend(
                    children[original]
                        .iter()
                        .copied()
                        .zip(children[translated].iter().copied())
                        .rev(),
                );
            }
            groups.push((original, translated, pairs));
        }
        if self.diagnostics.rejected {
            return Some(());
        }
        self.bind_translation_namespaces(&originals)?;
        let symbol_models: Vec<_> = self.symbols.iter().map(|symbol| symbol.model).collect();
        for (original_model, translated_model, pairs) in groups {
            let imports = match self.translation_imports(
                symbol_models[original_model],
                symbol_models[translated_model],
                &originals,
            ) {
                Ok(Some(imports)) => imports,
                Ok(None) => {
                    self.translation_error(
                        original_model,
                        translated_model,
                        "ILI-TRANSLATION-IMPORTS",
                        "The imports must correspond uniquely to the original model's imports",
                    );
                    continue;
                }
                Err(()) => {
                    self.diagnostics.limit(
                        "Semantic work budget exceeded",
                        Some(self.symbols[translated_model].name_span.clone()),
                    );
                    return None;
                }
            };
            self.bind_translation_group(&pairs, &originals, &symbol_models, &imports)?;
        }
        Some(())
    }

    fn bind_translation_group(
        &mut self,
        pairs: &[(usize, usize)],
        originals: &[usize],
        symbol_models: &[usize],
        imports: &BTreeMap<usize, usize>,
    ) -> Option<()> {
        for &(original, translated) in pairs {
            let at = self.symbols[translated].name_span.clone();
            let result = if self.symbols[translated].declaration.kind == Kind::Model {
                Ok(Some(self.translation_profile_matches(
                    symbol_models[original],
                    symbol_models[translated],
                )))
            } else {
                self.compare_translation_declarations(
                    original,
                    translated,
                    originals,
                    symbol_models,
                    imports,
                )
            };
            match result {
                Ok(Some(true)) => {
                    let symbol = &self.symbols[translated];
                    let reference = if symbol.declaration.kind == Kind::Model {
                        let name = &self.selected_models[symbol.model]
                            .translation
                            .as_ref()?
                            .original;
                        self.reference_to_symbol(translated, original, name)?
                    } else {
                        let name = super::Name {
                            value: symbol.declaration.name.clone(),
                            span: symbol.name_span.clone(),
                        };
                        self.reference_to_symbol(translated, original, &name)?
                    };
                    self.symbols[translated].declaration.translation_of = Some(reference);
                }
                Ok(Some(false)) => self.translation_error(
                    original,
                    translated,
                    "ILI-TRANSLATION-DEFINITION",
                    "The translated definition differs from the original",
                ),
                Ok(None) => self.unsupported_expression(&at),
                Err(()) => {
                    self.diagnostics
                        .limit("Semantic work budget exceeded", Some(at));
                    return None;
                }
            }
        }
        Some(())
    }

    fn bind_translation_namespaces(&mut self, originals: &[usize]) -> Option<()> {
        for model in 0..self.models.len() {
            if self.selected_models[model].translation.is_none() {
                continue;
            }
            let declaration = self.names[self.models[model].name()];
            let at = self.symbols[declaration].name_span.clone();
            let original = match original_root(declaration, originals, &mut self.work) {
                Ok(original) => self.symbols[original].model,
                Err(()) => {
                    self.diagnostics
                        .limit("Semantic work budget exceeded", Some(at));
                    return None;
                }
            };
            let namespace = self.models[original].effective_xml_namespace();
            if self.models[model]
                .xml_namespace()
                .is_some_and(|declared| declared != namespace)
            {
                self.diagnostics.push_related(
                    "ILI-TRANSLATION-XMLNS",
                    "A translated XMLNS must match the original transfer namespace",
                    self.models[model].xml_namespace_span().cloned(),
                    vec![RelatedLocation {
                        span: self.models[original]
                            .xml_namespace_span()
                            .unwrap_or(&self.models[original].span)
                            .clone(),
                        message: "Original transfer namespace".into(),
                    }],
                );
                continue;
            }
            if !self.step(namespace.len(), &at) {
                return None;
            }
            let namespace = self.models[original].effective_xml_namespace().to_owned();
            self.models[model].effective_xml_namespace = namespace;
        }
        Some(())
    }

    fn translation_profile_matches(&self, original: usize, translated: usize) -> bool {
        let source = &self.models[original];
        let target = &self.models[translated];
        source.kind() == target.kind()
            && (source.language() != LanguageVersion::V23
                || source.is_contracted() == target.is_contracted())
            && source.no_incremental_transfer() == target.no_incremental_transfer()
            && source.charset() == target.charset()
    }

    fn translation_imports(
        &mut self,
        original: usize,
        translated: usize,
        originals: &[usize],
    ) -> Result<Option<BTreeMap<usize, usize>>, ()> {
        let mut correspondence = BTreeMap::new();
        let mut used_originals = BTreeSet::new();
        let mut pending = vec![(original, translated)];
        while let Some((a, b)) = pending.pop() {
            self.work.charge(1)?;
            if let Some(previous) = correspondence.insert(b, a) {
                if previous != a {
                    return Ok(None);
                }
                continue;
            }
            if !used_originals.insert(a)
                || self.bound_imports[a].len() != self.bound_imports[b].len()
            {
                return Ok(None);
            }
            let mut unmatched = [BTreeMap::new(), BTreeMap::new()];
            for (side, (model, other)) in [(a, b), (b, a)].into_iter().enumerate() {
                for (&name, &unqualified) in &self.bound_imports[model] {
                    self.work.charge(1)?;
                    if self.bound_imports[other].get(name) == Some(&unqualified) {
                        if side == 0 && name != "INTERLIS" {
                            let imported = self.model_names[name];
                            pending.push((imported, imported));
                        }
                        continue;
                    }
                    if name == "INTERLIS" {
                        return Ok(None);
                    }
                    let root = original_root(self.names[name], originals, &mut self.work)?;
                    if unmatched[side]
                        .insert((root, unqualified), self.model_names[name])
                        .is_some()
                    {
                        return Ok(None);
                    }
                }
            }
            let [source, target] = unmatched;
            if source.len() != target.len() {
                return Ok(None);
            }
            for ((source_key, source), (target_key, target)) in source.into_iter().zip(target) {
                if source_key != target_key {
                    return Ok(None);
                }
                pending.push((source, target));
            }
        }
        Ok(Some(correspondence))
    }

    fn compare_translation_declarations(
        &mut self,
        original: usize,
        translated: usize,
        originals: &[usize],
        symbol_models: &[usize],
        imports: &BTreeMap<usize, usize>,
    ) -> ComparisonResult {
        let a = &self.symbols[original].declaration;
        let b = &self.symbols[translated].declaration;
        if a.is_abstract() != b.is_abstract()
            || a.is_final() != b.is_final()
            || a.mandatory() != b.mandatory()
            || a.declared_mandatory() != b.declared_mandatory()
            || a.is_transient() != b.is_transient()
            || a.generic != b.generic
            || a.constraints().len() != b.constraints().len()
            || a.subdivision().map(|subdivision| subdivision.continuous)
                != b.subdivision().map(|subdivision| subdivision.continuous)
        {
            return Ok(Some(false));
        }
        let a_syntax = self.symbols[original].ast;
        let b_syntax = self.symbols[translated].ast;
        let a_properties = properties(a_syntax);
        let b_properties = properties(b_syntax);
        let is_view_topic = |syntax: Option<&SyntaxDecl>| matches!(syntax, Some(SyntaxDecl::Topic { options, .. }) if options.view.is_some());
        if is_view_topic(a_syntax) != is_view_topic(b_syntax)
            || a_properties.len() != b_properties.len()
            || !a_properties
                .iter()
                .all(|a| b_properties.iter().any(|b| a.value == b.value))
        {
            return Ok(Some(false));
        }
        let mut comparison = Comparison {
            types: &self.types,
            expressions: &self.expressions,
            names: &self.names,
            originals,
            symbol_models,
            imports,
            temporal_languages: &self.temporal_languages,
            work: &mut self.work,
        };
        if !comparison.reference(a.reference.as_ref(), b.reference.as_ref())?
            || !comparison.reference(a.base_reference(), b.base_reference())?
            || !comparison.reference(a.derived_from(), b.derived_from())?
            || a.is_nullable_view_base() != b.is_nullable_view_base()
            || !comparison.unordered_references(
                a.declared_topic_dependencies(),
                b.declared_topic_dependencies(),
            )?
        {
            return Ok(Some(false));
        }
        if let (Some(a), Some(b)) = (a.oid_rules(), b.oid_rules()) {
            if !comparison.oid_rule(a.basket.as_ref(), b.basket.as_ref())?
                || !comparison.oid_rule(a.object.as_ref(), b.object.as_ref())?
            {
                return Ok(Some(false));
            }
        } else if a.oid_rules().is_some() != b.oid_rules().is_some() {
            return Ok(Some(false));
        }
        if let (Some(a), Some(b)) = (a.association(), b.association())
            && (!same_cardinality(&a.cardinality, &b.cardinality) || a.identified != b.identified)
        {
            return Ok(Some(false));
        }
        if let (Some(a), Some(b)) = (a.role(), b.role())
            && (!same_cardinality(&a.cardinality, &b.cardinality)
                || a.strength != b.strength
                || a.ordered != b.ordered
                || a.external != b.external
                || a.hiding != b.hiding
                || !comparison.object_targets(&a.targets, &b.targets)?)
        {
            return Ok(Some(false));
        }
        if let (Some(a), Some(b)) = (a.unit(), b.unit())
            && !comparison.units(a, b)?
        {
            return Ok(Some(false));
        }
        if !comparison.unordered_references(
            a.deferred_generics.as_deref().unwrap_or_default(),
            b.deferred_generics.as_deref().unwrap_or_default(),
        )? {
            return Ok(Some(false));
        }
        let a_context = a.context.as_deref().unwrap_or_default();
        let b_context = b.context.as_deref().unwrap_or_default();
        if a_context.len() != b_context.len() {
            return Ok(Some(false));
        }
        for (a, b) in a_context.iter().zip(b_context) {
            if !comparison.reference(Some(&a.generic), Some(&b.generic))?
                || !comparison.unordered_references(&a.alternatives, &b.alternatives)?
            {
                return Ok(Some(false));
            }
        }
        if !matches!(
            a.kind,
            Kind::Topic
                | Kind::Domain
                | Kind::RuntimeParameter
                | Kind::Class
                | Kind::Structure
                | Kind::Attribute
                | Kind::Function
                | Kind::Parameter
                | Kind::Association
                | Kind::Role
                | Kind::Unit
                | Kind::Context
                | Kind::View
                | Kind::ViewBase
        ) {
            return Ok(None);
        }
        if a.function()
            .and_then(|function| function.explanation.as_ref())
            .is_some()
            != b.function()
                .and_then(|function| function.explanation.as_ref())
                .is_some()
        {
            return Ok(Some(false));
        }
        let mut expressions = Vec::new();
        if !pair_expressions(&mut expressions, a.derivations(), b.derivations()) {
            return Ok(Some(false));
        }
        if let (Some(left), Some(right)) = (a.view(), b.view()) {
            if left.formation != right.formation
                || left.bases.len() != right.bases.len()
                || !pair_expressions(&mut expressions, &left.selections, &right.selections)
                || !pair_expressions(&mut expressions, &left.grouping, &right.grouping)
            {
                return Ok(Some(false));
            }
            match (&left.inspection, &right.inspection) {
                (Some(a), Some(b)) => {
                    if !comparison.reference(Some(&a.parent), Some(&b.parent))?
                        || !comparison.reference(Some(&a.element), Some(&b.element))?
                    {
                        return Ok(Some(false));
                    }
                    expressions.push((a.path, b.path));
                }
                (None, None) => {}
                _ => return Ok(Some(false)),
            }
            let (a, b) = (
                a.effective_attributes.as_deref().unwrap_or_default(),
                b.effective_attributes.as_deref().unwrap_or_default(),
            );
            if a.len() != b.len() || a.iter().zip(b).any(|(&a, &b)| originals[b] != a) {
                return Ok(Some(false));
            }
        }
        for (a, b) in a.constraints().iter().zip(b.constraints()) {
            if !comparison.pair_constraint(&mut expressions, a, b)? {
                return Ok(Some(false));
            }
        }
        let type_equal = match (a.type_id, b.type_id) {
            (Some(a), Some(b)) => comparison.types(a, b)?,
            (None, None) => Some(true),
            _ => Some(false),
        };
        if type_equal == Some(false) {
            return Ok(Some(false));
        }
        let expressions_equal = comparison.expressions_match(expressions)?;
        Ok(if expressions_equal == Some(false) {
            Some(false)
        } else {
            type_equal.and(expressions_equal)
        })
    }

    fn translation_error(&mut self, original: usize, translated: usize, code: &str, message: &str) {
        self.diagnostics.push_related(
            code,
            message,
            Some(self.symbols[translated].name_span.clone()),
            vec![RelatedLocation {
                span: self.symbols[original].name_span.clone(),
                message: "Corresponding original declaration".into(),
            }],
        );
    }
}

/// An unchecked family keeps the analysis incomplete even if pairing succeeded.
type ComparisonResult = Result<Option<bool>, ()>;

fn properties(syntax: Option<&SyntaxDecl>) -> &[crate::syntax::Located<crate::syntax::Property>] {
    match syntax {
        Some(SyntaxDecl::Topic { options, .. }) => &options.properties,
        Some(
            SyntaxDecl::Container { properties, .. }
            | SyntaxDecl::Attribute { properties, .. }
            | SyntaxDecl::Domain { properties, .. },
        ) => properties,
        Some(SyntaxDecl::Association(association)) => &association.properties,
        Some(SyntaxDecl::View(view)) => &view.properties,
        _ => &[],
    }
}

fn pair_expressions(
    pending: &mut Vec<(ExpressionId, ExpressionId)>,
    a: &[ExpressionId],
    b: &[ExpressionId],
) -> bool {
    if a.len() != b.len() {
        return false;
    }
    pending.extend(a.iter().copied().zip(b.iter().copied()));
    true
}

fn same_cardinality(a: &Cardinality, b: &Cardinality) -> bool {
    a.lower == b.lower && a.upper == b.upper
}

fn pair_optional(
    pending: &mut Vec<(ExpressionId, ExpressionId)>,
    a: Option<ExpressionId>,
    b: Option<ExpressionId>,
) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            pending.push((a, b));
            true
        }
        (None, None) => true,
        _ => false,
    }
}

struct Comparison<'a> {
    types: &'a [TypeInfo],
    expressions: &'a [BoundExpression],
    names: &'a BTreeMap<String, usize>,
    originals: &'a [usize],
    symbol_models: &'a [usize],
    imports: &'a BTreeMap<usize, usize>,
    temporal_languages: &'a BTreeMap<usize, LanguageVersion>,
    work: &'a mut WorkBudget,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum OriginalTarget<'a> {
    Declared(usize, usize),
    Unmatched(usize),
    Predefined(&'a str),
}

fn original_root(mut id: usize, originals: &[usize], work: &mut WorkBudget) -> Result<usize, ()> {
    while id != originals[id] {
        work.charge(1)?;
        id = originals[id];
    }
    Ok(id)
}

impl Comparison<'_> {
    fn pair_constraint(
        &mut self,
        pending: &mut Vec<(ExpressionId, ExpressionId)>,
        a: &BoundConstraint,
        b: &BoundConstraint,
    ) -> Result<bool, ()> {
        self.work.charge(1)?;
        if !pair_optional(pending, a.expression, b.expression) {
            return Ok(false);
        }
        use ConstraintRule::*;
        Ok(match (&a.rule, &b.rule) {
            (Domain, Domain) | (Mandatory, Mandatory) => true,
            (
                Plausibility {
                    direction: a,
                    percentage: av,
                },
                Plausibility {
                    direction: b,
                    percentage: bv,
                },
            ) => a == b && compare_decimal(av, bv, self.work).map_err(|_| ())? == Ordering::Equal,
            (
                Unique {
                    basket: a,
                    local_prefix: ap,
                    keys: ak,
                },
                Unique {
                    basket: b,
                    local_prefix: bp,
                    keys: bk,
                },
            ) => a == b && pair_optional(pending, *ap, *bp) && pair_expressions(pending, ak, bk),
            (
                Existence {
                    source_path: ap,
                    required: ar,
                },
                Existence {
                    source_path: bp,
                    required: br,
                },
            ) => {
                if ar.len() != br.len() {
                    return Ok(false);
                }
                pending.push((*ap, *bp));
                for (a, b) in ar.iter().zip(br) {
                    if !self.reference(Some(&a.viewable), Some(&b.viewable))? {
                        return Ok(false);
                    }
                    pending.push((a.path, b.path));
                }
                true
            }
            (
                Set {
                    basket: a,
                    filter: af,
                },
                Set {
                    basket: b,
                    filter: bf,
                },
            ) => a == b && pair_optional(pending, *af, *bf),
            _ => false,
        })
    }

    fn expressions_match(
        &mut self,
        mut pending: Vec<(ExpressionId, ExpressionId)>,
    ) -> ComparisonResult {
        use BoundExpressionKind::*;
        let mut complete = true;
        while let Some((left, right)) = pending.pop() {
            self.work.charge(1)?;
            let a = underlying_expression(self.expressions, left);
            let b = underlying_expression(self.expressions, right);
            let equal = match (&a.kind, &b.kind) {
                (Undefined, Undefined) | (This, This) => true,
                (Boolean { value: a }, Boolean { value: b }) => a == b,
                (Number { value: av, .. }, Number { value: bv, .. }) => {
                    if !self.constant(av, bv)? {
                        return Ok(Some(false));
                    }
                    let au = numeric_operand(self.expressions, self.types, left).unit;
                    let bu = numeric_operand(self.expressions, self.types, right).unit;
                    if au.is_some() != bu.is_some() {
                        complete = false;
                        continue;
                    }
                    self.reference(au, bu)?
                }
                (Text { value: a, .. }, Text { value: b, .. }) => {
                    self.work.charge((a.len() + b.len()) as u32)?;
                    a == b
                }
                (Temporal { components: a, .. }, Temporal { components: b, .. }) => a == b,
                (Enumeration { path: ap }, Enumeration { path: bp }) => {
                    let x = self.enum_positions(a.type_id, ap)?;
                    let y = self.enum_positions(b.type_id, bp)?;
                    match (x, y) {
                        (Some(x), Some(y)) => x == y,
                        (None, None) => ap == bp,
                        _ => false,
                    }
                }
                (Class { reference: a }, Class { reference: b })
                | (AttributePath { reference: a }, AttributePath { reference: b })
                | (RuntimeParameter { reference: a }, RuntimeParameter { reference: b }) => {
                    self.reference(Some(a), Some(b))?
                }
                (Path { steps: a }, Path { steps: b }) => {
                    if a.len() != b.len() {
                        return Ok(Some(false));
                    }
                    for (a, b) in a.iter().zip(b) {
                        if !equivalent_selector(a.selection.as_ref(), b.selection.as_ref())
                            || (a.reference.declared_name() == "PARENT")
                                != (b.reference.declared_name() == "PARENT")
                            || !self.reference(Some(&a.reference), Some(&b.reference))?
                        {
                            return Ok(Some(false));
                        }
                    }
                    true
                }
                (ObjectSet { explicit: ae }, ObjectSet { explicit: be }) => {
                    if ae != be {
                        return Ok(Some(false));
                    }
                    match a.type_id.zip(b.type_id) {
                        Some((a, b)) => match self.types(a, b)? {
                            Some(equal) => equal,
                            None => {
                                complete = false;
                                continue;
                            }
                        },
                        None => return Ok(None),
                    }
                }
                (
                    Call {
                        function: a,
                        arguments: av,
                    },
                    Call {
                        function: b,
                        arguments: bv,
                    },
                ) => self.reference(Some(a), Some(b))? && pair_expressions(&mut pending, av, bv),
                (
                    Binary {
                        operator: a,
                        left: al,
                        right: ar,
                    },
                    Binary {
                        operator: b,
                        left: bl,
                        right: br,
                    },
                ) => {
                    pending.extend([(*al, *bl), (*ar, *br)]);
                    a == b
                }
                (
                    Unary {
                        operator: a,
                        operand: av,
                    },
                    Unary {
                        operator: b,
                        operand: bv,
                    },
                ) => {
                    pending.push((*av, *bv));
                    a == b
                }
                (Defined { expression: a }, Defined { expression: b }) => {
                    pending.push((*a, *b));
                    true
                }
                _ => false,
            };
            if !equal {
                return Ok(Some(false));
            }
        }
        Ok(complete.then_some(true))
    }

    fn constant(&mut self, a: &UnitConstant, b: &UnitConstant) -> Result<bool, ()> {
        use UnitConstant::*;
        match (a, b) {
            (Decimal(a), Decimal(b)) => {
                Ok(compare_decimal(a, b, self.work).map_err(|_| ())? == Ordering::Equal)
            }
            (Pi, Pi) | (LnBase, LnBase) => Ok(true),
            _ => Ok(false),
        }
    }

    fn object_targets(&mut self, a: &[ObjectTarget], b: &[ObjectTarget]) -> Result<bool, ()> {
        if a.len() != b.len() {
            return Ok(false);
        }
        for (a, b) in a.iter().zip(b) {
            if !self.reference(a.target.as_ref(), b.target.as_ref())?
                || !self.references(&a.restrictions, &b.restrictions)?
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn units(&mut self, a: &UnitInfo, b: &UnitInfo) -> Result<bool, ()> {
        if a.abstract_unit != b.abstract_unit
            || !self.reference(a.base.as_ref(), b.base.as_ref())?
        {
            return Ok(false);
        }
        use UnitDefinition::*;
        Ok(match (&a.definition, &b.definition) {
            (Fundamental, Fundamental) => true,
            (Composed { terms: a }, Composed { terms: b }) => {
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (a, b) in a.iter().zip(b) {
                    if a.operator != b.operator || !self.reference(Some(&a.unit), Some(&b.unit))? {
                        return Ok(false);
                    }
                }
                true
            }
            (
                Derived {
                    target: a,
                    scale: av,
                },
                Derived {
                    target: b,
                    scale: bv,
                },
            ) => {
                if !self.reference(Some(a), Some(b))? {
                    return Ok(false);
                }
                use UnitScale::*;
                match (av, bv) {
                    (Identity, Identity) | (Function(_), Function(_)) => true,
                    (Factors(a), Factors(b)) => {
                        if a.len() != b.len() {
                            return Ok(false);
                        }
                        for (a, b) in a.iter().zip(b) {
                            self.work.charge(1)?;
                            if a.operator != b.operator || !self.constant(&a.value, &b.value)? {
                                return Ok(false);
                            }
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        })
    }

    fn geometry(&mut self, a: &GeometryType, b: &GeometryType) -> Result<bool, ()> {
        if a.geometry_kind != b.geometry_kind
            || a.directed != b.directed
            || a.without_overlaps != b.without_overlaps
            || !self.reference(a.vertex.as_ref(), b.vertex.as_ref())?
            || !self.reference(a.line_attributes.as_ref(), b.line_attributes.as_ref())?
            || !self.unordered_names(
                a.line_forms.iter().map(line_form_name),
                b.line_forms.iter().map(line_form_name),
            )?
        {
            return Ok(false);
        }
        use OverlapTolerance::*;
        Ok(match (&a.tolerance, &b.tolerance) {
            (None, None) => true,
            (Some(TrackWidth(a)), Some(TrackWidth(b))) => self.reference(a.as_ref(), b.as_ref())?,
            (Some(Explicit(a)), Some(Explicit(b))) => {
                compare_decimal(a, b, self.work).map_err(|_| ())? == Ordering::Equal
            }
            _ => false,
        })
    }

    fn enum_positions(
        &mut self,
        ty: Option<usize>,
        path: &[String],
    ) -> Result<Option<Vec<usize>>, ()> {
        let Some(ty) = ty else {
            return Ok(None);
        };
        let ty = match self.types[ty] {
            TypeInfo::EnumTree { base, .. } => base,
            _ => ty,
        };
        let TypeInfo::Enumeration { values, .. } = &self.types[ty] else {
            return Ok(None);
        };
        let mut elements = values.elements();
        let mut positions = Vec::new();
        for part in path {
            self.work.charge(elements.len() as u32)?;
            let Some(position) = elements.iter().position(|element| element.name() == part) else {
                return Ok(None);
            };
            positions.push(position);
            elements = elements[position]
                .children()
                .map(Enumeration::elements)
                .unwrap_or_default();
        }
        Ok(Some(positions))
    }

    fn name(&mut self, a: &str, b: &str) -> Result<bool, ()> {
        Ok(self.target(a, false)? == self.target(b, true)?)
    }

    fn target<'a>(&mut self, name: &'a str, translated: bool) -> Result<OriginalTarget<'a>, ()> {
        self.work.charge(name.len() as u32)?;
        let Some(&id) = self.names.get(name) else {
            return Ok(OriginalTarget::Predefined(name));
        };
        let model = self.symbol_models[id];
        let original_model = if translated {
            let Some(&model) = self.imports.get(&model) else {
                return Ok(OriginalTarget::Unmatched(id));
            };
            model
        } else {
            model
        };
        let declaration = original_root(id, self.originals, self.work)?;
        Ok(OriginalTarget::Declared(original_model, declaration))
    }

    fn unordered_references(
        &mut self,
        a: &[TypeReference],
        b: &[TypeReference],
    ) -> Result<bool, ()> {
        self.unordered_names(
            a.iter().map(TypeReference::target_name),
            b.iter().map(TypeReference::target_name),
        )
    }

    fn unordered_names<'a, 'b>(
        &mut self,
        a: impl Iterator<Item = &'a str>,
        b: impl Iterator<Item = &'b str>,
    ) -> Result<bool, ()> {
        let mut a = a
            .map(|name| self.target(name, false))
            .collect::<Result<Vec<_>, _>>()?;
        let mut b = b
            .map(|name| self.target(name, true))
            .collect::<Result<Vec<_>, _>>()?;
        a.sort_unstable();
        b.sort_unstable();
        Ok(a == b)
    }

    fn reference(
        &mut self,
        a: Option<&TypeReference>,
        b: Option<&TypeReference>,
    ) -> Result<bool, ()> {
        match (a, b) {
            (Some(a), Some(b)) => self.name(a.target_name(), b.target_name()),
            (None, None) => Ok(true),
            _ => Ok(false),
        }
    }

    fn references(&mut self, a: &[TypeReference], b: &[TypeReference]) -> Result<bool, ()> {
        if a.len() != b.len() {
            return Ok(false);
        }
        for (a, b) in a.iter().zip(b) {
            if !self.reference(Some(a), Some(b))? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn oid_rule(&mut self, a: Option<&OidRule>, b: Option<&OidRule>) -> Result<bool, ()> {
        match (a, b) {
            (Some(a), Some(b)) => {
                Ok(a.kind == b.kind && self.reference(a.domain.as_ref(), b.domain.as_ref())?)
            }
            (None, None) => Ok(true),
            _ => Ok(false),
        }
    }

    fn decimal(&mut self, a: &DecimalInfo, b: &DecimalInfo) -> Result<bool, ()> {
        self.work
            .charge((a.coefficient().len() + b.coefficient().len()) as u32)?;
        Ok(a.coefficient() == b.coefficient() && a.scale() == b.scale())
    }

    fn numeric(&mut self, a: &NumericRange, b: &NumericRange) -> Result<bool, ()> {
        if a.circular != b.circular
            || a.direction != b.direction
            || !self.reference(a.unit.as_ref(), b.unit.as_ref())?
        {
            return Ok(false);
        }
        if let (Some(a), Some(b)) = (&a.reference_system, &b.reference_system) {
            if a.axis != b.axis || !self.reference(Some(&a.domain), Some(&b.domain))? {
                return Ok(false);
            }
        } else if a.reference_system.is_some() != b.reference_system.is_some() {
            return Ok(false);
        }
        match (&a.bounds, &b.bounds) {
            (Some(a), Some(b)) => {
                Ok(self.decimal(&a.lower, &b.lower)? && self.decimal(&a.upper, &b.upper)?)
            }
            (None, None) => Ok(true),
            _ => Ok(false),
        }
    }

    fn enumeration(&mut self, a: &Enumeration, b: &Enumeration) -> Result<bool, ()> {
        if a.is_final() != b.is_final() || a.elements().len() != b.elements().len() {
            return Ok(false);
        }
        for (a, b) in a.elements().iter().zip(b.elements()) {
            self.work.charge(1)?;
            match (a.children(), b.children()) {
                (Some(a), Some(b)) if self.enumeration(a, b)? => {}
                (None, None) => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    fn types(&mut self, a: usize, b: usize) -> ComparisonResult {
        if self.temporal_languages.get(&a) != self.temporal_languages.get(&b) {
            return Ok(Some(false));
        }
        self.descriptors(&self.types[a], &self.types[b])
    }

    fn descriptors(&mut self, a: &TypeInfo, b: &TypeInfo) -> ComparisonResult {
        use TypeInfo::*;
        self.work.charge(1)?;
        Ok(Some(match (a, b) {
            (Boolean, Boolean) | (UnboundedText, UnboundedText) => true,
            (Text { max_length: a }, Text { max_length: b }) => a == b,
            (MultilineText { max_length: a }, MultilineText { max_length: b }) => a == b,
            (
                FixedText {
                    format: a,
                    max_length: al,
                },
                FixedText {
                    format: b,
                    max_length: bl,
                },
            ) => a == b && al == bl,
            (Alignment(a), Alignment(b)) => a == b,
            (Blackbox(a), Blackbox(b)) => a == b,
            (Temporal(a), Temporal(b)) => a == b,
            (TemporalRange(a), TemporalRange(b)) => {
                a.kind == b.kind
                    && a.language == b.language
                    && a.lower.components == b.lower.components
                    && a.upper.components == b.upper.components
                    && self.reference(Some(&a.base), Some(&b.base))?
            }
            (Formatted(a), Formatted(b)) => {
                if a.parts.len() != b.parts.len()
                    || !self.reference(Some(&a.structure), Some(&b.structure))?
                {
                    return Ok(Some(false));
                }
                for (a, b) in a.parts.iter().zip(&b.parts) {
                    self.work.charge(1)?;
                    use crate::FormattedPart::{Literal, Numeric};
                    let matches = match (a, b) {
                        (Literal { value: a, .. }, Literal { value: b, .. }) => {
                            self.work.charge((a.len() + b.len()) as u32)?;
                            a == b
                        }
                        (
                            Numeric {
                                attribute: a,
                                width: aw,
                            },
                            Numeric {
                                attribute: b,
                                width: bw,
                            },
                        ) => aw == bw && self.reference(Some(a), Some(b))?,
                        _ => false,
                    };
                    if !matches {
                        return Ok(Some(false));
                    }
                }
                match (&a.bounds, &b.bounds) {
                    (Some((al, au)), Some((bl, bu))) => {
                        self.work.charge(
                            (al.original.len()
                                + au.original.len()
                                + bl.original.len()
                                + bu.original.len()) as u32,
                        )?;
                        al.original == bl.original && au.original == bu.original
                    }
                    (None, None) => true,
                    _ => false,
                }
            }
            (Numeric(a), Numeric(b)) => self.numeric(a, b)?,
            (Coordinate(a), Coordinate(b)) => {
                if a.kind != b.kind
                    || a.axes.len() != b.axes.len()
                    || a.rotation != b.rotation
                    || a.reference_system != b.reference_system
                {
                    return Ok(Some(false));
                }
                for (a, b) in a.axes.iter().zip(&b.axes) {
                    if !self.numeric(a, b)? {
                        return Ok(Some(false));
                    }
                }
                true
            }
            (Oid(OidType::Typed(a)), Oid(OidType::Typed(b))) => return self.descriptors(a, b),
            (Oid(a), Oid(b)) => std::mem::discriminant(a) == std::mem::discriminant(b),
            (
                Enumeration {
                    values: a,
                    order: ao,
                    declared_order: ad,
                },
                Enumeration {
                    values: b,
                    order: bo,
                    declared_order: bd,
                },
            ) => ao == bo && ad == bd && self.enumeration(a, b)?,
            (EnumTree { domain: a, .. }, EnumTree { domain: b, .. }) => match (a, b) {
                (Some(a), Some(b)) => self.name(a, b)?,
                _ => return Ok(None),
            },
            (ClassValue(a), ClassValue(b)) => {
                a.kind == b.kind && self.references(&a.restrictions, &b.restrictions)?
            }
            (EnumArgument { tree_values: a }, EnumArgument { tree_values: b }) => a == b,
            (AttributePath(a), AttributePath(b)) => {
                if a.restrictions.len() != b.restrictions.len() {
                    return Ok(Some(false));
                }
                let owners = match (&a.owner, &b.owner) {
                    (None, None) => Some(true),
                    (
                        Some(AttributePathOwner::Viewable(a)),
                        Some(AttributePathOwner::Viewable(b)),
                    ) => Some(self.reference(Some(a), Some(b))?),
                    (
                        Some(AttributePathOwner::ClassAttribute(a)),
                        Some(AttributePathOwner::ClassAttribute(b)),
                    ) => self.expressions_match(vec![(*a, *b)])?,
                    _ => Some(false),
                };
                let mut complete = owners == Some(true);
                if owners == Some(false) {
                    return Ok(Some(false));
                }
                for (a, b) in a.restrictions.iter().zip(&b.restrictions) {
                    if a.mandatory != b.mandatory {
                        return Ok(Some(false));
                    }
                    match self.types(a.ty, b.ty)? {
                        Some(true) => {}
                        Some(false) => return Ok(Some(false)),
                        None => complete = false,
                    }
                }
                return Ok(complete.then_some(true));
            }
            (Structure(a), Structure(b)) => {
                self.reference(a.target.as_ref(), b.target.as_ref())?
                    && self.references(&a.restrictions, &b.restrictions)?
            }
            (Reference(a), Reference(b)) => {
                a.external == b.external
                    && self.reference(a.target.target.as_ref(), b.target.target.as_ref())?
                    && self.references(&a.target.restrictions, &b.target.restrictions)?
            }
            (ObjectSelection(a), ObjectSelection(b)) => {
                a.plural == b.plural && self.object_targets(&a.targets, &b.targets)?
            }
            (Collection(a), Collection(b)) => {
                if a.kind != b.kind || !same_cardinality(&a.cardinality, &b.cardinality) {
                    return Ok(Some(false));
                }
                return self.types(a.element, b.element);
            }
            (Geometry(a), Geometry(b)) => self.geometry(a, b)?,
            _ => false,
        }))
    }
}

fn line_form_name(form: &LineForm) -> &str {
    match form {
        LineForm::Straights => "INTERLIS.STRAIGHTS",
        LineForm::Arcs => "INTERLIS.ARCS",
        LineForm::Named(reference) => reference.target_name(),
    }
}
