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
#[derive(Debug)]
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
    pub circular: bool,
    pub direction: Option<NumericDirection>,
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
    pub(crate) topic_dependencies: Option<Box<[TypeReference]>>,
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
        self.topic_dependencies.as_deref().unwrap_or_default()
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

    /// Looks up a runtime parameter by its qualified name in the member namespace.
    pub fn runtime_parameter(&self, name: &str) -> Option<(&Declaration, &TypeInfo)> {
        let declaration = self
            .declarations
            .iter()
            .find(|d| d.kind == DeclarationKind::RuntimeParameter && d.qualified == name)?;
        Some((declaration, self.types.get(declaration.type_id?)?))
    }
}
