//! Owned provisional model data for the declared subset; never a full-schema seal.
use crate::Span;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum LanguageVersion {
    #[serde(rename = "2.3")]
    V23,
    #[serde(rename = "2.4")]
    V24,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationKind {
    Model,
    Topic,
    Class,
    Structure,
    Domain,
    Attribute,
    RuntimeParameter,
    Unit,
    LineForm,
    Function,
    Association,
    Role,
    Parameter,
    Context,
    View,
    Graphic,
    DrawingRule,
    MetadataBasket,
    MetaObject,
}
#[derive(Debug)]
pub struct ModelInfo {
    pub(crate) name: String,
    pub(crate) language: LanguageVersion,
    pub(crate) language_tag: Option<String>,
    pub(crate) issuer: String,
    pub(crate) version: String,
    pub(crate) contracted: bool,
    pub(crate) imports: Vec<String>,
}
impl ModelInfo {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn language(&self) -> LanguageVersion {
        self.language
    }
    pub fn language_tag(&self) -> Option<&str> {
        self.language_tag.as_deref()
    }
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn is_contracted(&self) -> bool {
        self.contracted
    }
    /// Only names actually bound in the supported input/predefined namespace.
    pub fn imports(&self) -> &[String] {
        &self.imports
    }
}
/// Original spelling and exact value `coefficient * 10^(-scale)`. No binary float.
#[derive(Clone, Debug)]
pub struct DecimalInfo {
    pub(crate) original: String,
    pub(crate) coefficient: String,
    pub(crate) fractional_digits: u32,
    pub(crate) exponent: i32,
    pub(crate) scaled: bool,
}
impl DecimalInfo {
    pub fn original(&self) -> &str {
        &self.original
    }
    pub fn coefficient(&self) -> &str {
        &self.coefficient
    }
    pub fn fractional_digits(&self) -> u32 {
        self.fractional_digits
    }
    pub fn written_exponent(&self) -> i32 {
        self.exponent
    }
    pub fn uses_exponent(&self) -> bool {
        self.scaled
    }
    pub fn scale(&self) -> i32 {
        i32::try_from(self.fractional_digits).expect("bounded precision") - self.exponent
    }
}
#[derive(Debug)]
pub enum TypeInfo {
    /// Positive exact integer, exposed as a string even beyond machine/JS integers.
    Text {
        max_length: String,
    },
    UnboundedText,
    MultilineText {
        max_length: Option<String>,
    },
    FixedText {
        format: FixedTextFormat,
        max_length: String,
    },
    Boolean,
    Alignment(AlignmentKind),
    Temporal(TemporalKind),
    Blackbox(BlackboxKind),
    Enumeration {
        values: Enumeration,
        order: Option<EnumerationOrder>,
        declared_order: Option<EnumerationOrder>,
    },
    EnumTree {
        domain: String,
    },
    Numeric(NumericRange),
    Coordinate(CoordinateType),
    Oid(OidType),
    Structure(StructureType),
    Collection(CollectionType),
    Reference(ReferenceType),
    Geometry(GeometryType),
}

impl TypeInfo {
    pub(crate) fn has_unbounded_range(&self) -> bool {
        match self {
            Self::Numeric(range) => range.bounds.is_none(),
            Self::Coordinate(coordinate) => {
                coordinate.axes.iter().any(|range| range.bounds.is_none())
            }
            Self::Oid(OidType::Typed(carrier)) => carrier.has_unbounded_range(),
            _ => false,
        }
    }
}

#[derive(Debug)]
pub enum OidType {
    Unstable,
    Any,
    Typed(Box<TypeInfo>),
    Standard,
    Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind {
    Bag,
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Cardinality {
    pub lower: String,
    pub upper: Option<String>,
}

#[derive(Debug)]
pub struct CollectionType {
    pub kind: CollectionKind,
    pub cardinality: Cardinality,
    pub(crate) element: usize,
}

#[derive(Debug, Serialize)]
pub struct StructureType {
    pub target: Option<TypeReference>,
    pub restrictions: Vec<TypeReference>,
}

#[derive(Debug, Serialize)]
pub struct ObjectTarget {
    pub target: Option<TypeReference>,
    pub restrictions: Vec<TypeReference>,
}

#[derive(Debug, Serialize)]
pub struct ReferenceType {
    pub external: bool,
    pub target: ObjectTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryKind {
    Polyline,
    Surface,
    Area,
    MultiPolyline,
    MultiSurface,
    MultiArea,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "reference", rename_all = "snake_case")]
pub enum LineForm {
    Straights,
    Arcs,
    Named(TypeReference),
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum OverlapTolerance {
    Explicit(DecimalInfo),
    TrackWidth(Option<TypeReference>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryType {
    pub geometry_kind: GeometryKind,
    pub directed: bool,
    pub line_forms: Vec<LineForm>,
    pub vertex: Option<TypeReference>,
    pub without_overlaps: bool,
    pub tolerance: Option<OverlapTolerance>,
    pub line_attributes: Option<TypeReference>,
    #[serde(skip)]
    pub(crate) language: LanguageVersion,
    #[serde(skip)]
    pub(crate) span: Span,
    #[serde(skip)]
    pub(crate) base_vertex: Option<TypeReference>,
    #[serde(skip)]
    pub(crate) explicit_tolerance: bool,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleStrength {
    Association,
    Aggregation,
    Composition,
}

#[derive(Debug, Serialize)]
pub struct RoleInfo {
    pub strength: RoleStrength,
    pub cardinality: Cardinality,
    pub ordered: bool,
    pub external: bool,
    pub hiding: bool,
    pub targets: Vec<ObjectTarget>,
}

#[derive(Debug)]
pub struct AssociationInfo {
    pub cardinality: Cardinality,
    pub identified: bool,
    pub(crate) roles: Box<[usize]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OidKind {
    Unstable,
    Any,
    Concrete,
}

#[derive(Clone, Debug, Serialize)]
pub struct OidRule {
    pub kind: OidKind,
    pub domain: Option<TypeReference>,
    #[serde(skip)]
    pub(crate) span: Span,
}

impl OidRule {
    pub fn source_span(&self) -> &Span {
        &self.span
    }
}

#[derive(Debug, Default, Serialize)]
pub struct OidRules {
    pub basket: Option<OidRule>,
    pub object: Option<OidRule>,
}

#[derive(Debug)]
pub(crate) struct TopicDependencies {
    pub declared: Box<[TypeReference]>,
    pub effective: Box<[TypeReference]>,
}

#[derive(Debug, Serialize)]
pub struct ContextMapping {
    pub generic: TypeReference,
    pub alternatives: Vec<TypeReference>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicGenericBinding {
    pub generic_domain: String,
    pub concrete_domains: Vec<String>,
    pub deferred: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateKind {
    Single,
    Multi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisRotation {
    pub zero_axis: u8,
    pub quarter_turn_axis: u8,
}

#[derive(Debug)]
pub struct CoordinateType {
    pub kind: CoordinateKind,
    pub axes: Vec<NumericRange>,
    pub rotation: Option<AxisRotation>,
    pub reference_system: Option<String>,
}

#[derive(Debug)]
pub struct NumericRange {
    pub bounds: Option<NumericBounds>,
    pub unit: Option<TypeReference>,
    pub reference_system: Option<DomainReferenceSystem>,
    pub circular: bool,
    pub direction: Option<NumericDirection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DomainReferenceSystem {
    pub domain: TypeReference,
    pub axis: Option<u8>,
}

#[derive(Debug)]
pub struct NumericBounds {
    pub lower: DecimalInfo,
    pub upper: DecimalInfo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NumericDirection {
    Clockwise,
    Counterclockwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixedTextFormat {
    Name,
    Uri,
    Interlis1Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignmentKind {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemporalKind {
    Date,
    TimeOfDay,
    DateTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlackboxKind {
    Xml,
    Binary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnumerationOrder {
    Ordered,
    Circular,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Enumeration {
    pub(crate) elements: Vec<EnumerationElement>,
    pub(crate) final_subtree: bool,
}

impl Enumeration {
    pub fn elements(&self) -> &[EnumerationElement] {
        &self.elements
    }

    pub fn is_final(&self) -> bool {
        self.final_subtree
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct EnumerationElement {
    pub(crate) name: String,
    #[serde(skip)]
    pub(crate) span: Span,
    pub(crate) children: Option<Enumeration>,
}

impl EnumerationElement {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source_span(&self) -> &Span {
        &self.span
    }

    pub fn children(&self) -> Option<&Enumeration> {
        self.children.as_ref()
    }
}
/// Retains declared name segments, exact bound target identity and original reference range.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeReference {
    #[serde(rename = "declaredName")]
    pub(crate) declared: String,
    #[serde(rename = "targetName")]
    pub(crate) target: String,
    #[serde(skip)]
    pub(crate) span: Span,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) predefined_language: Option<LanguageVersion>,
}
impl TypeReference {
    pub fn declared_name(&self) -> &str {
        &self.declared
    }
    pub fn target_name(&self) -> &str {
        &self.target
    }
    pub fn source_span(&self) -> &Span {
        &self.span
    }
    pub fn predefined_language(&self) -> Option<LanguageVersion> {
        self.predefined_language
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitInfo {
    pub designation: String,
    pub abstract_unit: bool,
    pub base: Option<TypeReference>,
    pub definition: UnitDefinition,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnitDefinition {
    Fundamental,
    Derived {
        target: TypeReference,
        scale: UnitScale,
    },
    Composed {
        terms: Vec<UnitTerm>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum UnitScale {
    Identity,
    Factors(Vec<UnitFactor>),
    Function(String),
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum UnitConstant {
    Decimal(DecimalInfo),
    Pi,
    LnBase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitOperator {
    Multiply,
    Divide,
}

#[derive(Debug, Serialize)]
pub struct UnitTerm {
    pub operator: UnitOperator,
    pub unit: TypeReference,
}

#[derive(Debug, Serialize)]
pub struct UnitFactor {
    pub operator: UnitOperator,
    pub value: UnitConstant,
}

#[derive(Debug)]
pub struct Declaration {
    pub(crate) name: String,
    pub(crate) qualified: String,
    pub(crate) kind: DeclarationKind,
    pub(crate) span: Span,
    pub(crate) parent: Option<usize>,
    pub(crate) type_id: Option<usize>,
    pub(crate) declared_mandatory: Option<bool>,
    pub(crate) final_declaration: bool,
    pub(crate) abstract_declaration: bool,
    pub(crate) effective_mandatory: Option<bool>,
    pub(crate) reference: Option<TypeReference>,
    pub(crate) base_reference: Option<TypeReference>,
    pub(crate) unit: Option<Box<UnitInfo>>,
    pub(crate) oids: Option<Box<OidRules>>,
    pub(crate) topic_dependencies: Option<Box<TopicDependencies>>,
    pub(crate) effective_attributes: Option<Box<[usize]>>,
    pub(crate) association: Option<Box<AssociationInfo>>,
    pub(crate) role: Option<Box<RoleInfo>>,
    pub(crate) generic: bool,
    pub(crate) context: Option<Box<[ContextMapping]>>,
    pub(crate) generic_bindings: Option<Box<[TopicGenericBinding]>>,
}
impl Declaration {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn qualified_name(&self) -> &str {
        &self.qualified
    }
    pub fn kind(&self) -> DeclarationKind {
        self.kind
    }
    pub fn source_span(&self) -> &Span {
        &self.span
    }
    /// Effective definition requirement for attributes or runtime parameters.
    pub fn mandatory(&self) -> Option<bool> {
        self.effective_mandatory
    }
    /// The literal MANDATORY modifier for typed declarations.
    pub fn declared_mandatory(&self) -> Option<bool> {
        self.declared_mandatory
    }
    pub fn is_final(&self) -> bool {
        self.final_declaration
    }
    pub fn is_abstract(&self) -> bool {
        self.abstract_declaration
    }
    pub fn type_reference(&self) -> Option<&TypeReference> {
        self.reference.as_ref().or(self.base_reference.as_ref())
    }
    pub fn base_reference(&self) -> Option<&TypeReference> {
        self.base_reference.as_ref()
    }
    pub fn unit(&self) -> Option<&UnitInfo> {
        self.unit.as_deref()
    }
    pub fn oid_rules(&self) -> Option<&OidRules> {
        self.oids.as_deref()
    }
    pub fn topic_dependencies(&self) -> &[TypeReference] {
        self.topic_dependencies
            .as_ref()
            .map_or(&[], |links| &links.effective)
    }
    pub fn declared_topic_dependencies(&self) -> &[TypeReference] {
        self.topic_dependencies
            .as_ref()
            .map_or(&[], |links| &links.declared)
    }
    pub fn association(&self) -> Option<&AssociationInfo> {
        self.association.as_deref()
    }
    pub fn role(&self) -> Option<&RoleInfo> {
        self.role.as_deref()
    }
    pub fn is_generic(&self) -> bool {
        self.generic
    }
    pub fn context_mappings(&self) -> &[ContextMapping] {
        self.context.as_deref().unwrap_or_default()
    }
    pub fn generic_bindings(&self) -> &[TopicGenericBinding] {
        self.generic_bindings.as_deref().unwrap_or_default()
    }
}
/// Read-only ordered data. Constructed only after all declared subset checks pass.
#[derive(Debug)]
pub struct ModelGraph {
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) types: Vec<TypeInfo>,
    pub(crate) names: BTreeMap<String, usize>,
}
impl ModelGraph {
    pub fn models(&self) -> &[ModelInfo] {
        &self.models
    }
    pub fn declarations(&self) -> &[Declaration] {
        &self.declarations
    }
    /// Exact full qualified-name lookup. A type takes precedence over a same-name
    /// runtime parameter; use `runtime_parameter` for its separate namespace.
    pub fn declaration(&self, name: &str) -> Option<&Declaration> {
        self.names
            .get(name)
            .and_then(|&id| self.declarations.get(id))
    }
    /// Direct members in source declaration order. Non-container/unknown names return None.
    /// An existing empty model/topic/class/structure returns Some(empty).
    pub fn members(&self, name: &str) -> Option<Vec<&Declaration>> {
        let &id = self.names.get(name)?;
        if !matches!(
            self.declarations[id].kind,
            DeclarationKind::Model
                | DeclarationKind::Topic
                | DeclarationKind::Class
                | DeclarationKind::Structure
                | DeclarationKind::Association
        ) {
            return None;
        }
        Some(
            self.declarations
                .iter()
                .filter(|d| d.parent == Some(id))
                .collect(),
        )
    }
    pub fn effective_type(&self, name: &str) -> Option<&TypeInfo> {
        self.declaration(name)?
            .type_id
            .and_then(|id| self.types.get(id))
    }
    pub fn collection_element_type(&self, name: &str) -> Option<&TypeInfo> {
        let TypeInfo::Collection(collection) = self.effective_type(name)? else {
            return None;
        };
        self.types.get(collection.element)
    }

    /// Effective attributes in transfer order, including inherited declarations.
    pub fn effective_attributes(&self, name: &str) -> Option<impl Iterator<Item = &Declaration>> {
        let declaration = self.declaration(name)?;
        if !matches!(
            declaration.kind,
            DeclarationKind::Class | DeclarationKind::Structure | DeclarationKind::Association
        ) {
            return None;
        }
        Some(
            declaration
                .effective_attributes
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|&id| &self.declarations[id]),
        )
    }
    pub fn effective_roles(&self, name: &str) -> Option<impl Iterator<Item = &Declaration>> {
        let association = self.declaration(name)?.association()?;
        Some(association.roles.iter().map(|&id| &self.declarations[id]))
    }

    /// Looks up a runtime parameter by its qualified name in the member namespace.
    pub fn runtime_parameter(&self, name: &str) -> Option<(&Declaration, &TypeInfo)> {
        let declaration = self
            .declarations
            .iter()
            .find(|d| d.kind == DeclarationKind::RuntimeParameter && d.qualified == name)?;
        Some((declaration, self.types.get(declaration.type_id?)?))
    }
}
