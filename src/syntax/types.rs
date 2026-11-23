//! Concrete type, path, unit and line-form syntax. No reference is resolved here.
use super::*;
pub(crate) use crate::model::{AlignmentKind, BlackboxKind, TemporalKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextKind {
    Text,
    MText,
    Name,
    Uri,
}
#[derive(Debug)]
pub(crate) struct TextSyntax {
    pub kind: TextKind,
    pub max_length: Option<Located<String>>,
    pub span: Span,
}
#[derive(Debug)]
pub(crate) struct NumericBounds {
    pub lower: Located<String>,
    pub upper: Located<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Clockwise,
    Counterclockwise,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum ReferenceSystemSyntax {
    Metaobject {
        reference: QualifiedName,
        axis: Option<Located<String>>,
        span: Span,
    },
    Domain {
        reference: QualifiedName,
        axis: Option<Located<String>>,
        span: Span,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct NumericSyntax {
    pub bounds: Option<NumericBounds>,
    pub circular: Option<Span>,
    pub unit: Option<QualifiedName>,
    pub direction: Option<Located<Direction>>,
    pub reference_system: Option<ReferenceSystemSyntax>,
    pub span: Span,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnumerationOrder {
    Ordered,
    Circular,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct EnumerationSyntax {
    pub elements: Vec<EnumerationElement>,
    pub final_marker: Option<Span>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct EnumerationElement {
    pub path: Vec<Name>,
    pub children: Option<EnumerationSyntax>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FormatAttribute {
    Numeric {
        name: Name,
        width: Option<Located<String>>,
    },
    Formatted {
        name: Name,
        domain: QualifiedName,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FormatPart {
    Literal(Located<String>),
    Attribute {
        attribute: FormatAttribute,
        span: Span,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct FormatDefinition {
    pub inheritance: Option<Span>,
    pub parts: Vec<FormatPart>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum FormatBase {
    Structure {
        reference: QualifiedName,
        format: FormatDefinition,
    },
    Domain(QualifiedName),
    Inherited,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct StringRange {
    pub lower: Located<String>,
    pub upper: Located<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CoordinateKind {
    Single,
    Multi,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct RotationSyntax {
    pub zero: Located<String>,
    pub quarter_turn: Located<String>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct CoordinateSyntax {
    pub kind: CoordinateKind,
    pub axes: Vec<NumericSyntax>,
    pub rotation: Option<RotationSyntax>,
    pub crs: Option<Located<String>>,
    pub span: Span,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeometryKind {
    Polyline,
    Surface,
    Area,
    MultiPolyline,
    MultiSurface,
    MultiArea,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum LineFormSyntax {
    Straights(Span),
    Arcs(Span),
    Named(QualifiedName),
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct OverlapSyntax {
    pub tolerance: Option<Located<String>>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct GeometrySyntax {
    pub kind: GeometryKind,
    pub directed: Option<Span>,
    pub line_forms: Vec<LineFormSyntax>,
    pub vertex: Option<QualifiedName>,
    pub overlaps: Option<OverlapSyntax>,
    pub line_attributes: Option<QualifiedName>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum OidSyntax {
    Any(Span),
    Numeric(Box<NumericSyntax>),
    Text(TextSyntax),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClassValueKind {
    Class,
    Structure,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum PathSelector {
    First(Span),
    Last(Span),
    Index(Located<String>),
    Association(Name),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpecialPath {
    This,
    ThisArea,
    ThatArea,
    Parent,
    Aggregates,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum PathElement {
    Special(Located<SpecialPath>),
    Name {
        name: Name,
        selector: Option<PathSelector>,
        span: Span,
    },
    AssociationAccess {
        name: Name,
        span: Span,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct ObjectPath {
    pub elements: Vec<PathElement>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum AttributePathOf {
    Path(ObjectPath),
    Argument(Name),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CollectionKind {
    Bag,
    List,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum StructureTarget {
    Named(QualifiedName),
    Any(Span),
}
#[derive(Debug)]
#[allow(dead_code)] // Concrete AST families retained independently from partial semantic support.
pub(crate) enum TypeSyntax {
    Text(TextSyntax),
    Boolean {
        span: Span,
    },
    Alignment {
        kind: AlignmentKind,
        span: Span,
    },
    Temporal {
        kind: TemporalKind,
        span: Span,
    },
    Numeric(NumericSyntax),
    Named(QualifiedName),
    Enumeration {
        enumeration: EnumerationSyntax,
        order: Option<Located<EnumerationOrder>>,
        span: Span,
    },
    EnumTree {
        domain: QualifiedName,
        span: Span,
    },
    Formatted {
        base: FormatBase,
        range: Option<StringRange>,
        span: Span,
    },
    Coordinate(CoordinateSyntax),
    Geometry(GeometrySyntax),
    Oid {
        carrier: OidSyntax,
        span: Span,
    },
    Blackbox {
        kind: BlackboxKind,
        span: Span,
    },
    ClassValue {
        kind: ClassValueKind,
        restrictions: Vec<QualifiedName>,
        span: Span,
    },
    AttributePathValue {
        of: Option<AttributePathOf>,
        restrictions: Vec<TypeUseSyntax>,
        span: Span,
    },
    Reference {
        properties: Vec<Located<Property>>,
        target: RestrictedTarget,
        span: Span,
    },
    Collection {
        kind: CollectionKind,
        cardinality: Option<Cardinality>,
        element: Box<TypeSyntax>,
        span: Span,
    },
    RestrictedStructure {
        target: StructureTarget,
        restrictions: Vec<QualifiedName>,
        span: Span,
    },
}
impl TypeSyntax {
    pub(crate) fn span(&self) -> &Span {
        match self {
            Self::Text(t) => &t.span,
            Self::Numeric(t) => &t.span,
            Self::Named(n) => &n.span,
            Self::Coordinate(t) => &t.span,
            Self::Geometry(t) => &t.span,
            Self::Boolean { span }
            | Self::Alignment { span, .. }
            | Self::Temporal { span, .. }
            | Self::Enumeration { span, .. }
            | Self::EnumTree { span, .. }
            | Self::Formatted { span, .. }
            | Self::Oid { span, .. }
            | Self::Blackbox { span, .. }
            | Self::ClassValue { span, .. }
            | Self::AttributePathValue { span, .. }
            | Self::Reference { span, .. }
            | Self::Collection { span, .. }
            | Self::RestrictedStructure { span, .. } => span,
        }
    }
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum DecimalConstant {
    Number(Located<String>),
    Pi(Span),
    LnBase(Span),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UnitOperator {
    Multiply,
    Divide,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UnitScale {
    Factors {
        first: DecimalConstant,
        rest: Vec<(Located<UnitOperator>, DecimalConstant)>,
    },
    Function(Located<String>),
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UnitDefinition {
    Derived {
        scale: Option<UnitScale>,
        unit: QualifiedName,
        span: Span,
    },
    Composed {
        first: QualifiedName,
        rest: Vec<(Located<UnitOperator>, QualifiedName)>,
        span: Span,
    },
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum UnitDisplay {
    Abstract(Span),
    ShortName(Name),
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct UnitDecl {
    pub name: Name,
    pub display: Option<UnitDisplay>,
    pub base: Option<QualifiedName>,
    pub definition: Option<UnitDefinition>,
    pub span: Span,
}
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct LineFormDecl {
    pub name: Name,
    pub segment: Name,
    pub span: Span,
}

impl Parser<'_> {
    pub(super) fn type_use(&mut self) -> Parsed<TypeUseSyntax> {
        self.step()?;
        let start = self.start();
        let mandatory = self.take("MANDATORY");
        let ty = if mandatory && matches!(self.text(), ";" | ":=" | ")") {
            None
        } else if !mandatory && matches!(self.text(), "BAG" | "LIST") {
            Some(self.collection_type()?)
        } else {
            Some(self.attribute_type()?)
        };
        self.node()?;
        Ok(TypeUseSyntax {
            mandatory,
            ty,
            span: self.span(start, self.end()),
        })
    }
    fn attribute_type(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        let start = self.start();
        if self.take("REFERENCE") {
            self.expect("TO")?;
            let properties = self.properties(&[Property::External])?;
            let target = self.restricted_target()?;
            self.node()?;
            return Ok(TypeSyntax::Reference {
                properties,
                target,
                span: self.span(start, self.end()),
            });
        }
        if self.is("ANYSTRUCTURE") {
            return self.restricted_structure();
        }
        if self.unreserved_name() || self.is("INTERLIS") {
            let name = self.qualified_name()?;
            if self.is("RESTRICTION") {
                let restrictions = self.structure_restrictions()?;
                self.node()?;
                return Ok(TypeSyntax::RestrictedStructure {
                    target: StructureTarget::Named(name),
                    restrictions,
                    span: self.span(start, self.end()),
                });
            }
            self.node()?;
            return Ok(TypeSyntax::Named(name));
        }
        self.type_syntax()
    }
    fn restricted_structure(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        let start = self.start();
        let target = if let Some(span) = self.marker("ANYSTRUCTURE") {
            StructureTarget::Any(span)
        } else {
            StructureTarget::Named(self.reference(3)?)
        };
        let restrictions = self.structure_restrictions()?;
        self.node()?;
        Ok(TypeSyntax::RestrictedStructure {
            target,
            restrictions,
            span: self.span(start, self.end()),
        })
    }
    fn structure_restrictions(&mut self) -> Parsed<Vec<QualifiedName>> {
        let mut refs = vec![];
        if self.take("RESTRICTION") {
            self.expect("(")?;
            refs.push(self.reference(3)?);
            while self.take(";") {
                refs.push(self.reference(3)?);
            }
            self.expect(")")?;
        }
        Ok(refs)
    }
    fn collection_type(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        let start = self.start();
        let kind = if self.take("BAG") {
            CollectionKind::Bag
        } else {
            self.expect("LIST")?;
            CollectionKind::List
        };
        let cardinality = if self.is("{") {
            Some(self.cardinality()?)
        } else {
            None
        };
        self.expect("OF")?;
        self.enter()?;
        let element = if self.language == Language::V23 {
            self.restricted_structure()
        } else {
            self.attribute_type()
        };
        self.depth -= 1;
        let element = element?;
        self.node()?;
        Ok(TypeSyntax::Collection {
            kind,
            cardinality,
            element: Box::new(element),
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn domain_section(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        let mut out = vec![];
        while self.unreserved_name() && !self.diagnostics.limited {
            self.step()?;
            let start = self.start();
            let name = self.name(false)?;
            let parsed: Parsed<Declaration> = (|| {
                let allowed = if self.language == Language::V24 {
                    &[Property::Abstract, Property::Generic, Property::Final][..]
                } else {
                    &[Property::Abstract, Property::Final][..]
                };
                let properties = self.properties(allowed)?;
                let base = if self.take("EXTENDS") {
                    Some(self.qualified_name()?)
                } else {
                    None
                };
                self.expect("=")?;
                let mandatory = self.take("MANDATORY");
                let ty = if mandatory && matches!(self.text(), ";" | "CONSTRAINTS") {
                    None
                } else {
                    Some(self.type_syntax()?)
                };
                let constraints = self.domain_constraints()?;
                self.expect(";")?;
                self.node()?;
                Ok(Declaration::Domain {
                    name,
                    properties,
                    base,
                    ty,
                    mandatory,
                    constraints,
                    span: self.span(start, self.end()),
                })
            })();
            match parsed {
                Ok(item) => out.push(item),
                Err(issue) => {
                    self.report(issue);
                    self.recover(context, true);
                }
            }
        }
        Ok(out)
    }
    pub(super) fn type_syntax(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        let start = self.start();
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::Invalid)
        ) {
            return Err(Self::lexical());
        }
        if self.language == Language::V23 && self.is("DATE") {
            return Err(Self::expected("DATE type requires INTERLIS 2.4"));
        }
        match self.text() {
            "TEXT" | "MTEXT" | "NAME" | "URI" => return Ok(TypeSyntax::Text(self.text_type()?)),
            "NUMERIC" | "-" | "+" => return Ok(TypeSyntax::Numeric(self.numeric_type()?)),
            "(" => return self.enumeration_type(),
            "FORMAT" => return self.formatted_type(),
            "COORD" => return Ok(TypeSyntax::Coordinate(self.coordinate_type()?)),
            "MULTICOORD" if self.language == Language::V24 => {
                return Ok(TypeSyntax::Coordinate(self.coordinate_type()?));
            }
            "DIRECTED" | "POLYLINE" | "SURFACE" | "AREA" => {
                return Ok(TypeSyntax::Geometry(self.geometry_type()?));
            }
            "MULTIPOLYLINE" | "MULTISURFACE" | "MULTIAREA" if self.language == Language::V24 => {
                return Ok(TypeSyntax::Geometry(self.geometry_type()?));
            }
            "CLASS" | "STRUCTURE" => return self.class_value_type(),
            "ATTRIBUTE" => return self.attribute_path_type(),
            _ => {}
        }
        if self
            .tokens
            .get(self.at)
            .is_some_and(|t| t.kind == Kind::Number)
        {
            return Ok(TypeSyntax::Numeric(self.numeric_type()?));
        }
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::String(_))
        ) {
            return self.formatted_type();
        }
        if self.take("BOOLEAN") {
            self.node()?;
            return Ok(TypeSyntax::Boolean {
                span: self.span(start, self.end()),
            });
        }
        if self.is("HALIGNMENT") || self.is("VALIGNMENT") {
            let kind = if self.take("HALIGNMENT") {
                AlignmentKind::Horizontal
            } else {
                self.expect("VALIGNMENT")?;
                AlignmentKind::Vertical
            };
            self.node()?;
            return Ok(TypeSyntax::Alignment {
                kind,
                span: self.span(start, self.end()),
            });
        }
        if self.language == Language::V24
            && matches!(self.text(), "DATE" | "TIMEOFDAY" | "DATETIME")
        {
            let kind = if self.take("DATE") {
                TemporalKind::Date
            } else if self.take("TIMEOFDAY") {
                TemporalKind::TimeOfDay
            } else {
                self.expect("DATETIME")?;
                TemporalKind::DateTime
            };
            self.node()?;
            return Ok(TypeSyntax::Temporal {
                kind,
                span: self.span(start, self.end()),
            });
        }
        if self.take("ALL") {
            self.expect("OF")?;
            let domain = self.qualified_name()?;
            self.node()?;
            return Ok(TypeSyntax::EnumTree {
                domain,
                span: self.span(start, self.end()),
            });
        }
        if self.take("OID") {
            let carrier = if let Some(span) = self.marker("ANY") {
                OidSyntax::Any(span)
            } else if matches!(self.text(), "TEXT" | "MTEXT" | "NAME" | "URI") {
                OidSyntax::Text(self.text_type()?)
            } else {
                OidSyntax::Numeric(Box::new(self.numeric_type()?))
            };
            self.node()?;
            return Ok(TypeSyntax::Oid {
                carrier,
                span: self.span(start, self.end()),
            });
        }
        if self.take("BLACKBOX") {
            let kind = if self.take("XML") {
                BlackboxKind::Xml
            } else {
                self.expect("BINARY")?;
                BlackboxKind::Binary
            };
            self.node()?;
            return Ok(TypeSyntax::Blackbox {
                kind,
                span: self.span(start, self.end()),
            });
        }
        Err(Self::expected(
            "Expected a concrete type; direct domain aliases are invalid",
        ))
    }
    fn text_type(&mut self) -> Parsed<TextSyntax> {
        self.step()?;
        let start = self.start();
        let kind = if self.take("TEXT") {
            TextKind::Text
        } else if self.take("MTEXT") {
            TextKind::MText
        } else if self.take("NAME") {
            TextKind::Name
        } else {
            self.expect("URI")?;
            TextKind::Uri
        };
        let max_length = if matches!(kind, TextKind::Text | TextKind::MText) && self.take("*") {
            Some(self.unsigned_integer()?)
        } else {
            None
        };
        self.node()?;
        Ok(TextSyntax {
            kind,
            max_length,
            span: self.span(start, self.end()),
        })
    }
    fn numeric_type(&mut self) -> Parsed<NumericSyntax> {
        self.step()?;
        let start = self.start();
        let bounds = if self.take("NUMERIC") {
            None
        } else {
            let lower = self.number()?;
            self.expect("..")?;
            let upper = self.number()?;
            Some(NumericBounds { lower, upper })
        };
        let circular = self.marker("CIRCULAR");
        let unit = if self.take("[") {
            let n = self.reference(3)?;
            self.expect("]")?;
            Some(n)
        } else {
            None
        };
        let direction = if self.is("CLOCKWISE") || self.is("COUNTERCLOCKWISE") {
            self.node()?;
            let at = self.start();
            let value = if self.take("CLOCKWISE") {
                Direction::Clockwise
            } else {
                self.expect("COUNTERCLOCKWISE")?;
                Direction::Counterclockwise
            };
            Some(Located {
                value,
                span: self.span(at, self.end()),
            })
        } else {
            None
        };
        let reference_system = if direction.is_none() && (self.is("{") || self.is("<")) {
            Some(self.reference_system()?)
        } else {
            None
        };
        self.node()?;
        Ok(NumericSyntax {
            bounds,
            circular,
            unit,
            direction,
            reference_system,
            span: self.span(start, self.end()),
        })
    }
    fn reference_system(&mut self) -> Parsed<ReferenceSystemSyntax> {
        self.step()?;
        let start = self.start();
        let meta = self.take("{");
        if !meta {
            self.expect("<")?;
        }
        let reference = if meta {
            self.qualified_name_in(ReferenceContext::Ordinary, 4)?
        } else {
            self.qualified_name()?
        };
        let axis = if self.take("[") {
            let n = self.unsigned_integer()?;
            self.expect("]")?;
            Some(n)
        } else {
            None
        };
        self.expect(if meta { "}" } else { ">" })?;
        self.node()?;
        Ok(if meta {
            ReferenceSystemSyntax::Metaobject {
                reference,
                axis,
                span: self.span(start, self.end()),
            }
        } else {
            ReferenceSystemSyntax::Domain {
                reference,
                axis,
                span: self.span(start, self.end()),
            }
        })
    }
    fn enumeration_type(&mut self) -> Parsed<TypeSyntax> {
        let start = self.start();
        let enumeration = self.enumeration()?;
        let order = if self.is("ORDERED") || self.is("CIRCULAR") {
            let at = self.start();
            self.node()?;
            let value = if self.take("ORDERED") {
                EnumerationOrder::Ordered
            } else {
                self.expect("CIRCULAR")?;
                EnumerationOrder::Circular
            };
            Some(Located {
                value,
                span: self.span(at, self.end()),
            })
        } else {
            None
        };
        self.node()?;
        Ok(TypeSyntax::Enumeration {
            enumeration,
            order,
            span: self.span(start, self.end()),
        })
    }
    fn enumeration(&mut self) -> Parsed<EnumerationSyntax> {
        self.enter()?;
        let result = (|| {
            let start = self.start();
            self.expect("(")?;
            let mut elements = vec![];
            let final_marker = if let Some(span) = self.marker("FINAL") {
                Some(span)
            } else {
                loop {
                    let at = self.start();
                    let mut path = vec![self.name(false)?];
                    while self.take(".") {
                        path.push(self.name(false)?);
                    }
                    let children = if self.is("(") {
                        Some(self.enumeration()?)
                    } else {
                        None
                    };
                    self.node()?;
                    elements.push(EnumerationElement {
                        path,
                        children,
                        span: self.span(at, self.end()),
                    });
                    if !self.take(",") {
                        break;
                    }
                }
                if self.take(":") {
                    let at = self.start();
                    self.expect("FINAL")?;
                    Some(self.span(at, self.end()))
                } else {
                    None
                }
            };
            self.expect(")")?;
            self.node()?;
            Ok(EnumerationSyntax {
                elements,
                final_marker,
                span: self.span(start, self.end()),
            })
        })();
        self.depth -= 1;
        result
    }
    fn string_range(&mut self) -> Parsed<StringRange> {
        let lower = self.quoted()?;
        self.expect("..")?;
        let upper = self.quoted()?;
        self.node()?;
        Ok(StringRange { lower, upper })
    }
    fn formatted_type(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        let start = self.start();
        let (base, range) = if self.take("FORMAT") {
            if self.take("BASED") {
                self.expect("ON")?;
                let reference = self.reference(3)?;
                let format = self.format_definition()?;
                let range = if matches!(
                    self.tokens.get(self.at).map(|t| &t.kind),
                    Some(Kind::String(_))
                ) {
                    Some(self.string_range()?)
                } else {
                    None
                };
                (FormatBase::Structure { reference, format }, range)
            } else {
                let reference = self.qualified_name()?;
                (FormatBase::Domain(reference), Some(self.string_range()?))
            }
        } else {
            (FormatBase::Inherited, Some(self.string_range()?))
        };
        self.node()?;
        Ok(TypeSyntax::Formatted {
            base,
            range,
            span: self.span(start, self.end()),
        })
    }
    fn format_literal(&mut self) -> Parsed<FormatPart> {
        let value = self.quoted()?;
        if value
            .value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_digit)
        {
            return Err(Issue {
                kind: IssueKind::Syntax,
                primary: Some(value.span),
                message: "Format separator must not begin with a digit",
            });
        }
        self.node()?;
        Ok(FormatPart::Literal(value))
    }
    fn format_definition(&mut self) -> Parsed<FormatDefinition> {
        self.step()?;
        let start = self.start();
        self.expect("(")?;
        let inheritance = self.marker("INHERITANCE");
        let mut parts = vec![];
        if matches!(
            self.tokens.get(self.at).map(|t| &t.kind),
            Some(Kind::String(_))
        ) {
            parts.push(self.format_literal()?);
        }
        loop {
            self.step()?;
            let at = self.start();
            let name = self.name(false)?;
            let attribute = if self.take("/") {
                if self
                    .tokens
                    .get(self.at)
                    .is_some_and(|t| t.kind == Kind::Number)
                {
                    FormatAttribute::Numeric {
                        name,
                        width: Some(self.unsigned_integer()?),
                    }
                } else {
                    FormatAttribute::Formatted {
                        name,
                        domain: self.qualified_name()?,
                    }
                }
            } else {
                FormatAttribute::Numeric { name, width: None }
            };
            self.node()?;
            parts.push(FormatPart::Attribute {
                attribute,
                span: self.span(at, self.end()),
            });
            if !matches!(
                self.tokens.get(self.at).map(|t| &t.kind),
                Some(Kind::String(_))
            ) {
                break;
            }
            parts.push(self.format_literal()?);
            if self.is(")") {
                break;
            }
        }
        self.expect(")")?;
        self.node()?;
        Ok(FormatDefinition {
            inheritance,
            parts,
            span: self.span(start, self.end()),
        })
    }
    fn rotation(&mut self) -> Parsed<RotationSyntax> {
        let start = self.start();
        self.expect("ROTATION")?;
        let zero = self.unsigned_integer()?;
        self.expect("->")?;
        let quarter_turn = self.unsigned_integer()?;
        self.node()?;
        Ok(RotationSyntax {
            zero,
            quarter_turn,
            span: self.span(start, self.end()),
        })
    }
    fn coordinate_type(&mut self) -> Parsed<CoordinateSyntax> {
        self.step()?;
        let start = self.start();
        let kind = if self.take("COORD") {
            CoordinateKind::Single
        } else {
            self.expect("MULTICOORD")?;
            CoordinateKind::Multi
        };
        let mut axes = vec![self.numeric_type()?];
        let mut rotation = None;
        let mut crs = None;
        if self.take(",") {
            axes.push(self.numeric_type()?);
            if self.take(",") {
                if self.is("ROTATION") {
                    rotation = Some(self.rotation()?);
                } else {
                    axes.push(self.numeric_type()?);
                    if self.take(",") {
                        rotation = Some(self.rotation()?);
                    }
                }
            }
            if self.language == Language::V24 && self.take("REFSYS") {
                crs = Some(self.quoted()?);
            }
        }
        self.node()?;
        Ok(CoordinateSyntax {
            kind,
            axes,
            rotation,
            crs,
            span: self.span(start, self.end()),
        })
    }
    fn geometry_type(&mut self) -> Parsed<GeometrySyntax> {
        self.step()?;
        let start = self.start();
        let directed = self.marker("DIRECTED");
        let kind = match self.text() {
            "POLYLINE" => GeometryKind::Polyline,
            "SURFACE" => GeometryKind::Surface,
            "AREA" => GeometryKind::Area,
            "MULTIPOLYLINE" if self.language == Language::V24 => GeometryKind::MultiPolyline,
            "MULTISURFACE" if self.language == Language::V24 => GeometryKind::MultiSurface,
            "MULTIAREA" if self.language == Language::V24 => GeometryKind::MultiArea,
            _ => return Err(Self::expected("Expected a geometry type for this edition")),
        };
        if directed.is_some()
            && !matches!(kind, GeometryKind::Polyline | GeometryKind::MultiPolyline)
        {
            return Err(Self::expected(
                "DIRECTED requires POLYLINE or MULTIPOLYLINE",
            ));
        }
        self.step()?;
        self.at += 1;
        let mut line_forms = vec![];
        if self.take("WITH") {
            self.expect("(")?;
            loop {
                let form = if let Some(span) = self.marker("STRAIGHTS") {
                    LineFormSyntax::Straights(span)
                } else if let Some(span) = self.marker("ARCS") {
                    LineFormSyntax::Arcs(span)
                } else {
                    LineFormSyntax::Named(self.reference(2)?)
                };
                self.node()?;
                line_forms.push(form);
                if !self.take(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        let vertex = if self.take("VERTEX") {
            Some(self.qualified_name()?)
        } else {
            None
        };
        let overlaps = if self.take("WITHOUT") {
            let at = self.tokens[self.at - 1].range.start;
            self.expect("OVERLAPS")?;
            let tolerance = if self.take(">") {
                Some(self.number()?)
            } else if self.language == Language::V23 {
                return Err(Self::expected(
                    "INTERLIS 2.3 WITHOUT OVERLAPS requires '> Dec'",
                ));
            } else {
                None
            };
            self.node()?;
            Some(OverlapSyntax {
                tolerance,
                span: self.span(at, self.end()),
            })
        } else {
            None
        };
        let line_attributes = if self.language == Language::V23 && self.take("LINE") {
            self.expect("ATTRIBUTES")?;
            Some(self.reference(3)?)
        } else {
            None
        };
        self.node()?;
        Ok(GeometrySyntax {
            kind,
            directed,
            line_forms,
            vertex,
            overlaps,
            line_attributes,
            span: self.span(start, self.end()),
        })
    }
    fn class_value_type(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        if self.tokens.get(self.at + 1).is_some_and(|t| {
            t.kind == Kind::Word
                && !keywords::reserved(&self.source()[t.range.clone()], self.language)
        }) {
            return Err(Self::expected(
                "Unexpected identifier after a value-type keyword",
            ));
        }

        let start = self.start();
        let kind = if self.take("CLASS") {
            ClassValueKind::Class
        } else {
            self.expect("STRUCTURE")?;
            ClassValueKind::Structure
        };
        let mut restrictions = vec![];
        if self.take("RESTRICTION") {
            self.expect("(")?;
            restrictions.push(self.class_reference()?);
            while self.take(";") {
                restrictions.push(self.class_reference()?);
            }
            self.expect(")")?;
        }
        self.node()?;
        Ok(TypeSyntax::ClassValue {
            kind,
            restrictions,
            span: self.span(start, self.end()),
        })
    }
    fn attribute_path_type(&mut self) -> Parsed<TypeSyntax> {
        self.step()?;
        if self.tokens.get(self.at + 1).is_some_and(|t| {
            t.kind == Kind::Word
                && !keywords::reserved(&self.source()[t.range.clone()], self.language)
        }) {
            return Err(Self::expected(
                "Unexpected identifier after a value-type keyword",
            ));
        }

        let start = self.start();
        self.expect("ATTRIBUTE")?;
        let of = if self.take("OF") {
            Some(if self.take("@") {
                AttributePathOf::Argument(self.name(false)?)
            } else {
                AttributePathOf::Path(self.object_path()?)
            })
        } else {
            None
        };
        let mut restrictions = vec![];
        if self.take("RESTRICTION") {
            self.enter()?;
            let result = (|| {
                self.expect("(")?;
                restrictions.push(self.type_use()?);
                while self.take(";") {
                    restrictions.push(self.type_use()?);
                }
                self.expect(")")
            })();
            self.depth -= 1;
            result?;
        }
        self.node()?;
        Ok(TypeSyntax::AttributePathValue {
            of,
            restrictions,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn object_path(&mut self) -> Parsed<ObjectPath> {
        self.step()?;
        let start = self.start();
        let mut elements = vec![self.path_element()?];
        while self.take("->") {
            elements.push(self.path_element()?);
        }
        self.node()?;
        Ok(ObjectPath {
            elements,
            span: self.span(start, self.end()),
        })
    }
    fn path_element(&mut self) -> Parsed<PathElement> {
        self.step()?;
        let start = self.start();
        let special = match self.text() {
            "THIS" => Some(SpecialPath::This),
            "THISAREA" => Some(SpecialPath::ThisArea),
            "THATAREA" => Some(SpecialPath::ThatArea),
            "PARENT" => Some(SpecialPath::Parent),
            "AGGREGATES" => Some(SpecialPath::Aggregates),
            _ => None,
        };
        if let Some(value) = special {
            self.node()?;
            let span = self.current_span();
            self.step()?;
            self.at += 1;
            return Ok(PathElement::Special(Located { value, span }));
        }
        if self.take("\\") {
            let name = self.name(false)?;
            self.node()?;
            return Ok(PathElement::AssociationAccess {
                name,
                span: self.span(start, self.end()),
            });
        }
        let name = self.name(false)?;
        let selector = if self.take("[") {
            let selector = if let Some(s) = self.marker("FIRST") {
                PathSelector::First(s)
            } else if let Some(s) = self.marker("LAST") {
                PathSelector::Last(s)
            } else if self
                .tokens
                .get(self.at)
                .is_some_and(|t| t.kind == Kind::Number)
            {
                PathSelector::Index(self.unsigned_integer()?)
            } else {
                PathSelector::Association(self.name(false)?)
            };
            self.expect("]")?;
            self.node()?;
            Some(selector)
        } else {
            None
        };
        self.node()?;
        Ok(PathElement::Name {
            name,
            selector,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn unit_section(&mut self, context: Context) -> Parsed<Vec<Declaration>> {
        let mut items = vec![];
        while self.unreserved_name() && !self.diagnostics.limited {
            let start = self.start();
            let name = self.name(false)?;
            let result = (|| {
                let display = if self.take("(") {
                    let at = self.tokens[self.at - 1].range.start;
                    self.expect("ABSTRACT")?;
                    self.expect(")")?;
                    Some(UnitDisplay::Abstract(self.span(at, self.end())))
                } else if self.take("[") {
                    let n = self.name(false)?;
                    self.expect("]")?;
                    Some(UnitDisplay::ShortName(n))
                } else {
                    None
                };
                let base = if self.take("EXTENDS") {
                    Some(self.reference(3)?)
                } else {
                    None
                };
                let definition = if self.take("=") {
                    Some(self.unit_definition()?)
                } else {
                    None
                };
                self.expect(";")?;
                self.node()?;
                Ok(Declaration::Unit(UnitDecl {
                    name,
                    display,
                    base,
                    definition,
                    span: self.span(start, self.end()),
                }))
            })();
            match result {
                Ok(item) => items.push(item),
                Err(issue) => {
                    self.report(issue);
                    self.recover_named_section(context);
                }
            }
        }
        Ok(items)
    }
    fn recover_named_section(&mut self, context: Context) {
        let mut nesting = 0_u32;
        while self.at < self.tokens.len() && !self.is("END") && !self.diagnostics.limited {
            if self.step().is_err() {
                return;
            }
            if nesting == 0 && (self.unreserved_name() || self.recovery_start(context, false)) {
                return;
            }
            if nesting == 0 && self.take(";") {
                return;
            }
            match self.text() {
                "(" | "[" | "{" => nesting += 1,
                ")" | "]" | "}" => nesting = nesting.saturating_sub(1),
                _ => {}
            }
            self.at += 1;
        }
    }
    pub(super) fn decimal_constant(&mut self) -> Parsed<DecimalConstant> {
        self.step()?;
        self.node()?;
        Ok(if let Some(span) = self.marker("PI") {
            DecimalConstant::Pi(span)
        } else if let Some(span) = self.marker("LNBASE") {
            DecimalConstant::LnBase(span)
        } else {
            DecimalConstant::Number(self.number()?)
        })
    }
    fn unit_operator(&mut self) -> Parsed<Located<UnitOperator>> {
        let start = self.start();
        let value = if self.take("*") {
            UnitOperator::Multiply
        } else {
            self.expect("/")?;
            UnitOperator::Divide
        };
        self.node()?;
        Ok(Located {
            value,
            span: self.span(start, self.end()),
        })
    }
    fn unit_definition(&mut self) -> Parsed<UnitDefinition> {
        self.step()?;
        let start = self.start();
        if self.take("(") {
            let first = self.reference(3)?;
            let mut rest = vec![];
            while self.is("*") || self.is("/") {
                rest.push((self.unit_operator()?, self.reference(3)?));
            }
            self.expect(")")?;
            self.node()?;
            return Ok(UnitDefinition::Composed {
                first,
                rest,
                span: self.span(start, self.end()),
            });
        }
        let scale = if self.take("FUNCTION") {
            let Some(explanation) = self.explanation()? else {
                return Err(Self::expected("Unit FUNCTION requires an explanation"));
            };
            Some(UnitScale::Function(explanation))
        } else if !self.is("[") {
            let first = self.decimal_constant()?;
            let mut rest = vec![];
            while self.is("*") || self.is("/") {
                rest.push((self.unit_operator()?, self.decimal_constant()?));
            }
            Some(UnitScale::Factors { first, rest })
        } else {
            None
        };
        self.expect("[")?;
        let unit = self.reference(3)?;
        self.expect("]")?;
        self.node()?;
        Ok(UnitDefinition::Derived {
            scale,
            unit,
            span: self.span(start, self.end()),
        })
    }
    pub(super) fn line_form_section(&mut self) -> Parsed<Vec<Declaration>> {
        self.expect("LINE")?;
        self.expect("FORM")?;
        let mut items = vec![];
        while self.unreserved_name() && !self.diagnostics.limited {
            let start = self.start();
            let name = self.name(false)?;
            let result = (|| {
                self.expect(":")?;
                let segment = self.name(false)?;
                self.expect(";")?;
                self.node()?;
                Ok(Declaration::LineForm(LineFormDecl {
                    name,
                    segment,
                    span: self.span(start, self.end()),
                }))
            })();
            match result {
                Ok(item) => items.push(item),
                Err(issue) => {
                    self.report(issue);
                    self.recover_named_section(Context::Model);
                }
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisStatus, CompileRequest, SourceInput, analyze};
    fn source(version: &str, body: &str) -> String {
        format!(
            "INTERLIS {version}; MODEL M AT \"https://example.invalid\" VERSION \"1\" = {body} END M."
        )
    }
    pub(super) fn read(text: &str, limits: Limits) -> (SourceSet, Option<Document>, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "memory:///types-😀.ili".into(),
            text: text.into(),
        }])
        .unwrap();
        let mut d = Diagnostics::new(&limits);
        let mut budget = SyntaxBudget::new(&limits);
        let document = super::super::parse(&sources, 0, &limits, &mut budget, &mut d);
        (sources, document, d)
    }
    fn clean(version: &str, body: &str) -> (SourceSet, Document) {
        let (s, doc, d) = read(&source(version, body), Limits::default());
        assert!(d.entries.is_empty(), "{version}: {body}\n{:?}", d.entries);
        (s, doc.unwrap())
    }
    fn original<'a>(s: &'a SourceSet, span: &Span) -> &'a str {
        let p = s.location(span).unwrap();
        &s.files()[0].text()[p.start_byte as usize..p.end_byte as usize]
    }
    fn domain(item: &Declaration) -> &TypeSyntax {
        let Declaration::Domain { ty: Some(ty), .. } = item else {
            panic!("{item:?}")
        };
        ty
    }
    fn report(version: &str, body: &str) -> crate::AnalysisReport {
        analyze(CompileRequest {
            sources: vec![SourceInput {
                name: "x".into(),
                text: source(version, body),
            }],
            limits: Limits::default(),
        })
        .unwrap()
    }
    #[test]
    fn scalar_families_domain_properties_and_optional_type_are_preserved() {
        for version in ["2.3", "2.4"] {
            let (_, doc) = clean(
                version,
                "DOMAIN Label=TEXT*00040; Unbounded=TEXT; Multiline=MTEXT*255; Id=NAME; Link=URI; Flag=BOOLEAN; H=HALIGNMENT; V=VALIGNMENT; Base (ABSTRACT,FINAL) = NUMERIC; Derived EXTENDS Label=MANDATORY; Missing=MANDATORY;",
            );
            let items = &doc.models[0].declarations;
            assert_eq!(items.len(), 11);
            assert!(
                matches!(domain(&items[0]),TypeSyntax::Text(TextSyntax{kind:TextKind::Text,max_length:Some(n),..}) if n.value=="00040")
            );
            assert!(matches!(
                domain(&items[1]),
                TypeSyntax::Text(TextSyntax {
                    kind: TextKind::Text,
                    max_length: None,
                    ..
                })
            ));
            assert!(
                matches!(domain(&items[2]),TypeSyntax::Text(TextSyntax{kind:TextKind::MText,max_length:Some(n),..}) if n.value=="255")
            );
            assert!(matches!(
                domain(&items[3]),
                TypeSyntax::Text(TextSyntax {
                    kind: TextKind::Name,
                    max_length: None,
                    ..
                })
            ));
            assert!(matches!(
                domain(&items[4]),
                TypeSyntax::Text(TextSyntax {
                    kind: TextKind::Uri,
                    max_length: None,
                    ..
                })
            ));
            assert!(matches!(domain(&items[5]), TypeSyntax::Boolean { .. }));
            assert!(matches!(
                domain(&items[6]),
                TypeSyntax::Alignment {
                    kind: AlignmentKind::Horizontal,
                    ..
                }
            ));
            assert!(matches!(
                domain(&items[7]),
                TypeSyntax::Alignment {
                    kind: AlignmentKind::Vertical,
                    ..
                }
            ));
            let Declaration::Domain {
                properties,
                base,
                ty: Some(TypeSyntax::Numeric(n)),
                ..
            } = &items[8]
            else {
                panic!()
            };
            assert_eq!(
                properties.iter().map(|p| p.value).collect::<Vec<_>>(),
                [Property::Abstract, Property::Final]
            );
            assert!(base.is_none());
            assert!(n.bounds.is_none());
            assert!(
                matches!(&items[9],Declaration::Domain{base:Some(q),ty:None,mandatory:true,..} if q.segments[0].value=="Label")
            );
            assert!(matches!(
                &items[10],
                Declaration::Domain {
                    base: None,
                    ty: None,
                    mandatory: true,
                    ..
                }
            ));
        }
        let (_, doc) = clean(
            "2.4",
            "DOMAIN D=DATE; T=TIMEOFDAY; DT=DATETIME; G (GENERIC)=COORD NUMERIC,NUMERIC;",
        );
        assert!(matches!(
            domain(&doc.models[0].declarations[0]),
            TypeSyntax::Temporal {
                kind: TemporalKind::Date,
                ..
            }
        ));
        assert!(matches!(
            domain(&doc.models[0].declarations[1]),
            TypeSyntax::Temporal {
                kind: TemporalKind::TimeOfDay,
                ..
            }
        ));
        assert!(matches!(
            domain(&doc.models[0].declarations[2]),
            TypeSyntax::Temporal {
                kind: TemporalKind::DateTime,
                ..
            }
        ));
        assert!(
            matches!(&doc.models[0].declarations[3],Declaration::Domain{properties,..} if properties[0].value==Property::Generic)
        );
        clean(
            "2.3",
            "DOMAIN TIMEOFDAY=BOOLEAN; DATETIME=BOOLEAN; STRUCTURE S=t:TIMEOFDAY; dt:DATETIME; END S;",
        );
    }
    #[test]
    fn dotted_enumeration_paths_and_nested_children_remain_different() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "DOMAIN E=(red (dark,light:FINAL),green) ORDERED; R EXTENDS E=(red.dark,blue (FINAL):FINAL) CIRCULAR; T=ALL OF M.E;",
            );
            let TypeSyntax::Enumeration {
                enumeration: e,
                order: Some(order),
                ..
            } = domain(&doc.models[0].declarations[0])
            else {
                panic!()
            };
            assert_eq!(order.value, EnumerationOrder::Ordered);
            assert_eq!(e.elements[0].path.len(), 1);
            assert_eq!(e.elements[0].children.as_ref().unwrap().elements.len(), 2);
            assert!(
                e.elements[0]
                    .children
                    .as_ref()
                    .unwrap()
                    .final_marker
                    .is_some()
            );
            let TypeSyntax::Enumeration {
                enumeration: r,
                order: Some(order),
                ..
            } = domain(&doc.models[0].declarations[1])
            else {
                panic!()
            };
            assert_eq!(order.value, EnumerationOrder::Circular);
            assert_eq!(
                r.elements[0]
                    .path
                    .iter()
                    .map(|n| n.value.as_str())
                    .collect::<Vec<_>>(),
                ["red", "dark"]
            );
            assert!(r.elements[0].children.is_none());
            assert!(r.final_marker.is_some());
            let child = r.elements[1].children.as_ref().unwrap();
            assert!(child.elements.is_empty());
            assert_eq!(original(&s, &child.span), "(FINAL)");
            assert_eq!(original(&s, child.final_marker.as_ref().unwrap()), "FINAL");
            assert!(
                matches!(domain(&doc.models[0].declarations[2]),TypeSyntax::EnumTree{domain,..} if domain.segments[1].value=="E")
            );
        }
    }
    #[test]
    fn numeric_options_inner_references_and_written_precision_are_independent() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "DOMAIN Exact=9007199254740992..9007199254740993; Scaled=-0.1200E3..0.1201E3 CIRCULAR [Units.m] {M.T.B.System[0002]}; Dir=NUMERIC [INTERLIS.rad] COUNTERCLOCKWISE; Ref=0..100 <Model.Coord[1]>; Free=NUMERIC;",
            );
            let TypeSyntax::Numeric(exact) = domain(&doc.models[0].declarations[0]) else {
                panic!()
            };
            assert_eq!(
                exact.bounds.as_ref().unwrap().upper.value,
                "9007199254740993"
            );
            let TypeSyntax::Numeric(n) = domain(&doc.models[0].declarations[1]) else {
                panic!()
            };
            assert_eq!(n.bounds.as_ref().unwrap().lower.value, "-0.1200E3");
            assert_eq!(n.bounds.as_ref().unwrap().upper.value, "0.1201E3");
            assert_eq!(n.unit.as_ref().unwrap().segments[1].value, "m");
            assert!(n.direction.is_none());
            assert_eq!(original(&s, n.circular.as_ref().unwrap()), "CIRCULAR");
            let ReferenceSystemSyntax::Metaobject {
                reference,
                axis: Some(axis),
                span,
            } = n.reference_system.as_ref().unwrap()
            else {
                panic!()
            };
            assert_eq!(reference.segments.len(), 4);
            assert_eq!(reference.segments[3].value, "System");
            assert_eq!(axis.value, "0002");
            assert_eq!(original(&s, span), "{M.T.B.System[0002]}");
            for segment in &reference.segments {
                assert_eq!(original(&s, &segment.span), segment.value);
            }
            let TypeSyntax::Numeric(dir) = domain(&doc.models[0].declarations[2]) else {
                panic!()
            };
            assert!(dir.bounds.is_none());
            assert_eq!(
                dir.direction.as_ref().unwrap().value,
                Direction::Counterclockwise
            );
            assert!(dir.reference_system.is_none());
            let TypeSyntax::Numeric(r) = domain(&doc.models[0].declarations[3]) else {
                panic!()
            };
            assert!(
                matches!(&r.reference_system,Some(ReferenceSystemSyntax::Domain{reference,axis:Some(axis),..}) if reference.segments[1].value=="Coord" && axis.value=="1")
            );
            assert!(matches!(
                domain(&doc.models[0].declarations[4]),
                TypeSyntax::Numeric(NumericSyntax {
                    bounds: None,
                    unit: None,
                    direction: None,
                    reference_system: None,
                    ..
                })
            ));
        }
    }
    #[test]
    fn format_parts_inheritance_widths_domains_and_ranges_survive() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "DOMAIN F=FORMAT BASED ON Calendar (INHERITANCE \"prefix😀\" Year/4 \"-\" Month/Formats.Month \"!\") \"0000\"..\"9999\"; R EXTENDS F=\"0010\"..\"0099\"; D=FORMAT Other.Format \"1000\"..\"2000\";",
            );
            let TypeSyntax::Formatted {
                base: FormatBase::Structure { reference, format },
                range: Some(range),
                ..
            } = domain(&doc.models[0].declarations[0])
            else {
                panic!()
            };
            assert_eq!(reference.segments[0].value, "Calendar");
            assert_eq!(
                original(&s, format.inheritance.as_ref().unwrap()),
                "INHERITANCE"
            );
            assert_eq!(format.parts.len(), 5);
            assert!(matches!(&format.parts[0],FormatPart::Literal(v) if v.value=="prefix😀"));
            assert!(
                matches!(&format.parts[1],FormatPart::Attribute{attribute:FormatAttribute::Numeric{name,width:Some(width)},..} if name.value=="Year" && width.value=="4")
            );
            let FormatPart::Attribute {
                attribute:
                    FormatAttribute::Formatted {
                        name,
                        domain: format_domain,
                    },
                span,
            } = &format.parts[3]
            else {
                panic!()
            };
            assert_eq!(name.value, "Month");
            assert_eq!(format_domain.segments[1].value, "Month");
            assert_eq!(original(&s, &format_domain.segments[0].span), "Formats");
            assert_eq!(original(&s, span), "Month/Formats.Month");
            assert_eq!(range.lower.value, "0000");
            assert_eq!(range.upper.value, "9999");
            assert!(
                matches!(domain(&doc.models[0].declarations[1]),TypeSyntax::Formatted{base:FormatBase::Inherited,range:Some(r),..} if r.lower.value=="0010")
            );
            assert!(
                matches!(domain(&doc.models[0].declarations[2]),TypeSyntax::Formatted{base:FormatBase::Domain(q),range:Some(r),..} if q.segments[1].value=="Format" && r.upper.value=="2000")
            );
            // Only the inner format reference changes; it must never disappear.
            let (_, other) = clean(
                version,
                "DOMAIN F=FORMAT BASED ON Calendar (Month/Formats.Other);",
            );
            let TypeSyntax::Formatted {
                base: FormatBase::Structure { format, .. },
                ..
            } = domain(&other.models[0].declarations[0])
            else {
                panic!()
            };
            assert!(
                matches!(&format.parts[0],FormatPart::Attribute{attribute:FormatAttribute::Formatted{domain,..},..} if domain.segments[1].value=="Other")
            );
        }
    }
    #[test]
    fn coordinate_and_geometry_options_keep_all_inner_structure_references() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "DOMAIN C=COORD 0.00..100.00 [m] {Systems.Global[1]}, -10.00..10.00 [m] <Other.Coord[2]>, ROTATION 2->1; L=DIRECTED POLYLINE WITH (STRAIGHTS,ARCS,Curves.Custom) VERTEX M.C WITHOUT OVERLAPS >0.01;",
            );
            let TypeSyntax::Coordinate(c) = domain(&doc.models[0].declarations[0]) else {
                panic!()
            };
            assert_eq!(c.kind, CoordinateKind::Single);
            assert_eq!(c.axes.len(), 2);
            assert!(matches!(
                c.axes[0].reference_system,
                Some(ReferenceSystemSyntax::Metaobject { .. })
            ));
            assert!(matches!(
                c.axes[1].reference_system,
                Some(ReferenceSystemSyntax::Domain { .. })
            ));
            let r = c.rotation.as_ref().unwrap();
            assert_eq!(r.zero.value, "2");
            assert_eq!(r.quarter_turn.value, "1");
            assert_eq!(original(&s, &r.span), "ROTATION 2->1");
            assert!(c.crs.is_none());
            let TypeSyntax::Geometry(g) = domain(&doc.models[0].declarations[1]) else {
                panic!()
            };
            assert_eq!(g.kind, GeometryKind::Polyline);
            assert!(g.directed.is_some());
            assert!(matches!(g.line_forms[0], LineFormSyntax::Straights(_)));
            assert!(matches!(g.line_forms[1], LineFormSyntax::Arcs(_)));
            assert!(
                matches!(&g.line_forms[2],LineFormSyntax::Named(q) if q.segments[1].value=="Custom")
            );
            assert_eq!(g.vertex.as_ref().unwrap().segments[1].value, "C");
            assert_eq!(
                g.overlaps
                    .as_ref()
                    .unwrap()
                    .tolerance
                    .as_ref()
                    .unwrap()
                    .value,
                "0.01"
            );
        }
        for structure in ["M.T.A", "M.T.B"] {
            let (s, doc) = clean(
                "2.3",
                &format!(
                    "DOMAIN G=SURFACE WITH (STRAIGHTS) VERTEX C WITHOUT OVERLAPS >0.01 LINE ATTRIBUTES {structure};"
                ),
            );
            let TypeSyntax::Geometry(g) = domain(&doc.models[0].declarations[0]) else {
                panic!()
            };
            let attributes = g.line_attributes.as_ref().unwrap();
            assert_eq!(attributes.segments.len(), 3);
            assert_eq!(original(&s, &attributes.span), structure);
            assert_eq!(original(&s, &attributes.segments[2].span), &structure[4..]);
        }
        let (_, doc) = clean(
            "2.4",
            "DOMAIN C=MULTICOORD NUMERIC,NUMERIC REFSYS \"EPSG:2056\"; L=DIRECTED MULTIPOLYLINE WITH (ARCS) VERTEX C WITHOUT OVERLAPS; S=MULTISURFACE; A=MULTIAREA; Surface=SURFACE; Area=AREA;",
        );
        let TypeSyntax::Coordinate(c) = domain(&doc.models[0].declarations[0]) else {
            panic!()
        };
        assert_eq!(c.kind, CoordinateKind::Multi);
        assert_eq!(c.crs.as_ref().unwrap().value, "EPSG:2056");
        let TypeSyntax::Geometry(g) = domain(&doc.models[0].declarations[1]) else {
            panic!()
        };
        assert_eq!(g.kind, GeometryKind::MultiPolyline);
        assert!(g.overlaps.as_ref().unwrap().tolerance.is_none());
        for (item, kind) in doc.models[0].declarations[2..].iter().zip([
            GeometryKind::MultiSurface,
            GeometryKind::MultiArea,
            GeometryKind::Surface,
            GeometryKind::Area,
        ]) {
            assert!(matches!(domain(item),TypeSyntax::Geometry(g) if g.kind==kind));
        }
    }
    #[test]
    fn oid_blackbox_class_values_paths_references_and_collections_are_concrete() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "DOMAIN O=OID ANY; N=OID 0..2147483647; T=OID TEXT*36; X=BLACKBOX XML; B=BLACKBOX BINARY; C=CLASS RESTRICTION (INTERLIS.SIGN; M.T.C); S=STRUCTURE RESTRICTION (Base; M.Other); P=ATTRIBUTE OF @argument RESTRICTION (MANDATORY TEXT*4; REFERENCE TO (EXTERNAL) ANYCLASS); Path=ATTRIBUTE OF THIS->owner[Assoc]->parts[FIRST]->point[9007199254740993]->\\back->PARENT->AGGREGATES; STRUCTURE Holder=ref:REFERENCE TO (EXTERNAL,EXTERNAL) ANYCLASS RESTRICTION (M.T.C; Other.D); bag:BAG {2..*} OF ANYSTRUCTURE RESTRICTION (Base;Other.S); list:LIST OF Base; named:Unresolved; restricted:Maybe RESTRICTION (A;B); END Holder;",
            );
            let items = &doc.models[0].declarations;
            assert!(matches!(
                domain(&items[0]),
                TypeSyntax::Oid {
                    carrier: OidSyntax::Any(_),
                    ..
                }
            ));
            assert!(
                matches!(domain(&items[1]),TypeSyntax::Oid{carrier:OidSyntax::Numeric(n),..} if n.bounds.as_ref().unwrap().upper.value=="2147483647")
            );
            assert!(
                matches!(domain(&items[2]),TypeSyntax::Oid{carrier:OidSyntax::Text(TextSyntax{max_length:Some(n),..}),..} if n.value=="36")
            );
            assert!(matches!(
                domain(&items[3]),
                TypeSyntax::Blackbox {
                    kind: BlackboxKind::Xml,
                    ..
                }
            ));
            assert!(matches!(
                domain(&items[4]),
                TypeSyntax::Blackbox {
                    kind: BlackboxKind::Binary,
                    ..
                }
            ));
            assert!(
                matches!(domain(&items[5]),TypeSyntax::ClassValue{kind:ClassValueKind::Class,restrictions,..} if restrictions[0].segments[1].value=="SIGN" && restrictions[1].segments.len()==3)
            );
            assert!(
                matches!(domain(&items[6]),TypeSyntax::ClassValue{kind:ClassValueKind::Structure,restrictions,..} if restrictions.len()==2)
            );
            let TypeSyntax::AttributePathValue {
                of: Some(AttributePathOf::Argument(arg)),
                restrictions,
                ..
            } = domain(&items[7])
            else {
                panic!()
            };
            assert_eq!(arg.value, "argument");
            assert_eq!(restrictions.len(), 2);
            assert!(restrictions[0].mandatory);
            assert!(
                matches!(&restrictions[1].ty,Some(TypeSyntax::Reference{properties,..}) if properties[0].value==Property::External)
            );
            let TypeSyntax::AttributePathValue {
                of: Some(AttributePathOf::Path(path)),
                ..
            } = domain(&items[8])
            else {
                panic!()
            };
            assert_eq!(path.elements.len(), 7);
            assert!(
                matches!(&path.elements[1],PathElement::Name{selector:Some(PathSelector::Association(n)),..} if n.value=="Assoc")
            );
            assert!(matches!(
                &path.elements[2],
                PathElement::Name {
                    selector: Some(PathSelector::First(_)),
                    ..
                }
            ));
            assert!(
                matches!(&path.elements[3],PathElement::Name{selector:Some(PathSelector::Index(n)),..} if n.value=="9007199254740993")
            );
            assert!(
                matches!(&path.elements[4],PathElement::AssociationAccess{name,span} if name.value=="back" && original(&s,span)=="\\back")
            );
            assert!(matches!(
                &path.elements[6],
                PathElement::Special(Located {
                    value: SpecialPath::Aggregates,
                    ..
                })
            ));
            let Declaration::Container { attributes, .. } = &items[9] else {
                panic!()
            };
            let Declaration::Attribute {
                type_use:
                    TypeUseSyntax {
                        ty:
                            Some(TypeSyntax::Reference {
                                properties, target, ..
                            }),
                        ..
                    },
                ..
            } = &attributes[0]
            else {
                panic!()
            };
            assert_eq!(properties.len(), 2);
            assert!(matches!(target.target, TargetRef::AnyClass(_)));
            assert_eq!(target.restrictions[1].segments[1].value, "D");
            assert!(
                matches!(&attributes[1],Declaration::Attribute{type_use:TypeUseSyntax{ty:Some(TypeSyntax::Collection{kind:CollectionKind::Bag,cardinality:Some(Cardinality::Range{lower,upper:CardinalityBound::Unbounded(_),..}),element,..}),..},..} if lower.value=="2" && matches!(element.as_ref(),TypeSyntax::RestrictedStructure{target:StructureTarget::Any(_),restrictions,..} if restrictions.len()==2))
            );
            assert!(matches!(
                &attributes[2],
                Declaration::Attribute {
                    type_use: TypeUseSyntax {
                        ty: Some(TypeSyntax::Collection {
                            kind: CollectionKind::List,
                            cardinality: None,
                            ..
                        }),
                        ..
                    },
                    ..
                }
            ));
            assert!(
                matches!(&attributes[3],Declaration::Attribute{type_use:TypeUseSyntax{ty:Some(TypeSyntax::Named(q)),..},..} if q.segments[0].value=="Unresolved")
            );
            assert!(
                matches!(&attributes[4],Declaration::Attribute{type_use:TypeUseSyntax{ty:Some(TypeSyntax::RestrictedStructure{target:StructureTarget::Named(q),restrictions,..}),..},..} if q.segments[0].value=="Maybe" && restrictions[1].segments[0].value=="B")
            );
        }
        let (_, doc) = clean(
            "2.4",
            "STRUCTURE S=values:LIST {0..100} OF TEXT*8; refs:BAG OF REFERENCE TO (EXTERNAL) Unknown.C; END S;",
        );
        let Declaration::Container { attributes, .. } = &doc.models[0].declarations[0] else {
            panic!()
        };
        assert!(
            matches!(&attributes[0],Declaration::Attribute{type_use:TypeUseSyntax{ty:Some(TypeSyntax::Collection{element,..}),..},..} if matches!(element.as_ref(),TypeSyntax::Text(TextSyntax{max_length:Some(n),..}) if n.value=="8"))
        );
    }
    #[test]
    fn units_keep_operator_order_symbolic_constants_functions_and_line_segment_names() {
        for version in ["2.3", "2.4"] {
            let (s, doc) = clean(
                version,
                "UNIT Dimension (ABSTRACT); Metre [m] EXTENDS Dimension; Centimetre [cm]=1/100[m]; Symbolic=2*PI/LNBASE[rad]; Affine=FUNCTION // value * 2 + 1 // [K]; Product=(m/s*kg); Alias=[m]; LINE FORM curved:Segment; second:OtherSegment;",
            );
            let items = &doc.models[0].declarations;
            assert_eq!(items.len(), 9);
            assert!(matches!(
                &items[0],
                Declaration::Unit(UnitDecl {
                    display: Some(UnitDisplay::Abstract(_)),
                    definition: None,
                    ..
                })
            ));
            assert!(
                matches!(&items[1],Declaration::Unit(UnitDecl{display:Some(UnitDisplay::ShortName(n)),base:Some(base),..}) if n.value=="m" && base.segments[0].value=="Dimension")
            );
            let Declaration::Unit(UnitDecl {
                definition:
                    Some(UnitDefinition::Derived {
                        scale:
                            Some(UnitScale::Factors {
                                first: DecimalConstant::Number(first),
                                rest,
                            }),
                        unit,
                        ..
                    }),
                ..
            }) = &items[2]
            else {
                panic!()
            };
            assert_eq!(first.value, "1");
            assert_eq!(rest[0].0.value, UnitOperator::Divide);
            assert!(matches!(&rest[0].1,DecimalConstant::Number(n) if n.value=="100"));
            assert_eq!(unit.segments[0].value, "m");
            assert_eq!(original(&s, &rest[0].0.span), "/");
            assert!(
                matches!(&items[3],Declaration::Unit(UnitDecl{definition:Some(UnitDefinition::Derived{scale:Some(UnitScale::Factors{rest,..}),..}),..}) if matches!(rest[0].1,DecimalConstant::Pi(_)) && matches!(rest[1].1,DecimalConstant::LnBase(_)) && rest[0].0.value==UnitOperator::Multiply && rest[1].0.value==UnitOperator::Divide)
            );
            assert!(
                matches!(&items[4],Declaration::Unit(UnitDecl{definition:Some(UnitDefinition::Derived{scale:Some(UnitScale::Function(explanation)),..}),..}) if explanation.value==" value * 2 + 1 ")
            );
            assert!(
                matches!(&items[5],Declaration::Unit(UnitDecl{definition:Some(UnitDefinition::Composed{first,rest,..}),..}) if first.segments[0].value=="m" && rest.iter().map(|p|p.0.value).collect::<Vec<_>>()==[UnitOperator::Divide,UnitOperator::Multiply])
            );
            assert!(matches!(
                &items[6],
                Declaration::Unit(UnitDecl {
                    definition: Some(UnitDefinition::Derived { scale: None, .. }),
                    ..
                })
            ));
            assert!(
                matches!(&items[7],Declaration::LineForm(LineFormDecl{name,segment,..}) if name.value=="curved" && segment.value=="Segment")
            );
            assert!(
                matches!(&items[8],Declaration::LineForm(LineFormDecl{segment,..}) if segment.value=="OtherSegment")
            );
        }
        clean(
            "2.4",
            "UNIT UNIT DOMAIN DOMAIN PARAMETER CONTEXT TOPIC T=UNIT UNIT DOMAIN END T;",
        );
    }
    #[test]
    fn malformed_type_and_unit_productions_are_syntax_errors_in_both_editions() {
        let bodies = [
            "DOMAIN D=TEXT*+1;",
            "DOMAIN D=NAME*5;",
            "DOMAIN D=0..;",
            "DOMAIN D=NUMERIC CIRCULAR CIRCULAR;",
            "DOMAIN D=0..1 [m] CLOCKWISE {S};",
            "DOMAIN D=0..1 {S[1.5]};",
            "DOMAIN D=0..1 <M.C;",
            "DOMAIN D=COORD NUMERIC,NUMERIC,NUMERIC,NUMERIC;",
            "DOMAIN D=COORD NUMERIC,ROTATION 2->1;",
            "DOMAIN D=DIRECTED SURFACE;",
            "DOMAIN D=POLYLINE WITH (STRAIGHTS,);",
            "DOMAIN D=POLYLINE VERTEX C WITH (ARCS);",
            "DOMAIN D=OID BOOLEAN;",
            "DOMAIN D=BLACKBOX YAML;",
            "DOMAIN D=CLASS RESTRICTION ();",
            "DOMAIN D=ATTRIBUTE OF ;",
            "DOMAIN D=ATTRIBUTE RESTRICTION (TEXT*4;);",
            "STRUCTURE S=x:REFERENCE (EXTERNAL) TO C; END S;",
            "STRUCTURE S=x:REFERENCE TO (EXTERNAL,ORDERED) ANYCLASS; END S;",
            "STRUCTURE S=x:REFERENCE TO ANYCLASS RESTRICTION (A,B); END S;",
            "STRUCTURE S=x:MANDATORY BAG OF S; END S;",
            "STRUCTURE S=x:LIST OF BAG OF S; END S;",
            "DOMAIN D=REFERENCE TO C;",
            "DOMAIN D=BAG OF S;",
            "DOMAIN D=Unknown;",
            "DOMAIN D=();",
            "DOMAIN D=(a,);",
            "DOMAIN D=(a FINAL);",
            "DOMAIN D=(FINAL,a);",
            "DOMAIN D=(a.(b));",
            "DOMAIN D=(BASKET);",
            "DOMAIN D=FORMAT BASED ON S ();",
            "DOMAIN D=FORMAT BASED ON S (INHERITANCE);",
            "DOMAIN D=FORMAT BASED ON S (a \"2separator\" b);",
            "DOMAIN D=FORMAT BASED ON S (a b);",
            "DOMAIN D=FORMAT D;",
            "DOMAIN D=\"min\"..;",
            "UNIT u=FUNCTION [m];",
            "UNIT u=();",
            "UNIT u=1+2[m];",
            "UNIT u (FINAL);",
            "UNIT u [short] (ABSTRACT);",
            "LINE FORM curve:Model.Seg;",
        ];
        for version in ["2.3", "2.4"] {
            for body in bodies {
                let (_, _, d) = read(&source(version, body), Limits::default());
                assert!(d.syntax_invalid, "{version}: {body}\n{:?}", d.entries);
                assert!(!d.incomplete, "{body}: {:?}", d.entries);
                assert!(!d.limited);
            }
        }
    }
    #[test]
    fn version_specific_shapes_do_not_reserve_legacy_identifiers() {
        for body in [
            "DOMAIN D=DATE;",
            "DOMAIN D=MULTICOORD NUMERIC,NUMERIC;",
            "DOMAIN D=MULTIPOLYLINE;",
            "DOMAIN D=MULTISURFACE;",
            "DOMAIN D=MULTIAREA;",
            "DOMAIN D=COORD NUMERIC,NUMERIC REFSYS \"EPSG:1\";",
            "DOMAIN D=POLYLINE WITHOUT OVERLAPS;",
            "DOMAIN D (GENERIC)=COORD NUMERIC,NUMERIC;",
            "STRUCTURE S=x:LIST OF TEXT*4; END S;",
        ] {
            let (_, _, d) = read(&source("2.3", body), Limits::default());
            assert!(d.syntax_invalid, "{body}: {:?}", d.entries);
            assert!(!d.incomplete);
        }
        let (_, _, d) = read(
            &source("2.4", "DOMAIN D=SURFACE LINE ATTRIBUTES S;"),
            Limits::default(),
        );
        assert!(d.syntax_invalid);
        for name in [
            "TIMEOFDAY",
            "DATETIME",
            "MULTICOORD",
            "MULTIPOLYLINE",
            "MULTISURFACE",
            "MULTIAREA",
            "REFSYS",
            "GENERIC",
        ] {
            let body = format!("DOMAIN {name}=BOOLEAN; STRUCTURE S=value:{name}; END S;");
            clean("2.3", &body);
            assert_eq!(
                report("2.3", &body).status(),
                AnalysisStatus::CheckedSubset,
                "{name}"
            );
        }
    }
    #[test]
    fn new_type_semantics_are_explicitly_gated_and_descending_plain_bounds_stay_syntax() {
        for ty in [
            "(red,green)",
            "ALL OF INTERLIS.BOOLEAN",
            "NUMERIC",
            "0..360 CIRCULAR",
            "0..1 [INTERLIS.m]",
            "0..1 CLOCKWISE",
            "0..1 {Systems.R}",
            "FORMAT BASED ON INTERLIS.GregorianDate (Year/4)",
            "COORD NUMERIC,NUMERIC",
            "MULTICOORD NUMERIC,NUMERIC",
            "POLYLINE WITH (STRAIGHTS) VERTEX INTERLIS.LineCoord",
            "OID ANY",
            "CLASS RESTRICTION (INTERLIS.METAOBJECT)",
            "ATTRIBUTE OF @arg RESTRICTION (TEXT*4)",
        ] {
            let body = format!("DOMAIN New={ty};");
            clean("2.4", &body);
            let result = report("2.4", &body);
            assert_eq!(
                result.status(),
                AnalysisStatus::Incomplete,
                "{ty}: {:?}",
                result.diagnostics()
            );
            assert!(result.graph().is_none());
            assert!(!result.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        }
        clean("2.4", "DOMAIN N=10.00..1.00;");
        let result = report("2.4", "DOMAIN N=10.00..1.00;");
        assert_eq!(result.status(), AnalysisStatus::Rejected);
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.code == "ILI-NUMERIC-RANGE")
        );
        assert!(!result.diagnostics().iter().any(|d| d.code == "ILI-SYNTAX"));
        for body in [
            "DOMAIN D (FINAL)=BOOLEAN;",
            "DOMAIN Parent=TEXT*10; D EXTENDS Parent=TEXT*4;",
            "DOMAIN D=MANDATORY;",
            "UNIT metre[m];",
            "STRUCTURE Segment=END Segment; LINE FORM curve:Segment;",
        ] {
            clean("2.4", body);
            assert_eq!(report("2.4", body).status(), AnalysisStatus::Incomplete);
        }
    }
    #[test]
    fn recursive_type_limits_precede_deeper_construction_and_recovery_keeps_later_declarations() {
        let mut enumeration = "(leaf)".to_string();
        for i in 0..24 {
            enumeration = format!("(n{i}{enumeration})");
        }
        let restriction = format!(
            "{}TEXT*4{}",
            "ATTRIBUTE RESTRICTION (".repeat(24),
            ")".repeat(24)
        );
        for ty in [&enumeration, &restriction] {
            let text = source("2.4", &format!("DOMAIN D={ty};"));
            let (_, doc, d) = read(
                &text,
                Limits {
                    max_nesting: 32,
                    ..Limits::default()
                },
            );
            assert!(doc.is_some());
            assert!(d.entries.is_empty());
            for limits in [
                Limits {
                    max_nesting: 8,
                    ..Limits::default()
                },
                Limits {
                    max_syntax_nodes: 12,
                    ..Limits::default()
                },
                Limits {
                    max_parser_steps: 1,
                    ..Limits::default()
                },
            ] {
                let (_, doc, d) = read(&text, limits);
                assert!(doc.is_none());
                assert!(d.limited);
                assert!(d.entries.iter().any(|d| d.code == "ILI-LIMIT"));
                assert!(!d.syntax_invalid);
            }
        }
        let (_, doc, d) = read(
            &source(
                "2.4",
                "DOMAIN Broken=MANDATORY CLASS C=END C; DOMAIN Wrong=(); DOMAIN Good=BOOLEAN;",
            ),
            Limits::default(),
        );
        assert!(d.syntax_invalid);
        let doc = doc.unwrap();
        assert!(
            doc.models[0]
                .declarations
                .iter()
                .any(|d| matches!(d,Declaration::Container{name,..} if name.value=="C"))
        );
        assert!(
            doc.models[0]
                .declarations
                .iter()
                .any(|d| matches!(d,Declaration::Domain{name,..} if name.value=="Good"))
        );
        let (_, doc, d) = read(
            &source(
                "2.4",
                "UNIT bad (FINAL); good; LINE FORM broken:; later:Segment; DOMAIN Final=BOOLEAN;",
            ),
            Limits::default(),
        );
        assert!(d.syntax_invalid);
        let doc = doc.unwrap();
        assert!(
            doc.models[0]
                .declarations
                .iter()
                .any(|d| matches!(d,Declaration::Unit(u) if u.name.value=="good"))
        );
        assert!(
            doc.models[0]
                .declarations
                .iter()
                .any(|d| matches!(d,Declaration::LineForm(f) if f.name.value=="later"))
        );
        assert!(
            doc.models[0]
                .declarations
                .iter()
                .any(|d| matches!(d,Declaration::Domain{name,..} if name.value=="Final"))
        );
        clean("2.4", "DOMAIN Healthy=BOOLEAN;");
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    #[test]
    fn complete_type_fixtures_retain_original_sources_and_inner_links() {
        for (text, language) in [
            (
                include_str!("../../tests/fixtures/syntax/types23.ili"),
                Language::V23,
            ),
            (
                include_str!("../../tests/fixtures/syntax/types24.ili"),
                Language::V24,
            ),
        ] {
            let (sources, doc, d) = super::tests::read(text, Limits::default());
            assert!(d.entries.is_empty(), "{:?}", d.entries);
            let doc = doc.unwrap();
            assert_eq!(doc.language, language);
            assert_eq!(sources.files()[0].text(), text);
            assert_eq!(doc.models[0].end_name.value, "TypeExamples");
            if language == Language::V23 {
                let geometry = doc.models[0]
                    .declarations
                    .iter()
                    .find_map(|d| match d {
                        Declaration::Domain {
                            ty: Some(TypeSyntax::Geometry(g)),
                            ..
                        } => Some(g),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    geometry.line_attributes.as_ref().unwrap().segments[1].value,
                    "LineData"
                );
                assert!(
                    doc.models[0].declarations.iter().any(
                        |d| matches!(d,Declaration::LineForm(f) if f.segment.value=="Segment")
                    )
                );
            } else {
                let coordinate = doc.models[0]
                    .declarations
                    .iter()
                    .find_map(|d| match d {
                        Declaration::Domain {
                            ty: Some(TypeSyntax::Coordinate(c)),
                            ..
                        } if c.crs.is_some() => Some(c),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(coordinate.crs.as_ref().unwrap().value, "EPSG:2056");
                assert!(doc.models[0].declarations.iter().any(|d|matches!(d,Declaration::Context(c) if c.name.value=="empty" && c.bindings.is_empty())));
            }
        }
    }
}
