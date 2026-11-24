//! Name checks that depend only on declarations and import visibility.
use super::{Analyzer, BoundName, Kind, LookupError, Name, QualifiedName, RelatedLocation, Span};
use crate::syntax::{
    ArgumentType, ClassValueKind, Declaration, FormatAttribute, FormatBase, FormatPart,
    LineFormSyntax, NumericSyntax, OidRule, OidSyntax, ParameterType, ReferenceSystemSyntax,
    RestrictedTarget, StructureTarget, TargetRef, TypeSyntax, UnitDefinition, ViewFormation,
    ViewOrigin,
};

const DOMAIN: &[Kind] = &[Kind::Domain];
const VALUE_TYPE: &[Kind] = &[Kind::Domain, Kind::Structure];
const CLASS_BASE: &[Kind] = &[Kind::Class, Kind::Structure];
const VIEWABLE: &[Kind] = &[Kind::Class, Kind::Structure, Kind::Association, Kind::View];
const ROLE_TARGET: &[Kind] = &[Kind::Class, Kind::Association];
const OBJECTS_TARGET: &[Kind] = &[Kind::Class, Kind::Association, Kind::View];

fn class_value_kinds(kind: ClassValueKind) -> &'static [Kind] {
    match kind {
        ClassValueKind::Class => VIEWABLE,
        ClassValueKind::Structure => CLASS_BASE,
    }
}

impl Analyzer<'_, '_> {
    pub(super) fn class_value_type(
        &mut self,
        owner: usize,
        kind: ClassValueKind,
        sources: &[QualifiedName],
    ) -> Option<crate::ClassValueType> {
        let kinds = class_value_kinds(kind);
        let mut restrictions = Vec::with_capacity(sources.len());
        for name in sources {
            match self.lookup_name(owner, &name.segments, &name.span, kinds) {
                Ok(target) => restrictions.push(self.bound_reference(owner, name, target)?),
                Err(LookupError::Limit) => return None,
                Err(LookupError::Deferred) => {
                    self.diagnostics.push(
                        "ILI-UNSUPPORTED",
                        "The class-value restriction requires its target's full declaration",
                        Some(name.span.clone()),
                    );
                    return None;
                }
                Err(_) => {
                    self.require_name(owner, name, kinds);
                    return None;
                }
            }
        }
        Some(crate::ClassValueType { kind, restrictions })
    }

