use crate::{AnalysisReport, CompileRequest, RequestError, SourceLocation};
use crate::{DecimalInfo, ModelGraph, SourceSet, TypeInfo};
use serde::{
    Serialize, Serializer,
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
    if input.len() > MAX_WIRE_BYTES {
        return Err(RequestError::new(
            "REQUEST_WIRE_SIZE",
            "JSON input exceeds 16 MiB",
        ));
    }
    let request: CompileRequest = serde_json::from_str(input)
        .map_err(|_| RequestError::new("REQUEST_JSON", "Invalid JSON request"))?;
    crate::analyze(request)
}

pub(crate) fn encode_report(report: &AnalysisReport) -> Result<String, RequestError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Output<'a> {
        status: crate::AnalysisStatus,
        validation_scope: &'static str,
        full_validation: bool,
        diagnostics: Vec<Diagnostic<'a>>,
        model: Option<GraphOutput<'a>>,
    }
    #[derive(Serialize)]
    struct Related<'a> {
        message: &'a str,
        #[serde(flatten)]
        location: SourceLocation,
    }
    #[derive(Serialize)]
    struct Diagnostic<'a> {
        code: &'a str,
        severity: crate::Severity,
        message: &'a str,
        #[serde(flatten)]
        location: Option<SourceLocation>,
        related: Vec<Related<'a>>,
    }
    let diagnostics = report
        .diagnostics()
        .iter()
        .map(|d| Diagnostic {
            code: &d.code,
            severity: d.severity,
            message: &d.message,
            location: d
                .primary
                .as_ref()
                .and_then(|span| report.sources().location(span)),
            related: d
                .related
                .iter()
                .filter_map(|r| {
                    report.sources().location(&r.span).map(|location| Related {
                        message: &r.message,
                        location,
                    })
                })
                .collect(),
        })
        .collect();
    let output = Output {
        status: report.status(),
        validation_scope: "i0-subset",
        full_validation: false,
        diagnostics,
        model: report.graph().map(|graph| GraphOutput {
            graph,
            sources: report.sources(),
        }),
    };
    let mut writer = ReportWriter {
        bytes: Vec::new(),
        overflow: false,
    };
    if serde_json::to_writer(&mut writer, &output).is_err() {
        return Err(if writer.overflow {
            RequestError::new("REPORT_WIRE_SIZE", "JSON report exceeds 32 MiB")
        } else {
            RequestError::new("REPORT_JSON", "Cannot serialize analysis report")
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
        out.serialize_field("models", &ModelsOutput(self.graph))?;
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
struct ModelsOutput<'a>(&'a ModelGraph);
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
                imports: model.imports(),
            })?;
        }
        seq.end()
    }
}
struct DeclarationsOutput<'a> {
    graph: &'a ModelGraph,
    sources: &'a SourceSet,
}
impl Serialize for DeclarationsOutput<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Reference<'a> {
            declared_name: &'a str,
            target_name: &'a str,
            #[serde(flatten)]
            location: Option<SourceLocation>,
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Declaration<'a> {
            name: &'a str,
            qualified_name: &'a str,
            kind: crate::DeclarationKind,
            mandatory: Option<bool>,
            declared_mandatory: Option<bool>,
            type_reference: Option<Reference<'a>>,
            effective_type: Option<TypeOutput<'a>>,
            #[serde(flatten)]
            location: Option<SourceLocation>,
        }
        let mut seq = serializer.serialize_seq(Some(self.graph.declarations().len()))?;
        for declaration in self.graph.declarations() {
            let reference = declaration.type_reference().map(|r| Reference {
                declared_name: r.declared_name(),
                target_name: r.target_name(),
                location: self.sources.location(r.source_span()),
            });
            seq.serialize_element(&Declaration {
                name: declaration.name(),
                qualified_name: declaration.qualified_name(),
                kind: declaration.kind(),
                mandatory: declaration.mandatory(),
                declared_mandatory: declaration.declared_mandatory(),
                type_reference: reference,
                effective_type: self
                    .graph
                    .effective_type(declaration.qualified_name())
                    .map(TypeOutput),
                location: self.sources.location(declaration.source_span()),
            })?;
        }
        seq.end()
    }
}
struct TypeOutput<'a>(&'a TypeInfo);
impl Serialize for TypeOutput<'_> {
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
        impl<'a> From<&'a DecimalInfo> for Decimal<'a> {
            fn from(d: &'a DecimalInfo) -> Self {
                Self {
                    original: d.original(),
                    coefficient: d.coefficient(),
                    fractional_digits: d.fractional_digits(),
                    written_exponent: d.written_exponent(),
                    uses_exponent: d.uses_exponent(),
                    scale: d.scale(),
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
            },
            Boolean,
            Numeric {
                lower: Decimal<'a>,
                upper: Decimal<'a>,
            },
        }
        match self.0 {
            TypeInfo::Text { max_length } => Type::Text { max_length }.serialize(serializer),
            TypeInfo::Boolean => Type::Boolean.serialize(serializer),
            TypeInfo::Numeric { lower, upper } => Type::Numeric {
                lower: lower.into(),
                upper: upper.into(),
            }
            .serialize(serializer),
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
