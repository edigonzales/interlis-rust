use crate::{AnalysisReport, CompileRequest, RequestError, SourceLocation, SyntaxReport};
use crate::{DecimalInfo, Limits, ModelGraph, SourceInput, SourceSet, TypeInfo};
use serde::{
    Deserialize, Serialize, Serializer,
    ser::{SerializeSeq, SerializeStruct},
};
use std::io::Write;

pub(crate) const MAX_WIRE_BYTES: usize = 16 * 1024 * 1024;

/// Strict JSON request -> shared core -> JSON report. Boundary faults are RequestError;
/// expected language outcomes are normal reports, always fullValidation=false.
pub fn analyze_json(input: &str) -> Result<String, RequestError> {
    encode_report(&analyze_request_json(input)?)
}

/// Strict JSON boundary returning the owned report for native host presentation.
pub fn analyze_request_json(input: &str) -> Result<AnalysisReport, RequestError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AnalysisInput {
        sources: Vec<SourceInput>,
        #[serde(default)]
        limits: Limits,
        #[serde(default)]
        roots: Option<Vec<String>>,
    }
    let input: AnalysisInput = decode_request(input)?;
    let request = CompileRequest {
        sources: input.sources,
        limits: input.limits,
    };
    match input.roots {
        Some(roots) => crate::analyze_roots(request, &roots),
        None => crate::analyze(request),
    }
}

/// Strict JSON request to a bounded JSON syntax report.
pub fn parse_json(input: &str) -> Result<String, RequestError> {
    encode_syntax_report(&parse_request_json(input)?)
}

/// Strict JSON request to an owned syntax report.
pub fn parse_request_json(input: &str) -> Result<SyntaxReport, RequestError> {
    crate::parse(decode_request(input)?)
}

fn decode_request<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, RequestError> {
    if input.len() > MAX_WIRE_BYTES {
        return Err(RequestError::new(
            "REQUEST_WIRE_SIZE",
            "JSON input exceeds 16 MiB",
        ));
    }
    serde_json::from_str(input)
        .map_err(|_| RequestError::new("REQUEST_JSON", "Invalid JSON request"))
}

pub(crate) fn encode_report(report: &AnalysisReport) -> Result<String, RequestError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Output<'a> {
        status: crate::AnalysisStatus,
        validation_scope: &'static str,
        full_validation: bool,
        diagnostics: Vec<DiagnosticOutput<'a>>,
        model: Option<GraphOutput<'a>>,
    }
    bounded_json(&Output {
        status: report.status(),
        validation_scope: "i0-subset",
        full_validation: false,
        diagnostics: diagnostic_output(report.diagnostics(), report.sources()),
        model: report.graph().map(|graph| GraphOutput {
            graph,
            sources: report.sources(),
        }),
    })
}

pub(crate) fn encode_syntax_report(report: &SyntaxReport) -> Result<String, RequestError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Output<'a> {
        status: crate::SyntaxStatus,
        validation_scope: &'static str,
        full_validation: bool,
        diagnostics: Vec<DiagnosticOutput<'a>>,
        documents: Option<DocumentsOutput<'a>>,
    }
    bounded_json(&Output {
        status: report.status(),
        validation_scope: "syntax",
        full_validation: false,
        diagnostics: diagnostic_output(report.diagnostics(), report.sources()),
        documents: report
            .documents
            .as_deref()
            .map(|documents| DocumentsOutput {
                documents,
                sources: report.sources(),
            }),
    })
}

#[derive(Serialize)]
struct RelatedOutput<'a> {
    message: &'a str,
    #[serde(flatten)]
    location: SourceLocation,
}

#[derive(Serialize)]
struct DiagnosticOutput<'a> {
    code: &'a str,
    severity: crate::Severity,
    message: &'a str,
    #[serde(flatten)]
    location: Option<SourceLocation>,
    related: Vec<RelatedOutput<'a>>,
}

