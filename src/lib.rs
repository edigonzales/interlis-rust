//! Experimental INTERLIS analysis. No request currently receives full validation.
#![forbid(unsafe_code)]

mod diagnostic;
mod keywords;
mod lexer;
mod limits;
mod source;
mod syntax;
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod wasm;
mod wire;

pub use limits::Limits;
pub use source::{Position, SourceFile, SourceInput, SourceLocation, SourceSet, Span};
pub use wire::{analyze_json, analyze_request_json};

use serde::{Deserialize, Serialize};

/// Complete owned input for one isolated synchronous analysis; no host I/O occurs.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompileRequest {
    pub sources: Vec<SourceInput>,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisStatus {
    CheckedSubset,
    Rejected,
    Incomplete,
    LimitExceeded,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug)]
pub struct RelatedLocation {
    pub span: Span,
    pub message: String,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Option<Span>,
    pub related: Vec<RelatedLocation>,
}

/// An owned report. Sources and diagnostics remain readable after analysis ends.
#[derive(Debug)]
pub struct AnalysisReport {
    sources: SourceSet,
    status: AnalysisStatus,
    diagnostics: Vec<Diagnostic>,
}

impl AnalysisReport {
    pub fn status(&self) -> AnalysisStatus {
        self.status
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn sources(&self) -> &SourceSet {
        &self.sources
    }
    /// Canonical host projection, shared by native CLI and WASM.
    pub fn to_json(&self) -> Result<String, RequestError> {
        wire::encode_report(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError {
    code: &'static str,
    message: &'static str,
}

impl RequestError {
    pub(crate) fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
    pub fn code(&self) -> &str {
        self.code
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RequestError {}

pub const fn compiler_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Parses the supported syntax with deterministic budgets. Semantic checking is
/// not implemented yet; syntax success remains incomplete. Never performs host I/O.
pub fn analyze(request: CompileRequest) -> Result<AnalysisReport, RequestError> {
    request.limits.validate()?;
    let mut sources = SourceSet::new(request.sources)?;
    let mut diagnostics = diagnostic::Diagnostics::new(&request.limits);
    let mut total = Some(0_u64);
    let mut over = false;
    for file in sources.files() {
        let len = u64::try_from(file.text().len())
            .map_err(|_| RequestError::new("REQUEST_SOURCE_SIZE", "Unaddressable source"))?;
        total = total.and_then(|n| n.checked_add(len));
        over |= len > u64::from(request.limits.max_source_bytes);
    }
    over |= total.is_none_or(|n| n > u64::from(request.limits.max_total_source_bytes));
    if over {
        diagnostics.push("ILI-LIMIT", "Source byte budget exceeded", None);
    } else {
        sources.index_lines();
        let mut remaining = request.limits.max_tokens;
        for file in 0..sources.files().len() {
            let document = syntax::parse(
                &sources,
                file,
                &request.limits,
                &mut remaining,
                &mut diagnostics,
            );
            if diagnostics.limited {
                break;
            }
            if let Some(document) = document {
                // Syntax alone never certifies even the supported semantic subset.
                if !diagnostics.rejected {
                    diagnostics.push(
                        "ILI-UNSUPPORTED",
                        "Semantic analysis is not implemented yet",
                        Some(document.span),
                    );
                }
            }
        }
    }
    let status = if diagnostics.limited {
        AnalysisStatus::LimitExceeded
    } else if diagnostics.incomplete {
        AnalysisStatus::Incomplete
    } else if diagnostics.rejected {
        AnalysisStatus::Rejected
    } else {
        AnalysisStatus::Incomplete
    };
    diagnostics.entries.sort_by(|a, b| {
        let location = |d: &Diagnostic| d.primary.as_ref().map(Span::sort_key);
        location(a)
            .cmp(&location(b))
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.message.cmp(&b.message))
    });
    Ok(AnalysisReport {
        sources,
        status,
        diagnostics: diagnostics.entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(text: &str) -> CompileRequest {
        CompileRequest {
            sources: vec![SourceInput {
                name: "memory:///a".into(),
                text: text.into(),
            }],
            limits: Limits::default(),
        }
    }

    #[test]
    fn no_unsupported_source_claims_validation() {
        for text in ["", "INTERLIS 2.4; MODEL A AT \"x\" VERSION \"1\" = END A."] {
            let report = analyze(request(text)).unwrap();
            assert_eq!(report.status(), AnalysisStatus::Incomplete);
            assert_eq!(report.diagnostics()[0].code, "ILI-UNSUPPORTED");
            assert_eq!(report.sources().files()[0].text(), text);
        }
    }

    #[test]
    fn source_budgets_are_exact_and_total_and_do_not_build_indices() {
        let mut input = request("ä");
        input.limits.max_source_bytes = 1;
        let result = analyze(input).unwrap();
        assert_eq!(result.status(), AnalysisStatus::LimitExceeded);
        assert_eq!(result.diagnostics()[0].code, "ILI-LIMIT");
        assert!(result.diagnostics()[0].primary.is_none());
        let mut input = request("ä");
        input.limits.max_source_bytes = 2;
        assert_eq!(analyze(input).unwrap().status(), AnalysisStatus::Incomplete);
        let mut input = request("aa");
        input.limits.max_total_source_bytes = 3;
        input.sources.push(SourceInput {
            name: "b".into(),
            text: "bb".into(),
        });
        assert_eq!(
            analyze(input).unwrap().status(),
            AnalysisStatus::LimitExceeded
        );
    }

    #[test]
    fn invalid_budgets_and_empty_input_are_request_errors() {
        let mut input = request("");
        input.limits.max_nesting = 0;
        assert_eq!(analyze(input).unwrap_err().code(), "REQUEST_LIMITS");
        let mut input = request("");
        input.sources.clear();
        assert_eq!(analyze(input).unwrap_err().code(), "REQUEST_SOURCES");
    }

    #[test]
    fn native_parallel_and_subsequent_requests_are_independent() {
        let handles: Vec<_> = ["one", "two", "three"]
            .into_iter()
            .map(|text| {
                std::thread::spawn(move || {
                    let report = analyze(request(text)).unwrap();
                    assert_eq!(report.sources().files()[0].text(), text);
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(
            analyze(request("after")).unwrap().sources().files()[0].text(),
            "after"
        );
    }
}
