use crate::{
    Limits, RelatedLocation, Span,
    diagnostic::Diagnostics,
    limits::WorkBudget,
    model::{self, DeclarationKind as Kind, ModelGraph, ModelInfo, TypeInfo, TypeReference},
    number::{DecimalLiteral, NumberError},
    syntax::{self, Declaration as SyntaxDecl, DomainType, Name, QualifiedName, TypeSyntax},
};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

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
    name_span: &'a Span,
    model: usize,
    syntax: Option<&'a TypeSyntax>,
    base: Option<&'a QualifiedName>,
}
struct Root<'a> {
    syntax: &'a syntax::ModelDecl,
    model: usize,
}
enum Target {
    Domain(usize),
    Structure,
    Other,
    Boolean,
}
struct Analyzer<'a, 'd> {
    documents: &'a [syntax::Document],
    limits: &'a Limits,
    diagnostics: &'d mut Diagnostics,
    work: WorkBudget,
    symbols: Vec<Symbol<'a>>,
    roots: Vec<Root<'a>>,
    models: Vec<ModelInfo>,
    bound_imports: Vec<BTreeSet<String>>,
    model_names: BTreeMap<String, usize>,
    names: BTreeMap<String, usize>,
    child_names: BTreeMap<(usize, String), usize>,
    type_names: BTreeMap<(usize, String), usize>,
    blocked_models: BTreeSet<usize>,
    ambiguous_models: bool,
    states: Vec<State>,
    types: Vec<TypeInfo>,
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
            roots: vec![],
            models: vec![],
            bound_imports: vec![],
            model_names: BTreeMap::new(),
            names: BTreeMap::new(),
            child_names: BTreeMap::new(),
            type_names: BTreeMap::new(),
            blocked_models: BTreeSet::new(),
            ambiguous_models: false,
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
        name: &'a Name,
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
            let key = (parent, name.value.clone());
            if let Some(&previous) = self.child_names.get(&key) {
                let previous_span = self.symbols[previous].name_span.clone();
                self.duplicate(&name.span, &previous_span);
                duplicate = true;
            } else {
                self.child_names.insert(key.clone(), id);
            }
            if kind != Kind::Attribute {
                self.type_names.entry(key).or_insert(id);
            }
        }
        self.names.entry(qualified.clone()).or_insert(id);
        self.symbols.push(Symbol {
            declaration: model::Declaration {
                name: name.value.clone(),
                qualified,
                kind,
                span: span.clone(),
                parent,
                type_id: None,
                declared_mandatory: mandatory,
                effective_mandatory: None,
                reference: None,
            },
            name_span: &name.span,
            model,
            syntax: None,
            base: None,
        });
        self.states.push(if duplicate {
            State::Failed
        } else {
            State::Unseen
        });
        Some(id)
    }
    fn collect_models(&mut self) {
        for document in self.documents {
            if document.models.is_empty() {
                self.diagnostics.push(
                    "ILI-MODEL-MISSING",
                    "An I-0 analysis source must declare a model",
                    Some(document.span.clone()),
                );
                continue;
            }
            for ast in &document.models {
                if !self.step(1, &ast.name.span) {
                    return;
                }
                self.end_name(&ast.name, &ast.end_name);
                let model = self.models.len();
                if let Some(&previous) = self.model_names.get(&ast.name.value) {
                    let previous_span = self.roots[previous].syntax.name.span.clone();
                    self.duplicate(&ast.name.span, &previous_span);
                    self.ambiguous_models = true;
                } else {
                    self.model_names.insert(ast.name.value.clone(), model);
                }
                self.models.push(ModelInfo {
                    name: ast.name.value.clone(),
                    language: document.language,
                    language_tag: ast.language_tag.as_ref().map(|n| n.value.clone()),
                    issuer: ast.issuer.value.clone(),
                    version: ast.version.value.clone(),
                    imports: vec![],
                });
                self.bound_imports.push(BTreeSet::new());
                self.roots.push(Root { syntax: ast, model });
            }
        }
    }
    fn bind_imports(&mut self) {
        for root in 0..self.roots.len() {
            let model = self.roots[root].model;
            for import in &self.roots[root].syntax.imports {
                if !self.step(import.value.len().saturating_add(1), &import.span) {
                    return;
                }
                if import.value == "INTERLIS" || self.model_names.contains_key(&import.value) {
                    if self.bound_imports[model].insert(import.value.clone()) {
                        self.models[model].imports.push(import.value.clone());
                    }
                } else {
                    self.blocked_models.insert(model);
                    self.diagnostics.push(
                        "ILI-IMPORT-NOT-FOUND",
                        "Imported model is not present under its declared model name",
                        Some(import.span.clone()),
                    );
                }
            }
        }
    }
    fn index_declarations(&mut self, parent: usize, model: usize, declarations: &'a [SyntaxDecl]) {
        for ast in declarations {
            if self.diagnostics.limited {
                return;
            }
            match ast {
                SyntaxDecl::Topic {
                    name,
                    declarations,
                    end_name,
                    span,
                } => {
                    self.end_name(name, end_name);
                    if let Some(id) = self.add(Kind::Topic, name, span, Some(parent), model, None) {
                        self.index_declarations(id, model, declarations);
                    }
                }
                SyntaxDecl::Container {
                    kind,
                    name,
                    attributes,
                    end_name,
                    span,
                } => {
                    self.end_name(name, end_name);
                    let kind = match kind {
                        syntax::ContainerKind::Class => Kind::Class,
                        syntax::ContainerKind::Structure => Kind::Structure,
                    };
                    if kind == Kind::Class && self.symbols[parent].declaration.kind == Kind::Model {
                        self.diagnostics.push("ILI-SCOPE","A concrete class in the supported data-model subset must be in a topic",Some(name.span.clone()));
                    }
                    if let Some(id) = self.add(kind, name, span, Some(parent), model, None) {
                        self.index_declarations(id, model, attributes);
                    }
                }
                SyntaxDecl::Domain {
                    name,
                    ty,
                    mandatory,
                    span,
                } => {
                    if let Some(id) = self.add(
                        Kind::Domain,
                        name,
                        span,
                        Some(parent),
                        model,
                        Some(*mandatory),
                    ) {
                        match ty {
                            DomainType::Concrete(ty) => self.symbols[id].syntax = Some(ty),
                            DomainType::MandatoryDerivation(base) => {
                                self.symbols[id].base = Some(base)
                            }
                        }
                    }
                }
                SyntaxDecl::Attribute {
                    name,
                    ty,
                    mandatory,
                    span,
                } => {
                    if let Some(id) = self.add(
                        Kind::Attribute,
                        name,
                        span,
                        Some(parent),
                        model,
                        Some(*mandatory),
                    ) {
                        self.symbols[id].syntax = Some(ty);
                    }
                }
            }
        }
    }
    fn bind_name(&mut self, owner: usize, name: &QualifiedName) -> Option<Target> {
        let cost = name.segments.iter().try_fold(1_usize, |n, s| {
            n.checked_add(s.value.len().saturating_add(1))
        });
        if !self.step(cost.unwrap_or(usize::MAX), &name.span) {
            return None;
        }
        let model = self.symbols[owner].model;
        if self.blocked_models.contains(&model) {
            return None;
        }
        let parts = &name.segments;
        let id = if parts.len() == 1 {
            let mut scope = self.symbols[owner].declaration.parent;
            let mut found = None;
            while let Some(parent) = scope {
                if !self.step(1, &name.span) {
                    return None;
                }
                if let Some(&id) = self.type_names.get(&(parent, parts[0].value.clone())) {
                    found = Some(id);
                    break;
                }
                scope = self.symbols[parent].declaration.parent;
            }
            found
        } else {
            let first = &parts[0].value;
            if first == "INTERLIS" {
                return if parts.len() == 2 && parts[1].value == "BOOLEAN" {
                    Some(Target::Boolean)
                } else {
                    self.diagnostics.push(
                        "ILI-UNSUPPORTED",
                        "Predefined type is outside the supported subset",
                        Some(name.span.clone()),
                    );
                    None
                };
            }
            if first != &self.models[model].name && !self.bound_imports[model].contains(first) {
                self.diagnostics.push(
                    "ILI-TYPE-NOT-FOUND",
                    "Qualified domain must use the current or an explicitly imported model name",
                    Some(name.span.clone()),
                );
                return None;
            }
            let qualified = parts
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>()
                .join(".");
            self.names.get(&qualified).copied()
        };
        let Some(id) = id else {
            self.diagnostics.push(
                "ILI-TYPE-NOT-FOUND",
                "Domain reference does not resolve in the supported scope",
                Some(name.span.clone()),
            );
            return None;
        };
        let target = &self.symbols[id];
        if self.blocked_models.contains(&target.model) {
            return None;
        }
        let kind = target.declaration.kind;
        if !self.step(target.declaration.qualified.len(), &name.span) {
            return None;
        }
        self.symbols[owner].declaration.reference = Some(TypeReference {
            declared: parts
                .iter()
                .map(|n| n.value.as_str())
                .collect::<Vec<_>>()
                .join("."),
            target: self.symbols[id].declaration.qualified.clone(),
            span: name.span.clone(),
        });
        Some(match kind {
            Kind::Domain => Target::Domain(id),
            Kind::Structure => Target::Structure,
            _ => Target::Other,
        })
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
    fn concrete_type(&mut self, owner: usize, syntax: &TypeSyntax) -> Option<Effective> {
        let ty = match syntax {
            TypeSyntax::Text { max_length, span } => {
                if !self.step(1, span) {
                    return None;
                }
                let length = self.literal(max_length)?;
                if !length.is_positive_integer() {
                    self.diagnostics.push(
                        "ILI-TEXT-LENGTH",
                        "TEXT length must be a positive integer",
                        Some(max_length.span.clone()),
                    );
                    return None;
                }
                TypeInfo::Text {
                    max_length: length.info.original,
                }
            }
            TypeSyntax::Boolean { span } => {
                if !self.step(1, span) {
                    return None;
                }
                TypeInfo::Boolean
            }
            TypeSyntax::Numeric { lower, upper, span } => {
                if !self.step(1, span) {
                    return None;
                }
                let low = self.literal(lower)?;
                let high = self.literal(upper)?;
                if low.info.fractional_digits() != high.info.fractional_digits() {
                    self.diagnostics.push_related(
                        "ILI-NUMERIC-PRECISION",
                        "Numeric bounds must have matching written fractional precision",
                        Some(upper.span.clone()),
                        vec![RelatedLocation {
                            span: lower.span.clone(),
                            message: "Lower-bound precision".into(),
                        }],
                    );
                    return None;
                }
                match low.compare_value(&high, &mut self.work) {
                    Ok(Ordering::Greater) => {
                        self.diagnostics.push_related(
                            "ILI-NUMERIC-RANGE",
                            "Lower numeric bound exceeds upper bound",
                            Some(lower.span.clone()),
                            vec![RelatedLocation {
                                span: upper.span.clone(),
                                message: "Upper bound".into(),
                            }],
                        );
                        return None;
                    }
                    Err(error) => {
                        self.number_error(error, &lower.span);
                        return None;
                    }
                    _ => {}
                }
                TypeInfo::Numeric {
                    lower: low.info,
                    upper: high.info,
                }
            }
            TypeSyntax::Named(name) => {
                return match self.bind_name(owner, name)? {
                    Target::Domain(id) => self.resolve_domain(id),
                    Target::Boolean => {
                        let id = self.types.len();
                        self.types.push(TypeInfo::Boolean);
                        self.symbols[owner].declaration.reference = Some(TypeReference {
                            declared: "INTERLIS.BOOLEAN".into(),
                            target: "INTERLIS.BOOLEAN".into(),
                            span: name.span.clone(),
                        });
                        Some(Effective {
                            ty: id,
                            mandatory: false,
                        })
                    }
                    Target::Structure => {
                        self.diagnostics.push(
                            "ILI-UNSUPPORTED",
                            "Structure-valued attributes are outside the supported subset",
                            Some(name.span.clone()),
                        );
                        None
                    }
                    Target::Other => {
                        self.diagnostics.push(
                            "ILI-TYPE-NOT-FOUND",
                            "Referenced declaration is not a supported domain",
                            Some(name.span.clone()),
                        );
                        None
                    }
                };
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
            .and_then(|&id| self.symbols[id].base)
            .map_or(self.symbols[current].name_span, |q| &q.span)
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
            if self.blocked_models.contains(&self.symbols[current].model) {
                break None;
            }
            if let Some(base) = self.symbols[current].base {
                match self.bind_name(current, base) {
                    Some(Target::Domain(next)) => {
                        current = next;
                        continue;
                    }
                    Some(Target::Boolean) => {
                        self.diagnostics.push(
                            "ILI-DERIVATION",
                            "Predefined BOOLEAN is final and cannot be derived",
                            Some(base.span.clone()),
                        );
                        break None;
                    }
                    Some(_) => {
                        self.diagnostics.push(
                            "ILI-TYPE-NOT-FOUND",
                            "A domain derivation must reference a domain",
                            Some(base.span.clone()),
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
        if let Some(mut value) = result {
            for &id in path.iter().rev() {
                value.mandatory |= self.symbols[id]
                    .declaration
                    .declared_mandatory
                    .unwrap_or(false);
                self.symbols[id].declaration.type_id = Some(value.ty);
                self.states[id] = State::Resolved(value);
            }
            match self.states[start] {
                State::Resolved(value) => Some(value),
                _ => Some(value),
            }
        } else {
            for id in path {
                self.states[id] = State::Failed;
            }
            None
        }
    }
    fn run(mut self) -> Option<ModelGraph> {
        if let Some(first) = self.documents.first() {
            for document in self.documents.iter().skip(1) {
                if document.language != first.language {
                    self.diagnostics.push_related(
                        "ILI-UNSUPPORTED",
                        "Mixed 2.3/2.4 source closure is not implemented",
                        Some(document.span.clone()),
                        vec![RelatedLocation {
                            span: first.span.clone(),
                            message: "Other source language".into(),
                        }],
                    );
                }
            }
        }
        if self.diagnostics.incomplete {
            return None;
        }
        self.collect_models();
        if self.diagnostics.limited {
            return None;
        }
        if !self.ambiguous_models {
            self.bind_imports();
        }
        if self.diagnostics.limited {
            return None;
        }
        for root in 0..self.roots.len() {
            let ast = self.roots[root].syntax;
            let model = self.roots[root].model;
            let symbol = self.add(Kind::Model, &ast.name, &ast.span, None, model, None)?;
            self.index_declarations(symbol, model, &ast.declarations);
            if self.diagnostics.limited {
                return None;
            }
        }
        if self.ambiguous_models {
            return None;
        }
        if self.diagnostics.limited {
            return None;
        }
        for id in 0..self.symbols.len() {
            if self.symbols[id].declaration.kind == Kind::Domain {
                self.resolve_domain(id);
            }
            if self.diagnostics.limited {
                return None;
            }
        }
        for id in 0..self.symbols.len() {
            if self.symbols[id].declaration.kind != Kind::Attribute
                || self.blocked_models.contains(&self.symbols[id].model)
            {
                continue;
            }
            let Some(syntax) = self.symbols[id].syntax else {
                continue;
            };
            if let Some(value) = self.concrete_type(id, syntax) {
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
    limits: &Limits,
    diagnostics: &mut Diagnostics,
) -> Option<ModelGraph> {
    Analyzer::new(documents, limits, diagnostics).run()
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
        let mut tokens = limits.max_tokens;
        let document = syntax::parse(&sources, 0, &limits, &mut tokens, &mut diagnostics).unwrap();
        let documents = [document];
        let mut analyzer = Analyzer::new(&documents, &limits, &mut diagnostics);
        analyzer.collect_models();
        analyzer.bind_imports();
        let ast = analyzer.roots[0].syntax;
        let model = analyzer.roots[0].model;
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
