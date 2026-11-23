use super::{Analyzer, Kind, Namespace, QualifiedName, namespace};
use crate::LanguageVersion;

pub(super) enum BoundName {
    Declared(usize),
    Predefined(&'static str, Kind),
}

pub(super) enum LookupError {
    Missing,
    NotImported,
    Ambiguous([usize; 2]),
    Limit,
}

// Names and declaration kinds from both editions of Appendix A. They are
// classified here even when the compiler cannot yet check their types.
const PREDEFINED: &[(Kind, &[&str])] = &[
    (
        Kind::Domain,
        &[
            "URI",
            "NAME",
            "INTERLIS_1_DATE",
            "BOOLEAN",
            "HALIGNMENT",
            "VALIGNMENT",
            "ANYOID",
            "I32OID",
            "STANDARDOID",
            "UUIDOID",
            "LineCoord",
            "GregorianYear",
            "XMLTime",
            "XMLDate",
            "XMLDateTime",
        ],
    ),
    (
        Kind::Structure,
        &[
            "AXIS",
            "TimeOfDay",
            "UTC",
            "GregorianDate",
            "GregorianDateTime",
            "LineSegment",
            "StartSegment",
            "StraightSegment",
            "ArcSegment",
            "SurfaceEdge",
            "SurfaceBoundary",
            "LineGeometry",
        ],
    ),
    (
        Kind::Class,
        &[
            "METAOBJECT",
            "METAOBJECT_TRANSLATION",
            "REFSYSTEM",
            "COORDSYSTEM",
            "SCALSYSTEM",
            "SIGN",
            "TIMESYSTEMS.CALENDAR",
            "TIMESYSTEMS.TIMEOFDAYSYS",
        ],
    ),
    (
        Kind::Unit,
        &[
            "ANYUNIT",
            "DIMENSIONLESS",
            "LENGTH",
            "MASS",
            "TIME",
            "ELECTRIC_CURRENT",
            "TEMPERATURE",
            "AMOUNT_OF_MATTER",
            "ANGLE",
            "SOLID_ANGLE",
            "LUMINOUS_INTENSITY",
            "MONEY",
            "m",
            "kg",
            "s",
            "A",
            "K",
            "mol",
            "rad",
            "sr",
            "cd",
            "min",
            "h",
            "d",
            "M",
            "Y",
        ],
    ),
    (
        Kind::Function,
        &[
            "myClass",
            "isSubClass",
            "isOfClass",
            "elementCount",
            "objectCount",
            "len",
            "lenM",
            "trim",
            "trimM",
            "isEnumSubVal",
            "inEnumRange",
            "convertUnit",
            "areAreas",
        ],
    ),
    (Kind::LineForm, &["STRAIGHTS", "ARCS"]),
    (Kind::Topic, &["TIMESYSTEMS"]),
    (Kind::MetadataBasket, &["BaseTimeSystems"]),
    (
        Kind::MetaObject,
        &["BaseTimeSystems.GregorianCalendar", "BaseTimeSystems.UTC"],
    ),
];

const PREDEFINED_24: &[(Kind, &[&str])] = &[
    (Kind::Domain, &["NOOID"]),
    (
        Kind::Function,
        &[
            "length",
            "multilength",
            "surface",
            "multisurface",
            "areAreas2",
            "areAreas3",
        ],
    ),
];

fn predefined(language: LanguageVersion, spelling: &str, kinds: &[Kind]) -> Option<BoundName> {
    let edition = match language {
        LanguageVersion::V23 => &[][..],
        LanguageVersion::V24 => PREDEFINED_24,
    };
    PREDEFINED.iter().chain(edition).find_map(|&(kind, names)| {
        if !kinds.contains(&kind) {
            return None;
        }
        names
            .iter()
            .find(|&&name| name == spelling)
            .map(|&name| BoundName::Predefined(name, kind))
    })
}

impl Analyzer<'_, '_> {
    fn scoped_name(&self, scope: usize, name: &str, kinds: &[Kind]) -> Option<usize> {
        for kind in kinds {
            if let Some(&id) = self
                .scope_names
                .get(&(scope, namespace(*kind), name.to_owned()))
                && self.symbols[id].declaration.kind == *kind
            {
                return Some(id);
            }
        }
        None
    }

    pub(super) fn lookup_name(
        &mut self,
        owner: usize,
        name: &QualifiedName,
        kinds: &[Kind],
    ) -> Result<BoundName, LookupError> {
        let charge = name.segments.iter().map(|s| s.value.len() + 1).sum();
        if !self.step(charge, &name.span) {
            return Err(LookupError::Limit);
        }
        let model = self.symbols[owner].model;
        let language = self.models[model].language;
        let first = &name.segments[0].value;
        if name.segments.len() > 1 {
            if first == "INTERLIS" {
                let spelling = name.segments[1..]
                    .iter()
                    .map(|s| s.value.as_str())
                    .collect::<Vec<_>>()
                    .join(".");
                return predefined(language, &spelling, kinds).ok_or(LookupError::Missing);
            }
            if first != &self.models[model].name
                && !self.bound_imports[model].contains_key(first.as_str())
            {
                return Err(LookupError::NotImported);
            }
            let mut scope = self.names[first];
            let last = name.segments.len() - 1;
            for part in &name.segments[1..last] {
                scope = *self
                    .scope_names
                    .get(&(scope, Namespace::Type, part.value.clone()))
                    .ok_or(LookupError::Missing)?;
            }
            return self
                .scoped_name(scope, &name.segments[last].value, kinds)
                .map(BoundName::Declared)
                .ok_or(LookupError::Missing);
        }

        let mut scope = self.symbols[owner].declaration.parent;
        let mut later_domain = None;
        while let Some(parent) = scope {
            if !self.step(first.len() + 1, &name.span) {
                return Err(LookupError::Limit);
            }
            if let Some(id) = self.scoped_name(parent, first, kinds) {
                if self.symbols[id].declaration.kind == Kind::Domain
                    && self.domain_follows_reference(owner, id, &name.span)
                {
                    later_domain.get_or_insert(id);
                } else {
                    return Ok(BoundName::Declared(id));
                }
            }
            scope = self.symbols[parent].declaration.parent;
        }

        if !self.step(
            self.bound_imports[model]
                .len()
                .saturating_mul(first.len() + 1),
            &name.span,
        ) {
            return Err(LookupError::Limit);
        }
        let mut imported = None;
        for (&import, &unqualified) in &self.bound_imports[model] {
            if unqualified
                && import != "INTERLIS"
                && let Some(id) = self.scoped_name(self.names[import], first, kinds)
            {
                if let Some(previous) = imported {
                    return Err(LookupError::Ambiguous([previous, id]));
                }
                imported = Some(id);
            }
        }
        if let Some(id) = imported {
            return Ok(BoundName::Declared(id));
        }
        if self.bound_imports[model].get("INTERLIS") == Some(&true)
            && let Some(target) = predefined(language, first, kinds)
        {
            return Ok(target);
        }
        later_domain
            .map(BoundName::Declared)
            .ok_or(LookupError::Missing)
    }
}