    pub(super) fn check_direct_references(&mut self) {
        for owner in 0..self.symbols.len() {
            if self.diagnostics.limited {
                return;
            }
            let Some(ast) = self.symbols[owner].ast else {
                continue;
            };
            match ast {
                Declaration::Topic { options, .. } => {
                    self.optional_name(owner, options.base.as_ref(), &[Kind::Topic]);
                    for dependency in &options.dependencies {
                        self.require_name(owner, dependency, &[Kind::Topic]);
                    }
                    self.optional_name(owner, options.basket_oid.as_ref(), DOMAIN);
                    self.optional_name(owner, options.oid.as_ref(), DOMAIN);
                    for generic in &options.deferred_generics {
                        self.require_name(owner, generic, DOMAIN);
                    }
                }
                Declaration::Container {
                    base,
                    oid,
                    parameters,
                    ..
                } => {
                    let kinds = if self.symbols[owner].declaration.kind == Kind::Class {
                        CLASS_BASE
                    } else {
                        &[Kind::Structure]
                    };
                    self.optional_name(owner, base.as_ref(), kinds);
                    self.check_oid(owner, oid);
                    for parameter in parameters {
                        match &parameter.ty {
                            ParameterType::Attribute(t) => self.check_type(owner, t.ty.as_ref()),
                            ParameterType::Metaobject(class) => {
                                self.optional_name(owner, class.as_ref(), &[Kind::Class])
                            }
                        }
                    }
                }
                Declaration::Domain { base, ty, .. } => {
                    self.optional_name(owner, base.as_ref(), DOMAIN);
                    self.check_type(owner, ty.as_ref());
                }
                Declaration::Attribute { type_use, .. }
                | Declaration::RuntimeParameter { type_use, .. } => {
                    self.check_type(owner, type_use.ty.as_ref());
                }
                Declaration::Association(a) => {
                    self.optional_name(owner, a.base.as_ref(), &[Kind::Association]);
                    if let Some(view) = &a.derived_from {
                        self.require_name(owner, &view.target, VIEWABLE);
                    }
                    self.check_oid(owner, &a.oid);
                    for target in a.roles.iter().flat_map(|role| &role.targets) {
                        self.check_targets(owner, target, ROLE_TARGET);
                    }
                }
                Declaration::Unit(u) => {
                    self.optional_name(owner, u.base.as_ref(), &[Kind::Unit]);
                    match &u.definition {
                        Some(UnitDefinition::Derived { unit, .. }) => {
                            self.require_name(owner, unit, &[Kind::Unit])
                        }
                        Some(UnitDefinition::Composed { first, rest, .. }) => {
                            self.require_name(owner, first, &[Kind::Unit]);
                            for (_, unit) in rest {
                                self.require_name(owner, unit, &[Kind::Unit]);
                            }
                        }
                        None => {}
                    }
                }
                Declaration::LineForm(f) => {
                    self.require_parts(
                        owner,
                        std::slice::from_ref(&f.segment),
                        &f.segment.span,
                        &[Kind::Structure],
                    );
                }
                Declaration::Function(f) => {
                    for signature in f
                        .arguments
                        .iter()
                        .map(|arg| &arg.ty)
                        .chain(std::iter::once(&f.result))
                    {
                        match signature {
                            ArgumentType::Attribute(t) => self.check_type(owner, t.ty.as_ref()),
                            ArgumentType::Objects { target, .. } => {
                                self.check_targets(owner, target, OBJECTS_TARGET)
                            }
                            ArgumentType::EnumValue(_) | ArgumentType::EnumTreeValue(_) => {}
                        }
                    }
                }
                Declaration::Context(c) => {
                    for binding in &c.bindings {
                        self.require_name(owner, &binding.generic, DOMAIN);
                        for concrete in &binding.concrete {
                            self.require_name(owner, concrete, DOMAIN);
                        }
                    }
                }
                Declaration::View(v) => {
                    match &v.origin {
                        Some(ViewOrigin::Extends(base)) => {
                            self.require_name(owner, base, &[Kind::View])
                        }
                        Some(ViewOrigin::Formation(formation)) => match &formation.value {
                            ViewFormation::Projection(base)
                            | ViewFormation::Aggregation { base, .. } => {
                                self.require_name(owner, &base.target, VIEWABLE)
                            }
                            ViewFormation::Join { first, others } => {
                                self.require_name(owner, &first.target, VIEWABLE);
                                for base in others {
                                    self.require_name(owner, &base.base.target, VIEWABLE);
                                }
                            }
                            ViewFormation::Union(bases) => {
                                for base in bases {
                                    self.require_name(owner, &base.target, VIEWABLE);
                                }
                            }
                            ViewFormation::Inspection(inspection) => {
                                self.require_name(owner, &inspection.base.target, VIEWABLE)
                            }
                        },
                        None => {}
                    }
                    for extension in &v.base_extensions {
                        for view in &extension.extensions {
                            self.require_name(owner, &view.target, VIEWABLE);
                        }
                    }
                }
                Declaration::Graphic(g) => {
                    self.optional_name(owner, g.base.as_ref(), &[Kind::Graphic]);
                    self.optional_name(owner, g.viewable.as_ref(), VIEWABLE);
                    for rule in &g.rules {
                        self.optional_name(owner, rule.class.as_ref(), &[Kind::Class]);
                    }
                }
                Declaration::MetadataBasket(b) => {
                    self.optional_name(owner, b.base.as_ref(), &[Kind::MetadataBasket]);
                    self.require_name(owner, &b.topic, &[Kind::Topic]);
                }
                Declaration::Constraints(_) => {}
            }
        }
    }