fn diagnostic_output<'a>(
    diagnostics: &'a [crate::Diagnostic],
    sources: &SourceSet,
) -> Vec<DiagnosticOutput<'a>> {
    diagnostics
        .iter()
        .map(|d| DiagnosticOutput {
            code: &d.code,
            severity: d.severity,
            message: &d.message,
            location: d.primary.as_ref().and_then(|span| sources.location(span)),
            related: d
                .related
                .iter()
                .filter_map(|r| {
                    sources.location(&r.span).map(|location| RelatedOutput {
                        message: &r.message,
                        location,
                    })
                })
                .collect(),
        })
        .collect()
}

struct DocumentsOutput<'a> {
    documents: &'a [crate::syntax::Document],
    sources: &'a SourceSet,
}

impl Serialize for DocumentsOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Document<'a> {
            source: &'a str,
            language_version: crate::LanguageVersion,
            models: SyntaxModelsOutput<'a>,
        }
        let files = self.sources.files();
        if files.len() != self.documents.len() {
            return Err(serde::ser::Error::custom(
                "Missing document in syntax report",
            ));
        }
        let mut seq = serializer.serialize_seq(Some(self.documents.len()))?;
        for (document, file) in self.documents.iter().zip(files) {
            seq.serialize_element(&Document {
                source: file.name(),
                language_version: document.language,
                models: SyntaxModelsOutput {
                    models: &document.models,
                    sources: self.sources,
                },
            })?;
        }
        seq.end()
    }
}

struct SyntaxModelsOutput<'a> {
    models: &'a [crate::syntax::ModelDecl],
    sources: &'a SourceSet,
}

impl Serialize for SyntaxModelsOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Model<'a> {
            name: &'a str,
            kind: crate::syntax::ModelKind,
            version: &'a str,
            #[serde(flatten)]
            location: SourceLocation,
        }
        let mut seq = serializer.serialize_seq(Some(self.models.len()))?;
        for model in self.models {
            let location = self
                .sources
                .location(&model.span)
                .ok_or_else(|| serde::ser::Error::custom("Invalid model span in syntax report"))?;
            seq.serialize_element(&Model {
                name: &model.name.value,
                kind: model.kind,
                version: &model.version.value,
                location,
            })?;
        }
        seq.end()
    }
}

fn bounded_json(output: &impl Serialize) -> Result<String, RequestError> {
    let mut writer = ReportWriter {
        bytes: Vec::new(),
        overflow: false,
    };
    if serde_json::to_writer(&mut writer, output).is_err() {
        return Err(if writer.overflow {
            RequestError::new("REPORT_WIRE_SIZE", "JSON report exceeds 32 MiB")
        } else {
            RequestError::new("REPORT_JSON", "Cannot serialize report")
        });
    }
    String::from_utf8(writer.bytes)
        .map_err(|_| RequestError::new("REPORT_JSON", "Cannot encode UTF-8 report"))
}

