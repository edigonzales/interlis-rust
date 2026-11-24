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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelKind {
    Data,
    Type,
    RefSystem,
    Symbology,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataBasketKind {
    Sign,
    ReferenceSystem,
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
    ViewBase,
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
    pub(crate) kind: ModelKind,
    pub(crate) no_incremental_transfer: bool,
    pub(crate) explanation: Option<String>,
    pub(crate) charset: Option<(String, &'static str)>,
    pub(crate) xml_namespace: Option<String>,
    pub(crate) effective_xml_namespace: String,
    pub(crate) span: Span,
    pub(crate) issuer_span: Span,
    pub(crate) version_span: Span,
    pub(crate) charset_span: Option<Span>,
    pub(crate) xml_namespace_span: Option<Span>,
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
    pub fn kind(&self) -> ModelKind {
        self.kind
    }
    pub fn no_incremental_transfer(&self) -> bool {
        self.no_incremental_transfer
    }
    pub fn explanation(&self) -> Option<&str> {
        self.explanation.as_deref()
    }
    pub fn declared_charset(&self) -> Option<&str> {
        self.charset.as_ref().map(|charset| charset.0.as_str())
    }
    pub fn charset(&self) -> Option<&str> {
        self.charset.as_ref().map(|charset| charset.1)
    }
    pub fn xml_namespace(&self) -> Option<&str> {
        self.xml_namespace.as_deref()
    }
    pub fn effective_xml_namespace(&self) -> &str {
        &self.effective_xml_namespace
    }
    pub fn source_span(&self) -> &Span {
        &self.span
    }
    pub fn issuer_span(&self) -> &Span {
        &self.issuer_span
    }
    pub fn version_span(&self) -> &Span {
        &self.version_span
    }
    pub fn charset_span(&self) -> Option<&Span> {
        self.charset_span.as_ref()
    }
    pub fn xml_namespace_span(&self) -> Option<&Span> {
        self.xml_namespace_span.as_ref()
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
    pub(crate) computed: bool,
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
    pub fn is_computed(&self) -> bool {
        self.computed
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
    TemporalRange(TemporalRange),
    Formatted(Box<FormattedType>),
    Blackbox(BlackboxKind),
    Enumeration {
        values: Enumeration,
        order: Option<EnumerationOrder>,
        declared_order: Option<EnumerationOrder>,
    },
    EnumTree {
        domain: Option<String>,
        base: usize,
    },
    EnumArgument {
        tree_values: bool,
    },
    Numeric(NumericRange),
    Coordinate(CoordinateType),
    Oid(OidType),
    Structure(StructureType),
    ClassValue(ClassValueType),
    MetaObject(Option<TypeReference>),
    AttributePath(AttributePathType),
    Collection(CollectionType),
    Reference(ReferenceType),
    ObjectSelection(ObjectSelection),
    Geometry(GeometryType),
}

#[derive(Clone, Debug)]
pub enum FormattedPart {
    Literal {
        value: String,
        span: Span,
    },
    Numeric {
        attribute: TypeReference,
        width: Option<String>,
    },
}

#[derive(Debug)]
pub struct FormattedBound {
    pub original: String,
    pub components: Vec<DecimalInfo>,
    pub(crate) span: Span,
}

impl FormattedBound {
    pub fn source_span(&self) -> &Span {
        &self.span
    }
}

#[derive(Debug)]
pub struct FormattedType {
    pub structure: TypeReference,
    pub parts: Vec<FormattedPart>,
    pub bounds: Option<(FormattedBound, FormattedBound)>,
    pub(crate) base: Option<usize>,
    pub(crate) span: Span,
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

#[derive(Clone, Debug)]
pub struct AttributeSubdivision {
    pub continuous: bool,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ExpressionId(pub(crate) u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionType {
    Boolean,
    Numeric,
    Text,
    Temporal,
    Coordinate,
    Geometry,
    Oid,
    Undefined,
    Object,
    ObjectSet,
    Class,
    AttributePath,
    Enumeration,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOperator {
    Implies,
    Or,
    Add,
    Subtract,
    And,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    LessEqual,
    GreaterEqual,
    Less,
    Greater,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOperator {
    Not,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoundExpressionKind {
    Undefined,
    Boolean {
        value: bool,
    },
    Number {
        value: UnitConstant,
        unit: Option<TypeReference>,
    },
    Text {
        value: String,
        #[serde(rename = "canonicalLength")]
        canonical_length: usize,
    },
    Temporal {
        value: String,
        components: Vec<u16>,
    },
    Enumeration {
        path: Vec<String>,
    },
    This,
    ObjectSet {
        explicit: bool,
    },
    Class {
        reference: TypeReference,
    },
    AttributePath {
        reference: TypeReference,
    },
    Path {
        steps: Vec<BoundPathStep>,
    },
    RuntimeParameter {
        reference: TypeReference,
    },
    Call {
        function: TypeReference,
        arguments: Vec<ExpressionId>,
    },
    Unary {
        operator: UnaryOperator,
        operand: ExpressionId,
    },
    Binary {
        operator: BinaryOperator,
        left: ExpressionId,
        right: ExpressionId,
    },
    Group {
        expression: ExpressionId,
    },
    Defined {
        expression: ExpressionId,
    },
}

#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PathSelection {
    First,
    Last,
    Index(String),
    Axis(u8),
}

#[derive(Debug, Serialize)]
pub struct BoundPathStep {
    pub reference: TypeReference,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<PathSelection>,
    #[serde(skip)]
    pub(crate) selection_span: Option<Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub association_access: Option<Box<AssociationPathStep>>,
}

impl BoundPathStep {
    pub fn selection_span(&self) -> Option<&Span> {
        self.selection_span.as_ref()
    }
}

/// A path through an association from an object participating in its source role.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationPathStep {
    pub association: TypeReference,
    pub source_role: TypeReference,
    pub target_role: Option<TypeReference>,
    pub association_object: bool,
    pub cardinality: Cardinality,
    #[serde(skip)]
    pub(crate) type_id: usize,
}

#[derive(Debug)]
pub struct BoundExpression {
    pub value_type: ExpressionType,
    pub kind: BoundExpressionKind,
    pub(crate) span: Span,
    pub(crate) type_id: Option<usize>,
}

impl BoundExpression {
    pub fn source_span(&self) -> &Span {
        &self.span
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlausibilityDirection {
    AtMost,
    AtLeast,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ConstraintRule {
    Domain,
    Mandatory,
    Plausibility {
        direction: PlausibilityDirection,
        percentage: DecimalInfo,
    },
    Unique {
        basket: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        local_prefix: Option<ExpressionId>,
        keys: Vec<ExpressionId>,
    },
    Existence {
        source_path: ExpressionId,
        required: Vec<ExistenceTarget>,
    },
    Set {
        basket: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        filter: Option<ExpressionId>,
    },
}

#[derive(Debug, Serialize)]
pub struct ExistenceTarget {
    pub viewable: TypeReference,
    pub path: ExpressionId,
}

#[derive(Debug)]
pub struct BoundConstraint {
    pub name: Option<String>,
    pub target: Option<TypeReference>,
    pub rule: ConstraintRule,
    pub expression: Option<ExpressionId>,
    pub(crate) span: Span,
    pub(crate) source_metadata: Option<std::sync::Arc<SourceMetadata>>,
}

impl BoundConstraint {
    pub fn source_metadata(&self) -> Option<&SourceMetadata> {
        self.source_metadata.as_deref()
    }
    pub fn source_span(&self) -> &Span {
        &self.span
    }
}

#[derive(Debug)]
pub struct FunctionInfo {
    pub explanation: Option<String>,
    pub(crate) arguments: Box<[usize]>,
    pub(crate) explanation_span: Option<Span>,
    pub(crate) argument_order: Box<[ArgumentBinding]>,
}

#[derive(Debug)]
pub(crate) struct ArgumentBinding {
    pub position: usize,
    pub dependency: Option<usize>,
}

impl FunctionInfo {
    pub fn explanation_span(&self) -> Option<&Span> {
        self.explanation_span.as_ref()
    }
}

impl AttributeSubdivision {
    pub fn source_span(&self) -> &Span {
        &self.span
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassValueKind {
    Class,
    Structure,
}

#[derive(Debug, Serialize)]
pub struct ClassValueType {
    pub kind: ClassValueKind,
    pub restrictions: Vec<TypeReference>,
}

#[derive(Debug)]
pub struct AttributePathType {
    pub owner: Option<AttributePathOwner>,
    pub(crate) restrictions: Vec<AttributeRestriction>,
}

#[derive(Debug)]
pub enum AttributePathOwner {
    Viewable(TypeReference),
    ClassAttribute(ExpressionId),
    Argument(TypeReference),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AttributeRestriction {
    pub ty: usize,
    pub mandatory: bool,
}

#[derive(Debug)]
pub struct TemporalRange {
    pub kind: TemporalKind,
    pub language: LanguageVersion,
    pub base: TypeReference,
    pub lower: TemporalBound,
    pub upper: TemporalBound,
}

#[derive(Debug)]
pub struct TemporalBound {
    pub original: String,
    /// Date fields are year, month, day. Time fields are hour, minute,
    /// second, millisecond. Date-time uses both groups in that order.
    pub components: Vec<u16>,
    pub(crate) span: Span,
}

impl TemporalBound {
    pub fn source_span(&self) -> &Span {
        &self.span
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectTarget {
    pub target: Option<TypeReference>,
    pub restrictions: Vec<TypeReference>,
}

#[derive(Debug, Serialize)]
pub struct ObjectSelection {
    pub plural: bool,
    pub targets: Vec<ObjectTarget>,
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
    pub contexts: Vec<String>,
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

#[derive(Clone, Debug)]
pub struct NumericRange {
    pub bounds: Option<NumericBounds>,
    pub unit: Option<TypeReference>,
    pub reference_system: Option<NumericReferenceSystem>,
    pub circular: bool,
    pub direction: Option<NumericDirection>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DomainReferenceSystem {
    pub domain: TypeReference,
    pub axis: Option<u8>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum NumericReferenceSystem {
    Domain(DomainReferenceSystem),
    MetaObject {
        object: TypeReference,
        axis: Option<u8>,
    },
}

impl NumericReferenceSystem {
    pub fn target(&self) -> &TypeReference {
        match self {
            Self::Domain(reference) => &reference.domain,
            Self::MetaObject { object, .. } => object,
        }
    }
    pub fn axis(&self) -> Option<u8> {
        match self {
            Self::Domain(reference) => reference.axis,
            Self::MetaObject { axis, .. } => *axis,
        }
    }
}

#[derive(Clone, Debug)]
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
    #[serde(skip)]
    pub(crate) source_metadata: Option<std::sync::Arc<SourceMetadata>>,
}

impl EnumerationElement {
    pub fn source_metadata(&self) -> Option<&SourceMetadata> {
        self.source_metadata.as_deref()
    }
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

/// Declared annotations in source order, with their original ranges.
#[derive(Debug, Default)]
pub struct SourceMetadata {
    pub attributes: Vec<MetaAttribute>,
    pub documentation: Vec<DocumentationComment>,
}

impl SourceMetadata {
    pub fn attribute(&self, key: &str) -> Option<&MetaAttribute> {
        self.attributes
            .iter()
            .find(|attribute| attribute.key == key)
    }
}

#[derive(Clone, Debug)]
pub struct MetaAttribute {
    pub key: String,
    pub text: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct DocumentationComment {
    pub text: String,
    pub span: Span,
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
    pub(crate) translation_of: Option<TypeReference>,
    pub(crate) unit: Option<Box<UnitInfo>>,
    pub(crate) oids: Option<Box<OidRules>>,
    pub(crate) topic_dependencies: Option<Box<TopicDependencies>>,
    pub(crate) effective_attributes: Option<Box<[usize]>>,
    pub(crate) subdivision: Option<AttributeSubdivision>,
    pub(crate) transient: bool,
    pub(crate) derivations: Option<Box<[ExpressionId]>>,
    pub(crate) constraints: Option<Box<[BoundConstraint]>>,
    pub(crate) function: Option<Box<FunctionInfo>>,
    pub(crate) association: Option<Box<AssociationInfo>>,
    pub(crate) role: Option<Box<RoleInfo>>,
    pub(crate) generic: bool,
    pub(crate) context: Option<Box<[ContextMapping]>>,
    pub(crate) generic_bindings: Option<Box<[TopicGenericBinding]>>,
    pub(crate) deferred_generics: Option<Box<[TypeReference]>>,
    pub(crate) view: Option<Box<ViewInfo>>,
    pub(crate) view_topic: bool,
    pub(crate) nullable_view_base: bool,
    pub(crate) derived_from: Option<TypeReference>,
    pub(crate) predefined: bool,
    pub(crate) metadata_kind: Option<MetadataBasketKind>,
    pub(crate) source_metadata: Option<std::sync::Arc<SourceMetadata>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewFormation {
    Projection,
    Join,
    Union,
    Aggregation,
    Inspection,
}

#[derive(Clone, Debug)]
pub struct InspectionInfo {
    pub path: ExpressionId,
    pub element: TypeReference,
    pub parent: TypeReference,
    pub(crate) parent_type: Option<usize>,
}

#[derive(Debug)]
pub struct ViewInfo {
    pub formation: ViewFormation,
    pub selections: Vec<ExpressionId>,
    pub grouping: Vec<ExpressionId>,
    pub inspection: Option<InspectionInfo>,
    pub(crate) bases: Vec<usize>,
}

impl Declaration {
    pub fn source_metadata(&self) -> Option<&SourceMetadata> {
        self.source_metadata.as_deref()
    }
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
    pub fn translation_of(&self) -> Option<&TypeReference> {
        self.translation_of.as_ref()
    }
    pub fn unit(&self) -> Option<&UnitInfo> {
        self.unit.as_deref()
    }
    pub fn subdivision(&self) -> Option<&AttributeSubdivision> {
        self.subdivision.as_ref()
    }
    pub fn is_transient(&self) -> bool {
        self.transient
    }
    pub fn is_view_topic(&self) -> bool {
        self.view_topic
    }
    pub fn is_nullable_view_base(&self) -> bool {
        self.nullable_view_base
    }
    pub fn view(&self) -> Option<&ViewInfo> {
        self.view.as_deref()
    }
    pub fn derived_from(&self) -> Option<&TypeReference> {
        self.derived_from.as_ref()
    }
    pub fn is_predefined(&self) -> bool {
        self.predefined
    }
    pub fn metadata_basket_kind(&self) -> Option<MetadataBasketKind> {
        self.metadata_kind
    }
    pub fn derivations(&self) -> &[ExpressionId] {
        self.derivations.as_deref().unwrap_or_default()
    }
    pub fn constraints(&self) -> &[BoundConstraint] {
        self.constraints.as_deref().unwrap_or_default()
    }
    pub fn function(&self) -> Option<&FunctionInfo> {
        self.function.as_deref()
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
    pub fn declared_deferred_generics(&self) -> &[TypeReference] {
        self.deferred_generics.as_deref().unwrap_or_default()
    }
}
/// Read-only ordered data. Constructed only after all declared subset checks pass.
#[derive(Debug)]
pub struct ModelGraph {
    pub(crate) models: Vec<ModelInfo>,
    pub(crate) declarations: Vec<Declaration>,
    pub(crate) types: Vec<TypeInfo>,
    pub(crate) expressions: Vec<BoundExpression>,
    pub(crate) names: BTreeMap<String, usize>,
}
impl ModelGraph {
    pub fn association_path_type(&self, step: &AssociationPathStep) -> Option<&TypeInfo> {
        self.types.get(step.type_id)
    }
    pub fn metadata_objects(&self, name: &str) -> Option<impl Iterator<Item = &Declaration>> {
        let &id = self.names.get(name)?;
        self.declarations[id].metadata_basket_kind()?;
        Some(self.declarations.iter().filter(move |decl| {
            decl.kind == DeclarationKind::MetaObject && decl.parent == Some(id)
        }))
    }
    pub fn formatted_base(&self, format: &FormattedType) -> Option<&FormattedType> {
        let TypeInfo::Formatted(base) = self.types.get(format.base?)? else {
            return None;
        };
        Some(base)
    }
    pub fn view_bases(&self, name: &str) -> Option<impl Iterator<Item = &Declaration>> {
        Some(
            self.declaration(name)?
                .view()?
                .bases
                .iter()
                .map(|&id| &self.declarations[id]),
        )
    }
    pub fn expressions(&self) -> &[BoundExpression] {
        &self.expressions
    }
    pub fn expression(&self, id: ExpressionId) -> Option<&BoundExpression> {
        self.expressions.get(id.0 as usize)
    }
    pub fn expression_declared_type(&self, id: ExpressionId) -> Option<&TypeInfo> {
        self.types.get(self.expression(id)?.type_id?)
    }
    pub fn enum_tree_leaf_type(&self, ty: &TypeInfo) -> Option<&TypeInfo> {
        let TypeInfo::EnumTree { base, .. } = ty else {
            return None;
        };
        self.types.get(*base)
    }
    pub fn attribute_path_restrictions<'a>(
        &'a self,
        attribute: &'a AttributePathType,
    ) -> impl Iterator<Item = (&'a TypeInfo, bool)> + 'a {
        attribute
            .restrictions
            .iter()
            .map(|restriction| (&self.types[restriction.ty], restriction.mandatory))
    }
    pub fn function_arguments(&self, name: &str) -> Option<impl Iterator<Item = &Declaration>> {
        Some(
            self.declaration(name)?
                .function()?
                .arguments
                .iter()
                .map(|&id| &self.declarations[id]),
        )
    }
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
            DeclarationKind::Class
                | DeclarationKind::Structure
                | DeclarationKind::Association
                | DeclarationKind::View
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
