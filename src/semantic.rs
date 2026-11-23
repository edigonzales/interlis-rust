mod domains;
mod enumeration;
mod names;
mod oid;
mod references;
mod topics;
mod units;

use self::names::{BoundName, LookupError};
use crate::{
    Limits, RelatedLocation, Span,
    diagnostic::Diagnostics,
    limits::WorkBudget,
    model::{
        self, DeclarationKind as Kind, FixedTextFormat, ModelGraph, ModelInfo, OidType, TypeInfo,
        TypeReference,
    },
    number::{DecimalLiteral, NumberError},
    syntax::{self, Declaration as SyntaxDecl, Name, QualifiedName, TypeSyntax},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Effective {
    ty: usize,
    mandatory: bool,
}
#[derive(Clone, Copy)]
enum State {
    Unseen,
    Resolving,
    Resolved(Effective),
    Failed,
}
struct Symbol<'a> {
    declaration: model::Declaration,
    name_span: Span,
    model: usize,
    syntax: Option<&'a TypeSyntax>,
    base: Option<&'a QualifiedName>,
    base_scope: Option<usize>,
    ast: Option<&'a SyntaxDecl>,
}
impl<'a> Symbol<'a> {
    fn domain_dependency(&self) -> Option<&'a QualifiedName> {
        self.base.or(match self.syntax {
            Some(TypeSyntax::EnumTree { domain, .. }) => Some(domain),
            _ => None,
        })
    }

    fn inherits_names(&self) -> bool {
        let extended = |properties: &[syntax::Located<syntax::Property>]| {
            properties
                .iter()
                .any(|p| p.value == syntax::Property::Extended)
        };
        match self.ast {
            Some(SyntaxDecl::Container {
                base, properties, ..
            }) => base.is_some() || extended(properties),
            Some(SyntaxDecl::Association(a)) => {
                a.base.is_some() || a.derived_from.is_some() || extended(&a.properties)
            }
            Some(SyntaxDecl::MetadataBasket(b)) => b.base.is_some(),
            Some(SyntaxDecl::Graphic(g)) => g.base.is_some() || extended(&g.properties),
            Some(SyntaxDecl::View(_)) => true,
            _ => false,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Namespace {
    Type,
    Member,
    MetaObject,
    Context,
}
fn namespace(kind: Kind) -> Namespace {
    match kind {
        Kind::Attribute
        | Kind::RuntimeParameter
        | Kind::Role
        | Kind::Parameter
        | Kind::DrawingRule => Namespace::Member,
        Kind::MetaObject => Namespace::MetaObject,
        Kind::Context => Namespace::Context,
        _ => Namespace::Type,
    }
}
enum Target {
    Domain(usize),
    Structure,
    Predefined(&'static str),
}
struct Analyzer<'a, 'd> {
    documents: &'a [syntax::Document],
    limits: &'a Limits,
    diagnostics: &'d mut Diagnostics,
    work: WorkBudget,
    symbols: Vec<Symbol<'a>>,
    selected_models: Vec<&'a syntax::ModelDecl>,
    models: Vec<ModelInfo>,
    bound_imports: Vec<BTreeMap<&'a str, bool>>,
    model_names: BTreeMap<String, usize>,
    names: BTreeMap<String, usize>,
    scope_names: BTreeMap<(usize, Namespace, String), usize>,
    states: Vec<State>,
    types: Vec<TypeInfo>,
}
fn supported_type(ty: &TypeSyntax) -> bool {
    match ty {
        TypeSyntax::Text(_)
        | TypeSyntax::Boolean { .. }
        | TypeSyntax::Named(_)
        | TypeSyntax::Alignment { .. }
        | TypeSyntax::Temporal { .. }
        | TypeSyntax::Blackbox { .. }
        | TypeSyntax::Enumeration { .. }
        | TypeSyntax::EnumTree { .. } => true,
        TypeSyntax::Numeric(n) => n.reference_system.is_none(),
        TypeSyntax::Coordinate(c) => c.axes.iter().all(|axis| axis.reference_system.is_none()),
        TypeSyntax::Oid { carrier, .. } => match carrier {
            syntax::OidSyntax::Numeric(range) => range.reference_system.is_none(),
            _ => true,
        },
        TypeSyntax::Formatted { .. }
        | TypeSyntax::Geometry(_)
        | TypeSyntax::ClassValue { .. }
        | TypeSyntax::AttributePathValue { .. }
        | TypeSyntax::Reference { .. }
        | TypeSyntax::Collection { .. }
        | TypeSyntax::RestrictedStructure { .. } => false,
    }
}

fn supported_specialization(ty: &TypeSyntax) -> bool {
    matches!(
        ty,
        TypeSyntax::Text(_)
            | TypeSyntax::Numeric(_)
            | TypeSyntax::Coordinate(_)
            | TypeSyntax::Oid { .. }
            | TypeSyntax::Enumeration { .. }
    ) && supported_type(ty)
}

// Every newly parsed family must pass this gate before the I-0 analyzer runs.
// Inert section markers do not change the supported attribute semantics.
fn ensure_supported_semantics(
    selected_models: &[&syntax::ModelDecl],
    diagnostics: &mut Diagnostics,
    work: &mut WorkBudget,
) -> bool {
    fn charge(work: &mut WorkBudget, diagnostics: &mut Diagnostics, span: &Span) -> bool {
        if work.charge(1).is_err() {
            diagnostics.limit("Semantic work budget exceeded", Some(span.clone()));
            false
        } else {
            true
        }
    }
    fn declarations<'a>(
        items: &'a [SyntaxDecl],
        work: &mut WorkBudget,
        diagnostics: &mut Diagnostics,
    ) -> Option<&'a Span> {
        for item in items {
            if !charge(work, diagnostics, item.span()) {
                return Some(item.span());
            }
            let unsupported = match item {
                SyntaxDecl::Topic {
                    options,
                    declarations: children,
                    span,
                    ..
                } => {
                    if options.view.is_some() || !options.deferred_generics.is_empty() {
                        Some(span)
                    } else {
                        declarations(children, work, diagnostics)
                    }
                }
                SyntaxDecl::Container {
                    properties,
                    base,
                    parameters,
                    constraints,
                    attributes,
                    parameter_section,
                    span,
                    ..
                } => {
                    if properties.iter().any(|p| {
                        !matches!(
                            p.value,
                            syntax::Property::Abstract | syntax::Property::Final
                        )
                    }) || base.is_some()
                        || parameter_section.is_some()
                        || !parameters.is_empty()
                        || !constraints.is_empty()
                    {
                        Some(span)
                    } else {
                        declarations(attributes, work, diagnostics)
                    }
                }
                SyntaxDecl::Attribute {
                    properties,
                    subdivision,
                    type_use,
                    derivations,
                    span,
                    ..
                } => {
                    if !properties.is_empty()
                        || subdivision.is_some()
                        || type_use.ty.is_none()
                        || !derivations.is_empty()
                    {
                        Some(span)
                    } else {
                        type_use
                            .ty
                            .as_ref()
                            .filter(|ty| !supported_type(ty))
                            .map(TypeSyntax::span)
                    }
                }
                SyntaxDecl::Domain {
                    properties,
                    base,
                    ty,
                    mandatory,
                    constraints,
                    span,
                    ..
                } => {
                    let supported = match ty {
                        Some(t) => {
                            if base.is_none() {
                                supported_type(t)
                            } else {
                                supported_specialization(t)
                            }
                        }
                        None => *mandatory && base.is_some(),
                    };
                    if properties.iter().any(|p| {
                        !matches!(
                            p.value,
                            syntax::Property::Final | syntax::Property::Abstract
                        )
                    }) || properties
                        .iter()
                        .any(|p| p.value == syntax::Property::Final)
                        && properties
                            .iter()
                            .any(|p| p.value == syntax::Property::Abstract)
                        || !constraints.is_empty()
                        || ty.is_none() && !supported
                    {
                        Some(span)
                    } else if !supported {
                        Some(ty.as_ref().expect("type present").span())
                    } else {
                        None
                    }
                }
                SyntaxDecl::Unit(_) => None,
                SyntaxDecl::LineForm(form) => Some(&form.span),
                SyntaxDecl::Function(function) => Some(&function.span),
                SyntaxDecl::Constraints(constraints) => Some(&constraints.span),
                SyntaxDecl::Association(association) => Some(&association.span),
                SyntaxDecl::Context(context) => Some(&context.span),
                SyntaxDecl::View(view) => Some(&view.span),
                SyntaxDecl::Graphic(graphic) => Some(&graphic.span),
                SyntaxDecl::MetadataBasket(basket) => Some(&basket.span),
                SyntaxDecl::RuntimeParameter { type_use, span, .. } => match &type_use.ty {
                    Some(ty) if supported_type(ty) => None,
                    Some(ty) => Some(ty.span()),
                    None => Some(span),
                },
            };
            if unsupported.is_some() {
                return unsupported;
            }
        }
        None
    }
    for model in selected_models {
        if !charge(work, diagnostics, &model.span) {
            return false;
        }
        let kind_supported = model.kind == syntax::ModelKind::Data
            || model.kind == syntax::ModelKind::Type
                && model
                    .declarations
                    .iter()
                    .all(|d| matches!(d, SyntaxDecl::Domain { .. } | SyntaxDecl::Unit(_)));
        let header = !kind_supported
            || model.no_incremental_transfer.is_some()
            || model.explanation.is_some()
            || model.translation.is_some()
            || model.charset.is_some()
            || model.xmlns.is_some();
        let span = if header {
            model
                .no_incremental_transfer
                .as_ref()
                .or_else(|| model.explanation.as_ref().map(|e| &e.span))
                .or(Some(&model.span))
        } else {
            declarations(&model.declarations, work, diagnostics)
        };
        if diagnostics.limited {
            return false;
        }
        if let Some(span) = span {
            diagnostics.push(
                "ILI-UNSUPPORTED",
                "Parsed syntax has semantics outside the implemented subset",
                Some(span.clone()),
            );
            return false;
        }
    }
    true
}

impl<'a, 'd> Analyzer<'a, 'd> {
    fn new(
        documents: &'a [syntax::Document],
        limits: &'a Limits,
        diagnostics: &'d mut Diagnostics,
    ) -> Self {
        Self {
            documents,
            limits,
            diagnostics,
            work: WorkBudget::new(limits),
            symbols: vec![],
            selected_models: vec![],
            models: vec![],
            bound_imports: vec![],
            model_names: BTreeMap::new(),
            names: BTreeMap::new(),
            scope_names: BTreeMap::new(),
            states: vec![],
            types: vec![],
        }
    }

    fn step(&mut self, amount: usize, span: &Span) -> bool {
        if u32::try_from(amount)
            .ok()
            .is_some_and(|n| self.work.charge(n).is_ok())
        {
            true
        } else {
            self.diagnostics.push(
                "ILI-LIMIT",
                "Semantic work budget exceeded",
                Some(span.clone()),
            );
            false
        }
    }
    fn end_name(&mut self, name: &Name, end: &Name) {
        if name.value != end.value {
            self.diagnostics.push_related(
                "ILI-END-NAME",
                "Closing name does not match declaration",
                Some(end.span.clone()),
                vec![RelatedLocation {
                    span: name.span.clone(),
                    message: "Declaration starts here".into(),
                }],
            );
        }
    }
    fn duplicate(&mut self, span: &Span, previous: &Span) {
        self.diagnostics.push_related(
            "ILI-DUPLICATE",
            "Duplicate name in supported namespace",
            Some(span.clone()),
            vec![RelatedLocation {
                span: previous.clone(),
                message: "Earlier declaration".into(),
            }],
        );
    }
    fn add(
        &mut self,
        kind: Kind,
        name: &Name,
        span: &Span,
        parent: Option<usize>,
        model: usize,
        mandatory: Option<bool>,
    ) -> Option<usize> {
        // Charge generated qualified-name bytes before duplicating ancestor names.
        // This bounds derived allocations even for very long model/scope names.
        let parent_length = parent.map_or(0, |p| self.symbols[p].declaration.qualified.len() + 1);
        let length = parent_length.checked_add(name.value.len())?;
        if !self.step(
            length.saturating_add(name.value.len()).saturating_add(1),
            &name.span,
        ) {
            return None;
        }
        let qualified = parent.map_or_else(
            || name.value.clone(),
            |p| format!("{}.{}", self.symbols[p].declaration.qualified, name.value),
        );
        let id = self.symbols.len();
        let mut duplicate = false;
        if let Some(parent) = parent {
            let key = (parent, namespace(kind), name.value.clone());
            if let Some(&previous) = self.scope_names.get(&key) {
                let previous_span = self.symbols[previous].name_span.clone();
                self.duplicate(&name.span, &previous_span);
                duplicate = true;
            } else {
                self.scope_names.insert(key, id);
            }
        }
        self.names
            .entry(qualified.clone())
            .and_modify(|previous| {
                if namespace(self.symbols[*previous].declaration.kind) != Namespace::Type
                    && namespace(kind) == Namespace::Type
                {
                    *previous = id;
                }
            })
            .or_insert(id);
        self.symbols.push(Symbol {
            declaration: model::Declaration {
                name: name.value.clone(),
                qualified,
                kind,
                span: span.clone(),
                parent,
                type_id: None,
                declared_mandatory: mandatory,
                final_declaration: false,
                abstract_declaration: false,
                effective_mandatory: None,
                reference: None,
                base_reference: None,
                unit: None,
                oids: None,
                topic_dependencies: None,
            },
            name_span: name.span.clone(),
            model,
            syntax: None,
            base: None,
            base_scope: None,
            ast: None,
        });
        self.states.push(if duplicate {
            State::Failed
        } else {
            State::Unseen
        });
        Some(id)
    }
    fn collect_models(&mut self, requested: Option<&[String]>) {
        let mut candidates = Vec::new();
        let mut by_name = BTreeMap::<&str, Vec<usize>>::new();
        for document in self.documents {
            for ast in &document.models {
                if !self.step(ast.name.value.len().saturating_add(1), &ast.name.span) {
                    return;
                }
                by_name
                    .entry(&ast.name.value)
                    .or_default()
                    .push(candidates.len());
                candidates.push((document.language, ast));
            }
        }
        let mut selected = vec![requested.is_none(); candidates.len()];
        let mut pending = Vec::new();
        if let Some(requested) = requested {
            for name in requested {
                if let Some(indices) = by_name.get(name.as_str()) {
                    if let &[first, second, ..] = indices.as_slice() {
                        self.diagnostics.push_related(
                            "ILI-ROOT-AMBIGUOUS",
                            "More than one supplied model has the requested name",
                            None,
                            [first, second]
                                .into_iter()
                                .map(|candidate| RelatedLocation {
                                    span: candidates[candidate].1.name.span.clone(),
                                    message: "Candidate model".into(),
                                })
                                .collect(),
                        );
                        if self.diagnostics.limited {
                            return;
                        }
                        continue;
                    }
                    for &index in indices {
                        selected[index] = true;
                        pending.push(index);
                    }
                } else {
                    self.diagnostics.push(
                        "ILI-ROOT-NOT-FOUND",
                        &format!("Requested model '{name}' is absent"),
                        None,
                    );
                    if self.diagnostics.limited {
                        return;
                    }
                }
            }
        } else {
            pending.extend((0..candidates.len()).rev());
        }
        while let Some(index) = pending.pop() {
            let ast = candidates[index].1;
            if requested.is_some() {
                for import in &ast.imports {
                    let targets = by_name
                        .get(import.name.value.as_str())
                        .map_or(&[][..], Vec::as_slice);
                    if !self.step(
                        import
                            .name
                            .value
                            .len()
                            .saturating_add(targets.len())
                            .saturating_add(1),
                        &import.span,
                    ) {
                        return;
                    }
                    if let &[first, second, ..] = targets {
                        self.diagnostics.push_related(
                            "ILI-IMPORT-AMBIGUOUS",
                            "More than one supplied model has the imported name",
                            Some(import.name.span.clone()),
                            [first, second]
                                .into_iter()
                                .map(|candidate| RelatedLocation {
                                    span: candidates[candidate].1.name.span.clone(),
                                    message: "Candidate imported model".into(),
                                })
                                .collect(),
                        );
                        if self.diagnostics.limited {
                            return;
                        }
                        continue;
                    }
                    for &target in targets {
                        if !std::mem::replace(&mut selected[target], true) {
                            pending.push(target);
                        }
                    }
                }
            }
            let Some(translation) = &ast.translation else {
                continue;
            };
            let mut matching = None;
            let mut ambiguous = false;
            if let Some(indices) = by_name.get(translation.original.value.as_str()) {
                for &target in indices {
                    let original = candidates[target].1;
                    if !self.step(
                        original.version.value.len().saturating_add(1),
                        &translation.span,
                    ) {
                        return;
                    }
                    if original.version.value != translation.version.value {
                        continue;
                    }
                    if let Some(previous) = matching {
                        let conflicting: [usize; 2] = [previous, target];
                        self.diagnostics.push_related(
                            "ILI-TRANSLATION-ORIGINAL-AMBIGUOUS",
                            "Multiple sources declare this translation original and version",
                            Some(translation.original.span.clone()),
                            conflicting
                                .into_iter()
                                .map(|candidate| RelatedLocation {
                                    span: candidates[candidate].1.name.span.clone(),
                                    message: "Candidate original model".into(),
                                })
                                .collect(),
                        );
                        ambiguous = true;
                        break;
                    }
                    matching = Some(target);
                }
            }
            if !ambiguous {
                if let Some(target) = matching {
                    if !std::mem::replace(&mut selected[target], true) {
                        pending.push(target);
                    }
                } else {
                    self.diagnostics.push_related(
                        "ILI-TRANSLATION-ORIGINAL-NOT-FOUND",
                        "No supplied original model has this name and version",
                        Some(translation.original.span.clone()),
                        vec![RelatedLocation {
                            span: translation.version.span.clone(),
                            message: "Requested original version".into(),
                        }],
                    );
                }
            }
            if self.diagnostics.limited {
                return;
            }
        }
        if self.diagnostics.rejected {
            return;
        }
        for ((language, ast), present) in candidates.into_iter().zip(selected) {
            if present {
                self.end_name(&ast.name, &ast.end_name);
                let model = self.models.len();
                if let Some(&previous) = self.model_names.get(&ast.name.value) {
                    let previous_span = self.selected_models[previous].name.span.clone();
                    self.duplicate(&ast.name.span, &previous_span);
                } else {
                    self.model_names.insert(ast.name.value.clone(), model);
                }
                self.models.push(ModelInfo {
                    name: ast.name.value.clone(),
                    language,
                    language_tag: ast.language_tag.as_ref().map(|n| n.value.clone()),
                    issuer: ast.issuer.value.clone(),
                    version: ast.version.value.clone(),
                    contracted: ast.contracted.is_some(),
                    imports: vec![],
                });
                self.bound_imports.push(BTreeMap::new());
                self.selected_models.push(ast);
                if self.diagnostics.limited {
                    return;
                }
            }
        }
    }
    fn bind_imports(&mut self) {
        for model in 0..self.selected_models.len() {
            for import in &self.selected_models[model].imports {
                if !self.step(import.name.value.len().saturating_add(1), &import.span) {
                    return;
                }
                if import.name.value == "INTERLIS"
                    || self.model_names.contains_key(&import.name.value)
                {
                    let name = import.name.value.as_str();
                    self.bound_imports[model]
                        .entry(name)
                        .and_modify(|flag| *flag |= import.unqualified.is_some())
                        .or_insert_with(|| {
                            self.models[model].imports.push(name.to_owned());
                            import.unqualified.is_some()
                        });
                } else {
                    self.diagnostics.push(
                        "ILI-IMPORT-NOT-FOUND",
                        "Imported model is not present under its declared model name",
                        Some(import.span.clone()),
                    );
                }
            }
        }
    }
    fn check_import_cycles(&mut self) {
        #[derive(Clone, Copy, PartialEq)]
        enum Visit {
            Unseen,
            Active,
            Done,
        }
        let mut seen = vec![Visit::Unseen; self.selected_models.len()];
        let mut stack = Vec::new();
        for start in 0..self.selected_models.len() {
            if seen[start] != Visit::Unseen {
                continue;
            }
            seen[start] = Visit::Active;
            stack.push((start, self.selected_models[start].imports.iter()));
            while let Some((current, edges)) = stack.last_mut() {
                let Some(edge) = edges.next() else {
                    seen[*current] = Visit::Done;
                    stack.pop();
                    continue;
                };
                if !self.step(1, &edge.span) {
                    return;
                }
                let Some(&target) = self.model_names.get(&edge.name.value) else {
                    continue; // The predefined model has no supplied import edges.
                };
                match seen[target] {
                    Visit::Done => {}
                    Visit::Unseen => {
                        seen[target] = Visit::Active;
                        stack.push((target, self.selected_models[target].imports.iter()));
                    }
                    Visit::Active => {
                        self.diagnostics.push_related(
                            "ILI-IMPORT-CYCLE",
                            "Model import closes a dependency cycle",
                            Some(edge.name.span.clone()),
                            vec![RelatedLocation {
                                span: self.selected_models[target].name.span.clone(),
                                message: "Model already on the import path".into(),
                            }],
                        );
                        if self.diagnostics.limited {
                            return;
                        }
                    }
                }
            }
        }
    }
    fn check_translation_cycles(&mut self) {
        let mut visited_by = vec![None; self.selected_models.len()];
        for start in 0..self.selected_models.len() {
            let mut current = start;
            while visited_by[current].is_none() {
                visited_by[current] = Some(start);
                let Some(translation) = &self.selected_models[current].translation else {
                    break;
                };
                if !self.step(1, &translation.span) {
                    return;
                }
                let target = self.model_names[&translation.original.value];
                if visited_by[target] == Some(start) {
                    self.diagnostics.push_related(
                        "ILI-TRANSLATION-CYCLE",
                        "Translation original closes a dependency cycle",
                        Some(translation.original.span.clone()),
                        vec![RelatedLocation {
                            span: self.selected_models[target].name.span.clone(),
                            message: "Model already on the translation path".into(),
                        }],
                    );
                    break;
                }
                current = target;
            }
            if self.diagnostics.limited {
                return;
            }
        }
    }
    fn index_declarations(&mut self, parent: usize, model: usize, declarations: &'a [SyntaxDecl]) {
        for ast in declarations {
            if self.diagnostics.limited {
                return;
            }
            let first_symbol = self.symbols.len();
            match ast {
                SyntaxDecl::Topic {
                    name,
                    options,
                    declarations,
                    end_name,
                    span,
                    ..
                } => {
                    self.end_name(name, end_name);
                    if let Some(id) = self.add(Kind::Topic, name, span, Some(parent), model, None) {
                        self.index_abstract_final(id, &options.properties);
                        self.index_declarations(id, model, declarations);
                    }
                }
                SyntaxDecl::Container {
                    kind,
                    name,
                    attributes,
                    properties,
                    parameters,
                    end_name,
                    span,
                    ..
                } => {
                    self.end_name(name, end_name);
                    let kind = match kind {
                        syntax::ContainerKind::Class => Kind::Class,
                        syntax::ContainerKind::Structure => Kind::Structure,
                    };
                    if let Some(id) = self.add(kind, name, span, Some(parent), model, None) {
                        self.index_abstract_final(id, properties);
                        self.index_declarations(id, model, attributes);
                        for parameter in parameters {
                            self.add(
                                Kind::Parameter,
                                &parameter.name,
                                &parameter.span,
                                Some(id),
                                model,
                                None,
                            );
                        }
                    }
                }
                SyntaxDecl::Domain {
                    name,
                    properties,
                    ty,
                    mandatory,
                    base,
                    span,
                    ..
                } => {
                    if let Some(id) = self.add(
                        Kind::Domain,
                        name,
                        span,
                        Some(parent),
                        model,
                        Some(*mandatory),
                    ) {
                        self.symbols[id].syntax = ty.as_ref();
                        self.symbols[id].base = base.as_ref();
                        self.index_abstract_final(id, properties);
                    }
                }
                SyntaxDecl::Attribute {
                    name,
                    type_use,
                    span,
                    ..
                } => {
                    if let Some(id) = self.add(
                        Kind::Attribute,
                        name,
                        span,
                        Some(parent),
                        model,
                        Some(type_use.mandatory),
                    ) {
                        self.symbols[id].syntax = type_use.ty.as_ref();
                    }
                }
                SyntaxDecl::RuntimeParameter {
                    name,
                    span,
                    type_use,
                } => {
                    if self.models[model].language == crate::LanguageVersion::V23
                        && !self.models[model].is_contracted()
                    {
                        self.diagnostics.push(
                            "ILI-PARAMETER-CONTRACTED",
                            "INTERLIS 2.3 runtime parameters require a CONTRACTED model",
                            Some(name.span.clone()),
                        );
                    }
                    if let Some(id) = self.add(
                        Kind::RuntimeParameter,
                        name,
                        span,
                        Some(parent),
                        model,
                        Some(type_use.mandatory),
                    ) {
                        self.symbols[id].syntax = type_use.ty.as_ref();
                    }
                }
                SyntaxDecl::Unit(unit) => {
                    let name = match &unit.display {
                        Some(syntax::UnitDisplay::ShortName(short)) => short,
                        _ => &unit.name,
                    };
                    self.add(Kind::Unit, name, &unit.span, Some(parent), model, None);
                }
                SyntaxDecl::LineForm(form) => {
                    self.add(
                        Kind::LineForm,
                        &form.name,
                        &form.span,
                        Some(parent),
                        model,
                        None,
                    );
                }
                SyntaxDecl::Function(f) => {
                    if let Some(id) =
                        self.add(Kind::Function, &f.name, &f.span, Some(parent), model, None)
                    {
                        for argument in &f.arguments {
                            self.add(
                                Kind::Parameter,
                                &argument.name,
                                &argument.span,
                                Some(id),
                                model,
                                None,
                            );
                        }
                    }
                }
                SyntaxDecl::Context(context) => {
                    self.add(
                        Kind::Context,
                        &context.name,
                        &context.span,
                        Some(parent),
                        model,
                        None,
                    );
                }
                SyntaxDecl::Association(association) => {
                    match (&association.name, &association.end_name) {
                        (Some(name), Some(end)) => self.end_name(name, end),
                        (Some(name), None) => self.diagnostics.push_related(
                            "ILI-END-NAME",
                            "A named association must repeat its name after END",
                            Some(association.end_span.clone()),
                            vec![RelatedLocation {
                                span: name.span.clone(),
                                message: "Association name".into(),
                            }],
                        ),
                        (None, Some(end)) => self.diagnostics.push(
                            "ILI-END-NAME",
                            "An anonymous association ends without a name",
                            Some(end.span.clone()),
                        ),
                        (None, None) => {}
                    }
                    let implicit;
                    let name = if let Some(name) = &association.name {
                        name
                    } else if !association.roles.is_empty() {
                        let bytes = association.roles.iter().map(|r| r.name.value.len()).sum();
                        if !self.step(bytes, &association.span) {
                            return;
                        }
                        implicit = Name {
                            value: association
                                .roles
                                .iter()
                                .map(|r| r.name.value.as_str())
                                .collect(),
                            span: association.roles[0].name.span.clone(),
                        };
                        &implicit
                    } else {
                        continue;
                    };
                    if let Some(id) = self.add(
                        Kind::Association,
                        name,
                        &association.span,
                        Some(parent),
                        model,
                        None,
                    ) {
                        for role in &association.roles {
                            self.add(Kind::Role, &role.name, &role.span, Some(id), model, None);
                        }
                        self.index_declarations(id, model, &association.attributes);
                    }
                }
                SyntaxDecl::View(view) => {
                    self.end_name(&view.name, &view.end_name);
                    if let Some(id) = self.add(
                        Kind::View,
                        &view.name,
                        &view.span,
                        Some(parent),
                        model,
                        None,
                    ) {
                        for attribute in &view.attributes {
                            match attribute {
                                syntax::ViewAttribute::Typed(item) => self.index_declarations(
                                    id,
                                    model,
                                    std::slice::from_ref(item.as_ref()),
                                ),
                                syntax::ViewAttribute::Derived { name, span, .. } => {
                                    self.add(Kind::Attribute, name, span, Some(id), model, None);
                                }
                                syntax::ViewAttribute::AllOf(_) => {}
                            }
                        }
                    }
                }
                SyntaxDecl::Graphic(graphic) => {
                    self.end_name(&graphic.name, &graphic.end_name);
                    if let Some(id) = self.add(
                        Kind::Graphic,
                        &graphic.name,
                        &graphic.span,
                        Some(parent),
                        model,
                        None,
                    ) {
                        for rule in &graphic.rules {
                            self.add(
                                Kind::DrawingRule,
                                &rule.name,
                                &rule.span,
                                Some(id),
                                model,
                                None,
                            );
                        }
                    }
                }
                SyntaxDecl::MetadataBasket(basket) => {
                    if let Some(id) = self.add(
                        Kind::MetadataBasket,
                        &basket.name,
                        &basket.span,
                        Some(parent),
                        model,
                        None,
                    ) {
                        for name in basket.groups.iter().flat_map(|group| &group.objects) {
                            self.add(Kind::MetaObject, name, &name.span, Some(id), model, None);
                        }
                    }
                }
                SyntaxDecl::Constraints(_) => {} // A block augments its target's namespace.
            }
            if let Some(symbol) = self.symbols.get_mut(first_symbol) {
                symbol.ast = Some(ast);
            }
        }
    }
    fn index_abstract_final(
        &mut self,
        id: usize,
        properties: &[syntax::Located<syntax::Property>],
    ) {
        use syntax::Property::{Abstract, Final};
        for property in [Abstract, Final] {
            let mut appearances = properties.iter().filter(|p| p.value == property);
            if let Some(first) = appearances.next() {
                if property == Abstract {
                    self.symbols[id].declaration.abstract_declaration = true;
                } else {
                    self.symbols[id].declaration.final_declaration = true;
                }
                if let Some(repeated) = appearances.next() {
                    self.duplicate(&repeated.span, &first.span);
                }
            }
        }
        if self.symbols[id].declaration.is_abstract() && self.symbols[id].declaration.is_final() {
            self.diagnostics.push(
                "ILI-PROPERTY-CONFLICT",
                "A declaration cannot be both ABSTRACT and FINAL",
                Some(self.symbols[id].name_span.clone()),
            );
        }
    }
    fn bound_reference(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        target: BoundName,
    ) -> Option<TypeReference> {
        let (target, predefined_language) = match target {
            BoundName::Declared(id) => {
                let bytes = self.symbols[id].declaration.qualified.len();
                if !self.step(bytes, &name.span) {
                    return None;
                }
                (self.symbols[id].declaration.qualified.clone(), None)
            }
            BoundName::Predefined(name, _) => (
                format!("INTERLIS.{name}"),
                Some(self.models[self.symbols[owner].model].language),
            ),
        };
        Some(TypeReference {
            declared: name
                .segments
                .iter()
                .map(|part| part.value.as_str())
                .collect::<Vec<_>>()
                .join("."),
            target,
            predefined_language,
            span: name.span.clone(),
        })
    }
    fn bind_name(&mut self, owner: usize, name: &QualifiedName) -> Option<Target> {
        let kinds = if self.symbols[owner].declaration.kind == Kind::Domain {
            &[Kind::Domain][..]
        } else {
            &[Kind::Domain, Kind::Structure]
        };
        let (target, qualified, predefined_language) = match self.lookup_name(
            owner,
            &name.segments,
            &name.span,
            kinds,
        ) {
            Ok(BoundName::Declared(id)) => {
                let declaration = &self.symbols[id].declaration;
                if declaration.kind == Kind::Domain
                    && self.symbols[owner].declaration.kind != Kind::Domain
                    && !self.check_domain_order(owner, id, &name.span)
                {
                    return None;
                }
                let declaration = &self.symbols[id].declaration;
                if !self.step(declaration.qualified.len(), &name.span) {
                    return None;
                }
                let declaration = &self.symbols[id].declaration;
                let target = match declaration.kind {
                    Kind::Domain => Target::Domain(id),
                    Kind::Structure => Target::Structure,
                    _ => unreachable!("lookup only accepts domains or structures"),
                };
                (target, declaration.qualified.clone(), None)
            }
            Ok(BoundName::Predefined(name, kind)) => (
                if kind == Kind::Domain {
                    Target::Predefined(name)
                } else {
                    Target::Structure
                },
                format!("INTERLIS.{name}"),
                Some(self.models[self.symbols[owner].model].language),
            ),
            Err(LookupError::Ambiguous(candidates)) => {
                self.ambiguous_type(name, candidates);
                return None;
            }
            Err(LookupError::Limit) => return None,
            Err(LookupError::Deferred) => {
                self.diagnostics.push(
                    "ILI-UNSUPPORTED",
                    "This type reference requires inherited names",
                    Some(name.span.clone()),
                );
                return None;
            }
            Err(error @ (LookupError::NotImported | LookupError::Missing)) => {
                let message = match error {
                    LookupError::NotImported => {
                        "Qualified domain must use the current or an explicitly imported model name"
                    }
                    _ => "Domain reference does not resolve in the supported scope",
                };
                self.diagnostics
                    .push("ILI-TYPE-NOT-FOUND", message, Some(name.span.clone()));
                return None;
            }
        };
        self.symbols[owner].declaration.reference = Some(TypeReference {
            declared: name
                .segments
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>()
                .join("."),
            target: qualified,
            span: name.span.clone(),
            predefined_language,
        });
        Some(target)
    }
    fn domain_follows_reference(&self, owner: usize, target: usize, reference: &Span) -> bool {
        let target = &self.symbols[target];
        self.symbols[owner].model == target.model
            && target.name_span.sort_key() > reference.sort_key()
    }

    fn check_domain_order(&mut self, owner: usize, target: usize, reference: &Span) -> bool {
        if self.domain_follows_reference(owner, target, reference) {
            let target = &self.symbols[target];
            self.diagnostics.push_related(
                "ILI-DOMAIN-ORDER",
                "A domain must be defined before it is referenced in the same model",
                Some(reference.clone()),
                vec![RelatedLocation {
                    span: target.name_span.clone(),
                    message: "Later domain declaration".into(),
                }],
            );
            false
        } else {
            true
        }
    }
    fn ambiguous_type(&mut self, name: &QualifiedName, candidates: [usize; 2]) {
        self.diagnostics.push_related(
            "ILI-TYPE-AMBIGUOUS",
            "Unqualified type is provided by more than one imported model",
            Some(name.span.clone()),
            candidates
                .iter()
                .map(|&id| RelatedLocation {
                    span: self.symbols[id].name_span.clone(),
                    message: "Candidate declaration".into(),
                })
                .collect(),
        );
    }
    fn predefined_type(&mut self, name: &str, span: &Span) -> Option<Effective> {
        let ty = match name {
            "URI" => TypeInfo::FixedText {
                format: FixedTextFormat::Uri,
                max_length: "1023".into(),
            },
            "NAME" => TypeInfo::FixedText {
                format: FixedTextFormat::Name,
                max_length: "255".into(),
            },
            "INTERLIS_1_DATE" => TypeInfo::FixedText {
                format: FixedTextFormat::Interlis1Date,
                max_length: "8".into(),
            },
            "BOOLEAN" => TypeInfo::Boolean,
            "HALIGNMENT" => TypeInfo::Alignment(syntax::AlignmentKind::Horizontal),
            "VALIGNMENT" => TypeInfo::Alignment(syntax::AlignmentKind::Vertical),
            "XMLDate" => TypeInfo::Temporal(syntax::TemporalKind::Date),
            "XMLTime" => TypeInfo::Temporal(syntax::TemporalKind::TimeOfDay),
            "XMLDateTime" => TypeInfo::Temporal(syntax::TemporalKind::DateTime),
            "NOOID" => TypeInfo::Oid(OidType::Unstable),
            "ANYOID" => TypeInfo::Oid(OidType::Any),
            "STANDARDOID" => TypeInfo::Oid(OidType::Standard),
            "UUIDOID" => TypeInfo::Oid(OidType::Uuid),
            "I32OID" => {
                let bound = |value: &str| model::DecimalInfo {
                    original: value.into(),
                    coefficient: value.into(),
                    fractional_digits: 0,
                    exponent: 0,
                    scaled: false,
                };
                TypeInfo::Oid(OidType::Typed(Box::new(TypeInfo::Numeric(
                    model::NumericRange {
                        bounds: Some(model::NumericBounds {
                            lower: bound("0"),
                            upper: bound("2147483647"),
                        }),
                        unit: None,
                        circular: false,
                        direction: None,
                    },
                ))))
            }
            "LineCoord" => {
                let axis = || crate::NumericRange {
                    bounds: None,
                    unit: None,
                    circular: false,
                    direction: None,
                };
                TypeInfo::Coordinate(crate::CoordinateType {
                    kind: crate::CoordinateKind::Single,
                    axes: vec![axis(), axis()],
                    rotation: None,
                    reference_system: None,
                })
            }
            _ => {
                self.diagnostics.push(
                    "ILI-UNSUPPORTED",
                    "Predefined domain type is outside the implemented subset",
                    Some(span.clone()),
                );
                return None;
            }
        };
        let id = self.types.len();
        self.types.push(ty);
        Some(Effective {
            ty: id,
            mandatory: false,
        })
    }

    fn primitive_reference(
        &mut self,
        owner: usize,
        declared: &str,
        predefined: &str,
        span: &Span,
    ) -> Option<Effective> {
        self.symbols[owner].declaration.reference = Some(TypeReference {
            declared: declared.into(),
            target: format!("INTERLIS.{predefined}"),
            span: span.clone(),
            predefined_language: Some(self.models[self.symbols[owner].model].language),
        });
        self.predefined_type(predefined, span)
    }
    fn number_error(&mut self, error: NumberError, span: &Span) {
        let (code, message) = match error {
            NumberError::Form => ("ILI-SYNTAX", "Invalid INTERLIS decimal literal spelling"),
            NumberError::Limit => ("ILI-LIMIT", "Numeric digit or exponent budget exceeded"),
            NumberError::Work => ("ILI-LIMIT", "Semantic work budget exceeded"),
        };
        self.diagnostics.push(code, message, Some(span.clone()));
    }
    fn literal(&mut self, number: &syntax::Located<String>) -> Option<DecimalLiteral> {
        match DecimalLiteral::parse(&number.value, self.limits, &mut self.work) {
            Ok(value) => Some(value),
            Err(error) => {
                self.number_error(error, &number.span);
                None
            }
        }
    }
    fn text_type(&mut self, text: &syntax::TextSyntax) -> Option<TypeInfo> {
        if !self.step(1, &text.span) {
            return None;
        }
        let fixed = match text.kind {
            syntax::TextKind::Name => Some((FixedTextFormat::Name, "255")),
            syntax::TextKind::Uri => Some((FixedTextFormat::Uri, "1023")),
            _ => None,
        };
        if let Some((format, length)) = fixed {
            return Some(TypeInfo::FixedText {
                format,
                max_length: length.into(),
            });
        }
        let max_length = if let Some(maximum) = &text.max_length {
            let length = self.literal(maximum)?;
            if !length.is_positive_integer() {
                self.diagnostics.push(
                    "ILI-TEXT-LENGTH",
                    "Text length must be a positive integer",
                    Some(maximum.span.clone()),
                );
                return None;
            }
            Some(length.info.original)
        } else {
            None
        };
        match (text.kind, max_length) {
            (syntax::TextKind::MText, max_length) => Some(TypeInfo::MultilineText { max_length }),
            (syntax::TextKind::Text, Some(max_length)) => Some(TypeInfo::Text { max_length }),
            (syntax::TextKind::Text, None) => Some(TypeInfo::UnboundedText),
            _ => unreachable!("NAME and URI have fixed text formats"),
        }
    }
    fn concrete_type(&mut self, owner: usize, syntax: &TypeSyntax) -> Option<Effective> {
        let ty = match syntax {
            TypeSyntax::Text(text) => {
                let predefined = match text.kind {
                    syntax::TextKind::Name => Some("NAME"),
                    syntax::TextKind::Uri => Some("URI"),
                    _ => None,
                };
                if let Some(name) = predefined {
                    return self.primitive_reference(owner, name, name, &text.span);
                }
                self.text_type(text)?
            }
            TypeSyntax::Boolean { span } => {
                if !self.step(1, span) {
                    return None;
                }
                return self.primitive_reference(owner, "BOOLEAN", "BOOLEAN", span);
            }
            TypeSyntax::Alignment { kind, span } => {
                let name = match kind {
                    syntax::AlignmentKind::Horizontal => "HALIGNMENT",
                    syntax::AlignmentKind::Vertical => "VALIGNMENT",
                };
                return self.primitive_reference(owner, name, name, span);
            }
            TypeSyntax::Temporal { kind, span } => {
                let (declared, predefined) = match kind {
                    syntax::TemporalKind::Date => ("DATE", "XMLDate"),
                    syntax::TemporalKind::TimeOfDay => ("TIMEOFDAY", "XMLTime"),
                    syntax::TemporalKind::DateTime => ("DATETIME", "XMLDateTime"),
                };
                return self.primitive_reference(owner, declared, predefined, span);
            }
            TypeSyntax::Blackbox { kind, .. } => TypeInfo::Blackbox(*kind),
            TypeSyntax::Enumeration {
                enumeration, order, ..
            } => {
                let order = order.as_ref().map(|order| order.value);
                TypeInfo::Enumeration {
                    values: self.new_enumeration(enumeration)?,
                    order,
                    declared_order: order,
                }
            }
            TypeSyntax::EnumTree { domain, .. } => {
                let value = match self.bind_name(owner, domain)? {
                    Target::Domain(id) => self.resolve_domain(id)?,
                    Target::Predefined(name) => self.predefined_type(name, &domain.span)?,
                    Target::Structure => {
                        self.diagnostics.push(
                            "ILI-ENUM-TARGET",
                            "ALL OF requires an enumeration domain",
                            Some(domain.span.clone()),
                        );
                        return None;
                    }
                };
                return self.enum_tree_type(owner, value, &domain.span);
            }
            TypeSyntax::Numeric(numeric) => TypeInfo::Numeric(self.numeric_range(owner, numeric)?),
            TypeSyntax::Coordinate(coordinate) => {
                TypeInfo::Coordinate(self.coordinate_type(owner, coordinate)?)
            }
            TypeSyntax::Oid { carrier, .. } => TypeInfo::Oid(match carrier {
                syntax::OidSyntax::Any(_) => OidType::Any,
                syntax::OidSyntax::Numeric(range) => OidType::Typed(Box::new(TypeInfo::Numeric(
                    self.numeric_range(owner, range)?,
                ))),
                syntax::OidSyntax::Text(text) => OidType::Typed(Box::new(self.text_type(text)?)),
            }),
            TypeSyntax::Named(name) => {
                return match self.bind_name(owner, name)? {
                    Target::Domain(id) => self.resolve_domain(id),
                    Target::Predefined(domain) => self.predefined_type(domain, &name.span),
                    Target::Structure => {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "Structure-valued attributes are outside the supported subset",
                            Some(name.span.clone()),
                        );
                        None
                    }
                };
            }
            TypeSyntax::Formatted { .. }
            | TypeSyntax::Geometry(_)
            | TypeSyntax::ClassValue { .. }
            | TypeSyntax::AttributePathValue { .. }
            | TypeSyntax::Reference { .. }
            | TypeSyntax::Collection { .. }
            | TypeSyntax::RestrictedStructure { .. } => {
                unreachable!("unsupported type semantics checked before indexing")
            }
        };
        let id = self.types.len();
        self.types.push(ty);
        Some(Effective {
            ty: id,
            mandatory: false,
        })
    }
    fn cycle(&mut self, path: &[usize], current: usize) {
        let first = path.iter().position(|&id| id == current).unwrap_or(0);
        let cycle = &path[first..];
        let mut shown = cycle
            .iter()
            .take(16)
            .map(|&id| {
                self.symbols[id]
                    .declaration
                    .name
                    .chars()
                    .take(80)
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        if cycle.len() > 16 {
            shown.push("…".into());
        }
        shown.push(
            self.symbols[current]
                .declaration
                .name
                .chars()
                .take(80)
                .collect(),
        );
        let primary = path
            .last()
            .and_then(|&id| self.symbols[id].domain_dependency())
            .map_or(&self.symbols[current].name_span, |q| &q.span)
            .clone();
        let related = cycle
            .iter()
            .take(16)
            .map(|&id| RelatedLocation {
                span: self.symbols[id].name_span.clone(),
                message: "Domain in derivation cycle".into(),
            })
            .collect();
        self.diagnostics.push_related(
            "ILI-DOMAIN-CYCLE",
            &format!("Domain derivation cycle: {}", shown.join(" -> ")),
            Some(primary),
            related,
        );
    }
    fn resolve_domain(&mut self, start: usize) -> Option<Effective> {
        let mut current = start;
        let mut path = vec![];
        let mut forward = None;
        let result = loop {
            let span = self.symbols[current].name_span.clone();
            if !self.step(1, &span) {
                break None;
            }
            match self.states[current] {
                State::Resolved(value) => break Some(value),
                State::Failed => break None,
                State::Resolving => {
                    self.cycle(&path, current);
                    break None;
                }
                State::Unseen => {}
            }
            self.states[current] = State::Resolving;
            path.push(current);
            if let Some(reference) = self.symbols[current].domain_dependency() {
                match self.bind_name(current, reference) {
                    Some(Target::Domain(next)) => {
                        if self.symbols[current].base.is_some() {
                            let declaration = &mut self.symbols[current].declaration;
                            declaration.base_reference = declaration.reference.take();
                        }
                        if self.symbols[current].base.is_some()
                            && self.symbols[next].declaration.is_final()
                        {
                            self.diagnostics.push_related(
                                "ILI-DOMAIN-FINAL",
                                "A FINAL domain cannot be extended",
                                Some(reference.span.clone()),
                                vec![RelatedLocation {
                                    span: self.symbols[next].name_span.clone(),
                                    message: "FINAL base domain".into(),
                                }],
                            );
                            break None;
                        }
                        if forward.is_none()
                            && self.domain_follows_reference(current, next, &reference.span)
                        {
                            forward = Some((current, next, &reference.span));
                        }
                        current = next;
                        continue;
                    }
                    Some(Target::Predefined(name))
                        if matches!(
                            self.symbols[current].syntax,
                            Some(TypeSyntax::EnumTree { .. })
                        ) =>
                    {
                        break self.predefined_type(name, &reference.span);
                    }
                    Some(Target::Predefined(
                        "BOOLEAN" | "URI" | "NAME" | "INTERLIS_1_DATE" | "HALIGNMENT"
                        | "VALIGNMENT",
                    )) => {
                        self.diagnostics.push(
                            "ILI-DERIVATION",
                            "This predefined domain is final and cannot be derived",
                            Some(reference.span.clone()),
                        );
                        break None;
                    }
                    Some(Target::Predefined(
                        name @ ("LineCoord" | "NOOID" | "ANYOID" | "I32OID" | "STANDARDOID"
                        | "UUIDOID"),
                    )) => {
                        let declaration = &mut self.symbols[current].declaration;
                        declaration.base_reference = declaration.reference.take();
                        break self.predefined_type(name, &reference.span);
                    }
                    Some(Target::Predefined(_)) => {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "This predefined domain derivation is outside the implemented subset",
                            Some(reference.span.clone()),
                        );
                        break None;
                    }
                    Some(_) => {
                        self.diagnostics.push(
                            "ILI-TYPE-NOT-FOUND",
                            "A domain derivation must reference a domain",
                            Some(reference.span.clone()),
                        );
                        break None;
                    }
                    None => break None,
                }
            }
            let Some(syntax) = self.symbols[current].syntax else {
                break None;
            };
            break self.concrete_type(current, syntax);
        };
        let mut result = result.filter(|_| {
            forward.is_none_or(|(owner, target, span)| self.check_domain_order(owner, target, span))
        });
        for id in path.into_iter().rev() {
            if let Some(TypeSyntax::EnumTree { domain, .. }) = self.symbols[id].syntax {
                result = result.and_then(|value| self.enum_tree_type(id, value, &domain.span));
            } else if self.symbols[id].base.is_some()
                && let Some(TypeSyntax::Enumeration {
                    enumeration, order, ..
                }) = self.symbols[id].syntax
            {
                let order = order.as_ref().map(|order| order.value);
                result =
                    result.and_then(|value| self.extend_enumeration(value, enumeration, order));
            } else if self.symbols[id].base.is_some()
                && let Some(ty) = self.symbols[id].syntax
            {
                result = result.and_then(|value| self.specialize_scalar(id, value, ty));
            } else if self.symbols[id].base.is_some()
                && result.is_some_and(|value| matches!(self.types[value.ty], TypeInfo::Oid(_)))
            {
                self.diagnostics.push(
                    "ILI-OID-EXTENSION",
                    "An OID domain extension must specialize an open OID with a concrete definition",
                    Some(self.symbols[id].name_span.clone()),
                );
                result = None;
            }
            if result.is_some_and(|value| self.types[value.ty].has_unbounded_range())
                && !self.symbols[id].declaration.is_abstract()
            {
                self.diagnostics.push(
                    "ILI-DOMAIN-ABSTRACT",
                    "A numeric or coordinate domain with unbounded axes must be ABSTRACT",
                    Some(self.symbols[id].name_span.clone()),
                );
                result = None;
            }
            if let Some(mut value) = result {
                value.mandatory |= self.symbols[id]
                    .declaration
                    .declared_mandatory
                    .unwrap_or(false);
                self.symbols[id].declaration.type_id = Some(value.ty);
                self.states[id] = State::Resolved(value);
                result = Some(value);
            } else {
                self.states[id] = State::Failed;
            }
        }
        result
    }
    fn run(mut self, requested: Option<&[String]>) -> Option<ModelGraph> {
        self.collect_models(requested);
        if self.diagnostics.limited || self.diagnostics.rejected {
            return None;
        }
        self.bind_imports();
        if !self.diagnostics.rejected && !self.diagnostics.limited {
            self.check_import_cycles();
            self.check_translation_cycles();
        }
        if self.diagnostics.limited || self.diagnostics.rejected {
            return None;
        }
        for model in 0..self.selected_models.len() {
            let ast = self.selected_models[model];
            let symbol = self.add(Kind::Model, &ast.name, &ast.span, None, model, None)?;
            self.index_declarations(symbol, model, &ast.declarations);
            if self.diagnostics.limited {
                return None;
            }
        }
        if self.diagnostics.limited {
            return None;
        }
        let topics = self.bind_topics()?;
        self.check_direct_references();
        if self.diagnostics.limited
            || self.diagnostics.rejected
            || !ensure_supported_semantics(&self.selected_models, self.diagnostics, &mut self.work)
        {
            return None;
        }
        self.check_topic_members()?;
        self.check_abstract_topics(&topics)?;
        self.bind_units()?;
        for symbol in &self.symbols {
            if symbol.declaration.kind == Kind::Class
                && !symbol.declaration.is_abstract()
                && symbol
                    .declaration
                    .parent
                    .is_some_and(|p| self.symbols[p].declaration.kind == Kind::Model)
            {
                self.diagnostics.push(
                    "ILI-SCOPE",
                    "A concrete class in the supported data-model subset must be in a topic",
                    Some(symbol.name_span.clone()),
                );
            }
        }
        for id in 0..self.symbols.len() {
            if self.symbols[id].declaration.kind == Kind::Domain {
                self.resolve_domain(id);
            }
            if self.diagnostics.limited {
                return None;
            }
        }
        self.bind_oid_rules(&topics)?;
        for id in 0..self.symbols.len() {
            if !matches!(
                self.symbols[id].declaration.kind,
                Kind::Attribute | Kind::RuntimeParameter
            ) {
                continue;
            }
            let Some(syntax) = self.symbols[id].syntax else {
                continue;
            };
            if let Some(value) = self.concrete_type(id, syntax) {
                let symbol = &self.symbols[id];
                if symbol.declaration.kind == Kind::Attribute
                    && (self.types[value.ty].has_unbounded_range()
                        || symbol
                            .declaration
                            .type_reference()
                            .and_then(|link| self.names.get(link.target_name()))
                            .is_some_and(|&target| self.symbols[target].declaration.is_abstract()))
                {
                    self.diagnostics.push(
                        "ILI-ATTRIBUTE-ABSTRACT",
                        "An attribute with an abstract value domain must be ABSTRACT",
                        Some(syntax.span().clone()),
                    );
                    continue;
                }
                self.symbols[id].declaration.type_id = Some(value.ty);
                self.symbols[id].declaration.effective_mandatory = Some(
                    value.mandatory
                        || self.symbols[id]
                            .declaration
                            .declared_mandatory
                            .unwrap_or(false),
                );
            }
            if self.diagnostics.limited {
                return None;
            }
        }
        if self.diagnostics.rejected || self.diagnostics.incomplete {
            return None;
        }
        Some(ModelGraph {
            models: self.models,
            declarations: self.symbols.into_iter().map(|s| s.declaration).collect(),
            types: self.types,
            names: self.names,
        })
    }
}
pub(crate) fn analyze(
    documents: &[syntax::Document],
    requested: Option<&[String]>,
    limits: &Limits,
    diagnostics: &mut Diagnostics,
) -> Option<ModelGraph> {
    Analyzer::new(documents, limits, diagnostics).run(requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceInput, SourceSet};
    #[test]
    fn resolving_last_unvisited_domain_walks_a_norm_ordered_chain_iteratively() {
        let mut text = String::from(
            "INTERLIS 2.4; MODEL M AT \"https://example.invalid\" VERSION \"1\" = DOMAIN D0=BOOLEAN; ",
        );
        for n in 1..10000 {
            text.push_str(&format!("D{n} EXTENDS D{}=MANDATORY;\n", n - 1));
        }
        text.push_str("END M.");
        let sources = SourceSet::new(vec![SourceInput {
            name: "a".into(),
            text,
        }])
        .unwrap();
        let limits = Limits::default();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut tokens = crate::limits::SyntaxBudget::new(&limits);
        let document = syntax::parse(&sources, 0, &limits, &mut tokens, &mut diagnostics).unwrap();
        let documents = [document];
        let mut analyzer = Analyzer::new(&documents, &limits, &mut diagnostics);
        analyzer.collect_models(None);
        analyzer.bind_imports();
        let ast = analyzer.selected_models[0];
        let model = 0;
        let owner = analyzer
            .add(Kind::Model, &ast.name, &ast.span, None, model, None)
            .unwrap();
        analyzer.index_declarations(owner, model, &ast.declarations);
        assert!(analyzer.states.iter().all(|s| matches!(s, State::Unseen)));
        let last = *analyzer.names.get("M.D9999").unwrap();
        let value = analyzer.resolve_domain(last).unwrap();
        assert!(value.mandatory);
        assert!(matches!(analyzer.types[value.ty], TypeInfo::Boolean));
        assert!(analyzer.diagnostics.entries.is_empty());
        assert_eq!(
            analyzer
                .states
                .iter()
                .filter(|s| matches!(s, State::Resolved(_)))
                .count(),
            10000
        );
    }
}