const MAX_REPORT_BYTES: usize = 32 * 1024 * 1024;
struct ReportWriter {
    bytes: Vec<u8>,
    overflow: bool,
}
impl Write for ReportWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(buffer.len())
            .is_none_or(|n| n > MAX_REPORT_BYTES)
        {
            self.overflow = true;
            return Err(std::io::Error::other("Report size budget exceeded"));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct GraphOutput<'a> {
    graph: &'a ModelGraph,
    sources: &'a SourceSet,
}
impl Serialize for GraphOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("ModelGraph", 2)?;
        out.serialize_field("models", &ModelsOutput(self.graph, self.sources))?;
        out.serialize_field(
            "declarations",
            &DeclarationsOutput {
                graph: self.graph,
                sources: self.sources,
            },
        )?;
        out.end()
    }
}
struct ModelsOutput<'a>(&'a ModelGraph, &'a SourceSet);
impl Serialize for ModelsOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Model<'a> {
            name: &'a str,
            language_version: crate::LanguageVersion,
            language_tag: Option<&'a str>,
            issuer: &'a str,
            version: &'a str,
            contracted: bool,
            kind: crate::ModelKind,
            no_incremental_transfer: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            explanation: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            declared_charset: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            charset: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            xml_namespace: Option<&'a str>,
            effective_xml_namespace: &'a str,
            issuer_location: Option<SourceLocation>,
            version_location: Option<SourceLocation>,
            #[serde(skip_serializing_if = "Option::is_none")]
            charset_location: Option<SourceLocation>,
            #[serde(skip_serializing_if = "Option::is_none")]
            xml_namespace_location: Option<SourceLocation>,
            #[serde(flatten)]
            location: Option<SourceLocation>,
            imports: &'a [String],
        }
        let mut seq = serializer.serialize_seq(Some(self.0.models().len()))?;
        for model in self.0.models() {
            seq.serialize_element(&Model {
                name: model.name(),
                language_version: model.language(),
                language_tag: model.language_tag(),
                issuer: model.issuer(),
                version: model.version(),
                contracted: model.is_contracted(),
                kind: model.kind(),
                no_incremental_transfer: model.no_incremental_transfer(),
                explanation: model.explanation(),
                declared_charset: model.declared_charset(),
                charset: model.charset(),
                xml_namespace: model.xml_namespace(),
                effective_xml_namespace: model.effective_xml_namespace(),
                issuer_location: self.1.location(model.issuer_span()),
                version_location: self.1.location(model.version_span()),
                charset_location: model.charset_span().and_then(|span| self.1.location(span)),
                xml_namespace_location: model
                    .xml_namespace_span()
                    .and_then(|span| self.1.location(span)),
                location: self.1.location(model.source_span()),
                imports: model.imports(),
            })?;
        }
        seq.end()
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reference<'a> {
    declared_name: &'a str,
    target_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    predefined_language: Option<crate::LanguageVersion>,
    #[serde(flatten)]
    location: Option<SourceLocation>,
}
fn project_reference<'a>(r: &'a crate::TypeReference, sources: &SourceSet) -> Reference<'a> {
    Reference {
        declared_name: r.declared_name(),
        target_name: r.target_name(),
        predefined_language: r.predefined_language(),
        location: sources.location(r.source_span()),
    }
}
struct DeclarationsOutput<'a> {
    graph: &'a ModelGraph,
    sources: &'a SourceSet,
}
impl Serialize for DeclarationsOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Association<'a> {
            cardinality: &'a crate::Cardinality,
            identified: bool,
            roles: Vec<&'a str>,
        }
        #[derive(Serialize)]
        struct Subdivision {
            continuous: bool,
            #[serde(flatten)]
            location: Option<SourceLocation>,
        }
        #[derive(Serialize)]
        struct Context<'a> {
            generic: Reference<'a>,
            alternatives: Vec<Reference<'a>>,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Declaration<'a> {
            name: &'a str,
            qualified_name: &'a str,
            kind: crate::DeclarationKind,
            mandatory: Option<bool>,
            declared_mandatory: Option<bool>,
            #[serde(rename = "final")]
            final_declaration: bool,
            #[serde(rename = "abstract")]
            abstract_declaration: bool,
            type_reference: Option<Reference<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            base_reference: Option<Reference<'a>>,
            effective_type: Option<TypeOutput<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            unit: Option<&'a crate::UnitInfo>,
            #[serde(skip_serializing_if = "Option::is_none")]
            oid_rules: Option<&'a crate::OidRules>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            topic_dependencies: Vec<Reference<'a>>,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            declared_topic_dependencies: Vec<Reference<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            effective_attributes: Option<Vec<&'a str>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            subdivision: Option<Subdivision>,
            #[serde(skip_serializing_if = "Option::is_none")]
            association: Option<Association<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            role: Option<&'a crate::RoleInfo>,
            #[serde(skip_serializing_if = "is_false")]
            generic: bool,
            #[serde(skip_serializing_if = "Vec::is_empty")]
            context_mappings: Vec<Context<'a>>,
            #[serde(skip_serializing_if = "<[crate::TopicGenericBinding]>::is_empty")]
            generic_bindings: &'a [crate::TopicGenericBinding],
            #[serde(skip_serializing_if = "Vec::is_empty")]
            deferred_generics: Vec<Reference<'a>>,
            #[serde(flatten)]
            location: Option<SourceLocation>,
        }
        fn is_false(value: &bool) -> bool {
            !value
        }
        let mut seq = serializer.serialize_seq(Some(self.graph.declarations().len()))?;
        for declaration in self.graph.declarations() {
            let reference = declaration
                .type_reference()
                .map(|r| project_reference(r, self.sources));
            seq.serialize_element(&Declaration {
                name: declaration.name(),
                qualified_name: declaration.qualified_name(),
                kind: declaration.kind(),
                mandatory: declaration.mandatory(),
                declared_mandatory: declaration.declared_mandatory(),
                final_declaration: declaration.is_final(),
                abstract_declaration: declaration.is_abstract(),
                type_reference: reference,
                base_reference: declaration
                    .base_reference()
                    .map(|r| project_reference(r, self.sources)),
                effective_type: declaration
                    .type_id
                    .and_then(|id| self.graph.types.get(id))
                    .map(|ty| TypeOutput(ty, self.graph, self.sources)),
                unit: declaration.unit(),
                oid_rules: declaration.oid_rules(),
                topic_dependencies: declaration
                    .topic_dependencies()
                    .iter()
                    .map(|reference| project_reference(reference, self.sources))
                    .collect(),
                declared_topic_dependencies: declaration
                    .declared_topic_dependencies()
                    .iter()
                    .map(|reference| project_reference(reference, self.sources))
                    .collect(),
                effective_attributes: declaration.effective_attributes.as_ref().map(|attributes| {
                    attributes
                        .iter()
                        .map(|&id| self.graph.declarations()[id].qualified_name())
                        .collect()
                }),
                subdivision: declaration.subdivision().map(|subdivision| Subdivision {
                    continuous: subdivision.continuous,
                    location: self.sources.location(subdivision.source_span()),
                }),
                association: declaration.association().map(|association| Association {
                    cardinality: &association.cardinality,
                    identified: association.identified,
                    roles: association
                        .roles
                        .iter()
                        .map(|&role| self.graph.declarations()[role].qualified_name())
                        .collect(),
                }),
                role: declaration.role(),
                generic: declaration.is_generic(),
                context_mappings: declaration
                    .context_mappings()
                    .iter()
                    .map(|mapping| Context {
                        generic: project_reference(&mapping.generic, self.sources),
                        alternatives: mapping
                            .alternatives
                            .iter()
                            .map(|reference| project_reference(reference, self.sources))
                            .collect(),
                    })
                    .collect(),
                generic_bindings: declaration.generic_bindings(),
                deferred_generics: declaration
                    .declared_deferred_generics()
                    .iter()
                    .map(|reference| project_reference(reference, self.sources))
                    .collect(),
                location: self.sources.location(declaration.source_span()),
            })?;
        }
        seq.end()
    }
}
impl Serialize for DecimalInfo {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Decimal<'a> {
            original: &'a str,
            coefficient: &'a str,
            fractional_digits: u32,
            written_exponent: i32,
            uses_exponent: bool,
            scale: i32,
        }
        Decimal {
            original: self.original(),
            coefficient: self.coefficient(),
            fractional_digits: self.fractional_digits(),
            written_exponent: self.written_exponent(),
            uses_exponent: self.uses_exponent(),
            scale: self.scale(),
        }
        .serialize(serializer)
    }
}
struct TypeOutput<'a>(&'a TypeInfo, &'a ModelGraph, &'a SourceSet);
impl<'report> Serialize for TypeOutput<'report> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Range<'a> {
            lower: Option<&'a DecimalInfo>,
            upper: Option<&'a DecimalInfo>,
            #[serde(skip_serializing_if = "Option::is_none")]
            unit: Option<&'a crate::TypeReference>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reference_system: Option<&'a crate::DomainReferenceSystem>,
            circular: bool,
            direction: Option<crate::NumericDirection>,
        }
        #[derive(Serialize)]
        struct TemporalBound<'a> {
            original: &'a str,
            components: &'a [u16],
            #[serde(flatten)]
            location: Option<SourceLocation>,
        }
        impl<'a> From<&'a crate::NumericRange> for Range<'a> {
            fn from(range: &'a crate::NumericRange) -> Self {
                Self {
                    lower: range.bounds.as_ref().map(|bounds| &bounds.lower),
                    upper: range.bounds.as_ref().map(|bounds| &bounds.upper),
                    unit: range.unit.as_ref(),
                    reference_system: range.reference_system.as_ref(),
                    circular: range.circular,
                    direction: range.direction,
                }
            }
        }
        #[derive(Serialize)]
        #[serde(
            tag = "kind",
            rename_all = "snake_case",
            rename_all_fields = "camelCase"
        )]
        enum Type<'a> {
            Text {
                max_length: &'a str,
                #[serde(skip_serializing_if = "Option::is_none")]
                format: Option<crate::FixedTextFormat>,
            },
            UnboundedText,
            MultilineText {
                max_length: Option<&'a str>,
            },
            Boolean,
            Alignment {
                orientation: crate::AlignmentKind,
            },
            Temporal {
                format: crate::TemporalKind,
            },
            TemporalRange {
                format: crate::TemporalKind,
                language_version: crate::LanguageVersion,
                base: &'a Reference<'a>,
                lower: &'a TemporalBound<'a>,
                upper: &'a TemporalBound<'a>,
            },
            Blackbox {
                encoding: crate::BlackboxKind,
            },
            Enumeration {
                values: &'a crate::Enumeration,
                order: Option<crate::EnumerationOrder>,
                declared_order: Option<crate::EnumerationOrder>,
            },
            EnumTree {
                domain: &'a str,
            },
            Numeric {
                #[serde(flatten)]
                range: Range<'a>,
            },
            Coordinate {
                coordinate_kind: crate::CoordinateKind,
                axes: Vec<Range<'a>>,
                rotation: Option<crate::AxisRotation>,
                reference_system: Option<&'a str>,
            },
            Oid {
                oid_kind: &'static str,
                #[serde(skip_serializing_if = "Option::is_none")]
                max_length: Option<u8>,
                #[serde(skip_serializing_if = "Option::is_none")]
                carrier: Option<TypeOutput<'a>>,
            },
            Structure {
                target: Option<&'a crate::TypeReference>,
                restrictions: &'a [crate::TypeReference],
            },
            ClassValue {
                value_kind: crate::ClassValueKind,
                restrictions: Vec<Reference<'a>>,
            },
            Collection {
                collection_kind: crate::CollectionKind,
                cardinality: &'a crate::Cardinality,
                element: TypeOutput<'a>,
            },
            Reference {
                external: bool,
                target: &'a crate::ObjectTarget,
            },
            Geometry {
                #[serde(flatten)]
                geometry: &'a crate::GeometryType,
            },
        }
        match self.0 {
            TypeInfo::Text { max_length } => Type::Text {
                max_length,
                format: None,
            }
            .serialize(serializer),
            TypeInfo::FixedText { max_length, format } => Type::Text {
                max_length,
                format: Some(*format),
            }
            .serialize(serializer),
            TypeInfo::UnboundedText => Type::UnboundedText.serialize(serializer),
            TypeInfo::MultilineText { max_length } => Type::MultilineText {
                max_length: max_length.as_deref(),
            }
            .serialize(serializer),
            TypeInfo::Boolean => Type::Boolean.serialize(serializer),
            TypeInfo::Alignment(orientation) => Type::Alignment {
                orientation: *orientation,
            }
            .serialize(serializer),
            TypeInfo::Temporal(format) => Type::Temporal { format: *format }.serialize(serializer),
            TypeInfo::TemporalRange(range) => {
                let bound = |bound: &'report crate::TemporalBound| TemporalBound {
                    original: &bound.original,
                    components: &bound.components,
                    location: self.2.location(bound.source_span()),
                };
                Type::TemporalRange {
                    format: range.kind,
                    language_version: range.language,
                    base: &project_reference(&range.base, self.2),
                    lower: &bound(&range.lower),
                    upper: &bound(&range.upper),
                }
                .serialize(serializer)
            }
            TypeInfo::Blackbox(encoding) => Type::Blackbox {
                encoding: *encoding,
            }
            .serialize(serializer),
            TypeInfo::Enumeration {
                values,
                order,
                declared_order,
            } => Type::Enumeration {
                values,
                order: *order,
                declared_order: *declared_order,
            }
            .serialize(serializer),
            TypeInfo::EnumTree { domain } => Type::EnumTree { domain }.serialize(serializer),
            TypeInfo::Numeric(range) => Type::Numeric {
                range: range.into(),
            }
            .serialize(serializer),
            TypeInfo::Coordinate(coordinate) => Type::Coordinate {
                coordinate_kind: coordinate.kind,
                axes: coordinate.axes.iter().map(Range::from).collect(),
                rotation: coordinate.rotation,
                reference_system: coordinate.reference_system.as_deref(),
            }
            .serialize(serializer),
            TypeInfo::Oid(oid) => {
                let (oid_kind, max_length, carrier) = match oid {
                    crate::OidType::Unstable => ("unstable", None, None),
                    crate::OidType::Any => ("any", None, None),
                    crate::OidType::Typed(carrier) => {
                        ("typed", None, Some(TypeOutput(carrier, self.1, self.2)))
                    }
                    crate::OidType::Standard => ("standard", Some(16), None),
                    crate::OidType::Uuid => ("uuid", Some(36), None),
                };
                Type::Oid {
                    oid_kind,
                    max_length,
                    carrier,
                }
                .serialize(serializer)
            }
            TypeInfo::Structure(structure) => Type::Structure {
                target: structure.target.as_ref(),
                restrictions: &structure.restrictions,
            }
            .serialize(serializer),
            TypeInfo::ClassValue(value) => Type::ClassValue {
                value_kind: value.kind,
                restrictions: value
                    .restrictions
                    .iter()
                    .map(|reference| project_reference(reference, self.2))
                    .collect(),
            }
            .serialize(serializer),
            TypeInfo::Collection(collection) => Type::Collection {
                collection_kind: collection.kind,
                cardinality: &collection.cardinality,
                element: TypeOutput(&self.1.types[collection.element], self.1, self.2),
            }
            .serialize(serializer),
            TypeInfo::Reference(reference) => Type::Reference {
                external: reference.external,
                target: &reference.target,
            }
            .serialize(serializer),
            TypeInfo::Geometry(geometry) => Type::Geometry { geometry }.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_fields_and_encoding_and_types() {
        for input in [
            "",
            "{",
            "{\"sources\":[],\"extra\":0}",
            "{\"sources\":[],\"sources\":[]}",
            "{\"sources\":[{\"name\":\"a\",\"name\":\"b\",\"text\":\"\"}]}",
            "{\"sources\":[{\"name\":\"a\",\"text\":\"\\ud800\"}]}",
            "{\"sources\":[],\"limits\":{\"max_tokens\":1,\"max_tokens\":2}}",
            "{\"sources\":[],\"limits\":{\"max_tokens\":4294967296}}",
        ] {
            assert_eq!(
                analyze_json(input).unwrap_err().code(),
                "REQUEST_JSON",
                "{input}"
            );
        }
    }

    #[test]
    fn exact_unicode_wire_location_and_honest_status() {
        let text = "\u{feff}!! ä😀\r\nINTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = END A.";
        let request = serde_json::json!({"sources":[{"name":"a","text":text}]}).to_string();
        let report: serde_json::Value =
            serde_json::from_str(&analyze_json(&request).unwrap()).unwrap();
        assert_eq!(report["status"], "checked_subset");
        assert_eq!(report["fullValidation"], false);
        assert_eq!(report["validationScope"], "i0-subset");
        assert!(report["diagnostics"].as_array().unwrap().is_empty());
        let d = &report["model"]["declarations"][0];
        assert_eq!(d["startByte"], 28);
        assert_eq!(d["endByte"], 63);
        assert_eq!(d["endLine"], 2);
        assert_eq!(d["endColumnUtf16"], 50);
        let oversized = " ".repeat(MAX_WIRE_BYTES + 1);
        assert_eq!(
            analyze_json(&oversized).unwrap_err().code(),
            "REQUEST_WIRE_SIZE"
        );
    }
}
