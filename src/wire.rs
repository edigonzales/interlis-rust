use crate::{AnalysisReport, CompileRequest, RequestError, SourceLocation};
use serde::Serialize;

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
        model: Option<()>,
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
    serde_json::to_string(&Output {
        status: report.status(),
        validation_scope: "i0-subset",
        full_validation: false,
        diagnostics,
        model: None,
    })
    .map_err(|_| RequestError::new("REPORT_JSON", "Cannot serialize analysis report"))
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
        let text = "\u{feff}ä😀\r\nx";
        let request = serde_json::json!({"sources":[{"name":"a","text":text}]}).to_string();
        let report: serde_json::Value =
            serde_json::from_str(&analyze_json(&request).unwrap()).unwrap();
        assert_eq!(report["status"], "incomplete");
        assert_eq!(report["fullValidation"], false);
        assert_eq!(report["validationScope"], "i0-subset");
        assert!(report["model"].is_null());
        let d = &report["diagnostics"][0];
        assert_eq!(d["endByte"], 12);
        assert_eq!(d["endLine"], 2);
        assert_eq!(d["endColumnUtf16"], 2);
        let oversized = " ".repeat(MAX_WIRE_BYTES + 1);
        assert_eq!(
            analyze_json(&oversized).unwrap_err().code(),
            "REQUEST_WIRE_SIZE"
        );
    }
}