    fn check_type(&mut self, owner: usize, ty: Option<&TypeSyntax>) {
        match ty {
            Some(TypeSyntax::Named(name)) => self.require_name(owner, name, VALUE_TYPE),
            Some(TypeSyntax::EnumTree { domain, .. }) => self.require_name(owner, domain, DOMAIN),
            Some(TypeSyntax::Numeric(n)) => self.check_numeric(owner, n),
            Some(TypeSyntax::Oid {
                carrier: OidSyntax::Numeric(n),
                ..
            }) => self.check_numeric(owner, n),
            Some(TypeSyntax::Coordinate(c)) => {
                for axis in &c.axes {
                    self.check_numeric(owner, axis);
                }
            }
            Some(TypeSyntax::Geometry(g)) => {
                self.optional_name(owner, g.vertex.as_ref(), DOMAIN);
                self.optional_name(owner, g.line_attributes.as_ref(), &[Kind::Structure]);
                for form in &g.line_forms {
                    if let LineFormSyntax::Named(name) = form {
                        self.require_name(owner, name, &[Kind::LineForm]);
                    }
                }
            }
            Some(TypeSyntax::Formatted { base, .. }) => match base {
                FormatBase::Structure { reference, format } => {
                    self.require_name(owner, reference, &[Kind::Structure]);
                    for part in &format.parts {
                        if let FormatPart::Attribute {
                            attribute: FormatAttribute::Formatted { domain, .. },
                            ..
                        } = part
                        {
                            self.require_name(owner, domain, DOMAIN);
                        }
                    }
                }
                FormatBase::Domain(domain) => self.require_name(owner, domain, DOMAIN),
                FormatBase::Inherited => {}
            },
            Some(TypeSyntax::ClassValue {
                kind, restrictions, ..
            }) => {
                for name in restrictions {
                    self.require_name(owner, name, class_value_kinds(*kind));
                }
            }
            Some(TypeSyntax::Reference { target, .. }) => {
                self.check_targets(owner, target, ROLE_TARGET)
            }
            Some(TypeSyntax::Collection { element, .. }) => self.check_type(owner, Some(element)),
            Some(TypeSyntax::RestrictedStructure {
                target,
                restrictions,
                ..
            }) => {
                if let StructureTarget::Named(name) = target {
                    self.require_name(owner, name, &[Kind::Structure]);
                }
                for name in restrictions {
                    self.require_name(owner, name, &[Kind::Structure]);
                }
            }
            Some(TypeSyntax::AttributePathValue { restrictions, .. }) => {
                for t in restrictions {
                    self.check_type(owner, t.ty.as_ref());
                }
            }
            _ => {}
        }
    }

    fn check_numeric(&mut self, owner: usize, numeric: &NumericSyntax) {
        self.optional_name(owner, numeric.unit.as_ref(), &[Kind::Unit]);
        if let Some(ReferenceSystemSyntax::Domain { reference, .. }) = &numeric.reference_system {
            self.require_name(owner, reference, DOMAIN);
        }
    }

    fn check_targets(&mut self, owner: usize, target: &RestrictedTarget, kinds: &[Kind]) {
        if let TargetRef::Named(name) = &target.target {
            self.require_name(owner, name, kinds);
        }
        for restriction in &target.restrictions {
            self.require_name(owner, restriction, ROLE_TARGET);
        }
    }

    fn check_oid(&mut self, owner: usize, oid: &OidRule) {
        if let OidRule::Domain(domain) = oid {
            self.require_name(owner, domain, DOMAIN);
        }
    }

    fn optional_name(&mut self, owner: usize, name: Option<&QualifiedName>, kinds: &[Kind]) {
        if let Some(name) = name {
            self.require_name(owner, name, kinds);
        }
    }

    fn require_name(&mut self, owner: usize, name: &QualifiedName, kinds: &[Kind]) {
        self.require_parts(owner, &name.segments, &name.span, kinds);
    }

    fn require_parts(&mut self, owner: usize, parts: &[Name], span: &Span, kinds: &[Kind]) {
        if self.diagnostics.limited {
            return;
        }
        let type_reference = kinds == VALUE_TYPE || kinds == DOMAIN;
        let lookup = self.lookup_name(owner, parts, span, kinds);
        match lookup {
            Ok(BoundName::Declared(id)) => {
                if kinds == VALUE_TYPE
                    && self.symbols[id].declaration.kind == Kind::Domain
                    && self.symbols[owner].declaration.kind != Kind::Domain
                {
                    self.check_domain_order(owner, id, span);
                }
            }
            Ok(BoundName::Predefined(..)) | Err(LookupError::Deferred | LookupError::Limit) => {}
            Err(LookupError::Ambiguous(ids)) => self.diagnostics.push_related(
                if type_reference {
                    "ILI-TYPE-AMBIGUOUS"
                } else {
                    "ILI-NAME-AMBIGUOUS"
                },
                "Unqualified name is provided by more than one imported model",
                Some(span.clone()),
                ids.into_iter()
                    .map(|id| RelatedLocation {
                        span: self.symbols[id].name_span.clone(),
                        message: "Candidate declaration".into(),
                    })
                    .collect(),
            ),
            Err(error) => self.diagnostics.push(
                if type_reference {
                    "ILI-TYPE-NOT-FOUND"
                } else {
                    "ILI-NAME-NOT-FOUND"
                },
                match error {
                    LookupError::NotImported => {
                        "Qualified name must use the current or an explicitly imported model name"
                    }
                    _ => "No visible declaration of the expected kind matches this name",
                },
                Some(span.clone()),
            ),
        }
    }
}
