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
}
#[derive(Debug)]
pub struct ModelInfo {
    pub(crate) name: String,
    pub(crate) language: LanguageVersion,
    pub(crate) language_tag: Option<String>,
    pub(crate) issuer: String,
    pub(crate) version: String,
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
    Boolean,
    Numeric {
        lower: DecimalInfo,
        upper: DecimalInfo,
    },
}
/// Retains declared name segments, exact bound target identity and original reference range.
#[derive(Debug)]
pub struct TypeReference {
    pub(crate) declared: String,
    pub(crate) target: String,
    pub(crate) span: Span,
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
    pub(crate) effective_mandatory: Option<bool>,
    pub(crate) reference: Option<TypeReference>,
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
    /// Effective definition requirement, only for attributes. Domain derivations contribute.
    pub fn mandatory(&self) -> Option<bool> {
        self.effective_mandatory
    }
    /// The literal MANDATORY modifier, only for domain/attribute declarations.
    pub fn declared_mandatory(&self) -> Option<bool> {
        self.declared_mandatory
    }
    pub fn type_reference(&self) -> Option<&TypeReference> {
        self.reference.as_ref()
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
    /// Exact full qualified-name lookup; never strips prefixes or exposes arena IDs.
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
}
