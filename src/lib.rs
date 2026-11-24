//! INTERLIS 2.3 and 2.4 parsing and model compilation.
#![forbid(unsafe_code)]

mod diagnostic;
mod keywords;
mod lexer;
mod limits;
mod model;
mod number;
mod semantic;
mod source;
mod syntax;
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod wasm;
mod wire;

pub use limits::Limits;
pub use model::{
    AlignmentKind, AssociationInfo, AssociationPathStep, AttributePathOwner, AttributePathType,
    AttributeSubdivision, AxisRotation, BinaryOperator, BlackboxKind, BoundConstraint,
    BoundExpression, BoundExpressionKind, BoundPathStep, Cardinality, ClassValueKind,
    ClassValueType, CollectionKind, CollectionType, ConstraintRule, ContextMapping, CoordinateKind,
    CoordinateType, DecimalInfo, Declaration, DeclarationKind, DocumentationComment,
    DomainReferenceSystem, Enumeration, EnumerationElement, EnumerationOrder, ExistenceTarget,
    ExpressionId, ExpressionType, FixedTextFormat, FormattedBound, FormattedPart, FormattedType,
    FunctionInfo, GeometryKind, GeometryType, InspectionInfo, LanguageVersion, LineForm,
    MetaAttribute, MetadataBasketKind, ModelGraph, ModelInfo, ModelKind, NumericBounds,
    NumericDirection, NumericRange, NumericReferenceSystem, ObjectSelection, ObjectTarget, OidKind,
    OidRule, OidRules, OidType, OverlapTolerance, PathSelection, PlausibilityDirection,
    ReferenceType, RoleInfo, RoleStrength, SourceMetadata, StructureType, TemporalBound,
    TemporalKind, TemporalRange, TopicGenericBinding, TypeInfo, TypeReference, UnaryOperator,
    UnitConstant, UnitDefinition, UnitFactor, UnitInfo, UnitOperator, UnitScale, UnitTerm,
    ViewFormation, ViewInfo,
};
pub use model::{
    DrawingAssignment, DrawingCase, DrawingClause, DrawingRuleInfo, DrawingValue, GraphicInfo,
};
pub use source::{Position, SourceFile, SourceInput, SourceLocation, SourceSet, Span};
pub use wire::{
    analyze_json, analyze_request_json, compile_json, compile_request_json, parse_json,
    parse_request_json,
};

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
pub enum CompilationStatus {
    Compiled,
    Rejected,
    Incomplete,
    LimitExceeded,
    InternalError,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxStatus {
    Parsed,
    Invalid,
    UnsupportedVersion,
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
    graph: Option<ModelGraph>,
    unsupported_version: bool,
    internal_error: bool,
}

impl AnalysisReport {
    pub fn status(&self) -> AnalysisStatus {
        self.status
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn graph(&self) -> Option<&ModelGraph> {
        self.graph.as_ref()
    }
    pub fn sources(&self) -> &SourceSet {
        &self.sources
    }
    /// Canonical host projection, shared by native CLI and WASM.
    pub fn to_json(&self) -> Result<String, RequestError> {
        wire::encode_report(self)
    }
}

/// A model graph produced by a successful semantic compilation.
#[derive(Debug)]
pub struct CompiledSchema {
    graph: ModelGraph,
}

impl CompiledSchema {
    pub fn graph(&self) -> &ModelGraph {
        &self.graph
    }
}

#[derive(Debug)]
pub struct CompilationReport {
    sources: SourceSet,
    diagnostics: Vec<Diagnostic>,
    status: CompilationStatus,
    schema: Option<CompiledSchema>,
}

impl CompilationReport {
    pub fn status(&self) -> CompilationStatus {
        self.status
    }
    pub fn schema(&self) -> Option<&CompiledSchema> {
        self.schema.as_ref()
    }
    pub fn sources(&self) -> &SourceSet {
        &self.sources
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn to_json(&self) -> Result<String, RequestError> {
        wire::encode_compilation_report(self)
    }
}

/// An owned syntax result with original sources and diagnostics.
#[derive(Debug)]
pub struct SyntaxReport {
    sources: SourceSet,
    status: SyntaxStatus,
    diagnostics: Vec<Diagnostic>,
    documents: Option<Vec<syntax::Document>>,
}

impl SyntaxReport {
    pub fn status(&self) -> SyntaxStatus {
        self.status
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn sources(&self) -> &SourceSet {
        &self.sources
    }

    /// Encodes diagnostics and document summaries using original source positions.
    pub fn to_json(&self) -> Result<String, RequestError> {
        wire::encode_syntax_report(self)
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

struct Frontend {
    sources: SourceSet,
    documents: Vec<syntax::Document>,
    diagnostics: diagnostic::Diagnostics,
    failed_sources: Vec<FailedSource>,
}

struct FailedSource {
    models: Vec<(String, Option<String>)>,
    diagnostics: diagnostic::Diagnostics,
}

fn retain_unaffected_models(
    document: &mut syntax::Document,
    diagnostics: &diagnostic::Diagnostics,
) -> Option<Vec<(String, Option<String>)>> {
    let mut affected = std::collections::BTreeSet::new();
    for diagnostic in &diagnostics.entries {
        let (_, at, end) = diagnostic.primary.as_ref()?.sort_key();
        let index = document
            .models
            .partition_point(|model| model.span.sort_key().2 <= at);
        let (_, start, stop) = document.models.get(index)?.span.sort_key();
        if at < start || end > stop {
            return None;
        }
        affected.insert(index);
    }
    let mut failures = Vec::new();
    let mut position = 0;
    document.models.retain(|model| {
        let failed = affected.contains(&position);
        position += 1;
        if failed {
            failures.push((model.name.value.clone(), Some(model.version.value.clone())));
        }
        !failed
    });
    Some(failures)
}

impl Frontend {
    fn status(&self) -> SyntaxStatus {
        if self.diagnostics.limited {
            SyntaxStatus::LimitExceeded
        } else if self.diagnostics.syntax_invalid {
            SyntaxStatus::Invalid
        } else if self.diagnostics.unsupported_version {
            SyntaxStatus::UnsupportedVersion
        } else {
            SyntaxStatus::Parsed
        }
    }
}

fn frontend(
    inputs: Vec<SourceInput>,
    limits: &Limits,
    scoped: bool,
) -> Result<Frontend, RequestError> {
    limits.validate()?;
    let mut sources = SourceSet::new(inputs)?;
    let mut diagnostics = diagnostic::Diagnostics::new(limits);
    let mut documents = Vec::new();
    let mut failed_sources = Vec::new();
    let mut total = Some(0_u64);
    let mut over = false;
    for file in sources.files() {
        let len = u64::try_from(file.text().len())
            .map_err(|_| RequestError::new("REQUEST_SOURCE_SIZE", "Unaddressable source"))?;
        total = total.and_then(|n| n.checked_add(len));
        over |= len > u64::from(limits.max_source_bytes);
    }
    over |= total.is_none_or(|n| n > u64::from(limits.max_total_source_bytes));
    if over {
        diagnostics.limit("Source byte budget exceeded", None);
    } else {
        sources.index_lines();
        let mut remaining = limits::SyntaxBudget::new(limits);
        for file in 0..sources.files().len() {
            if scoped {
                let mut local = diagnostic::Diagnostics::new(limits);
                let mut model_headers = Vec::new();
                let mut document = syntax::parse_source(
                    &sources,
                    file,
                    limits,
                    &mut remaining,
                    &mut local,
                    Some(&mut model_headers),
                );
                if local.limited {
                    diagnostics.absorb(local);
                } else if local.syntax_invalid || local.unsupported_version {
                    if !local.unsupported_version
                        && let Some(mut partially_parsed) = document.take()
                        && let Some(affected) =
                            retain_unaffected_models(&mut partially_parsed, &local)
                    {
                        model_headers = affected;
                        documents.push(partially_parsed);
                    }
                    model_headers.sort_unstable();
                    model_headers.dedup();
                    failed_sources.push(FailedSource {
                        models: model_headers,
                        diagnostics: local,
                    });
                } else if let Some(document) = document {
                    documents.push(document);
                }
            } else if let Some(document) =
                syntax::parse(&sources, file, limits, &mut remaining, &mut diagnostics)
            {
                documents.push(document);
            }
            if diagnostics.limited {
                break;
            }
        }
    }
    Ok(Frontend {
        sources,
        documents,
        diagnostics,
        failed_sources,
    })
}

/// Parses the supplied sources as INTERLIS 2.3 or 2.4. Model validity is unchecked.
pub fn parse(request: CompileRequest) -> Result<SyntaxReport, RequestError> {
    let frontend = frontend(request.sources, &request.limits, false)?;
    let status = frontend.status();
    Ok(SyntaxReport {
        status,
        sources: frontend.sources,
        diagnostics: frontend.diagnostics.into_sorted(),
        documents: (status == SyntaxStatus::Parsed).then_some(frontend.documents),
    })
}

/// Checks the declared syntax/semantic subset with deterministic budgets.
/// Returns a provisional graph for the probe contract. No host I/O.
pub fn analyze(request: CompileRequest) -> Result<AnalysisReport, RequestError> {
    analyze_models(request, None, false)
}

/// Analyzes the specified models and their transitive imports. Every supplied
/// document must parse successfully before model selection is applied.
pub fn analyze_roots(
    request: CompileRequest,
    roots: &[String],
) -> Result<AnalysisReport, RequestError> {
    analyze_models(request, Some(roots), false)
}

/// Compiles explicitly supplied sources. With roots, model checking covers
/// those models and their transitive dependencies. Only a successful
/// compilation exposes a schema.
pub fn compile(
    request: CompileRequest,
    roots: Option<&[String]>,
) -> Result<CompilationReport, RequestError> {
    let AnalysisReport {
        sources,
        mut diagnostics,
        status,
        graph,
        unsupported_version,
        internal_error,
    } = analyze_models(request, roots, roots.is_some())?;
    let (status, schema) = match status {
        AnalysisStatus::CheckedSubset => (
            CompilationStatus::Compiled,
            Some(CompiledSchema {
                graph: graph.expect("checked report has a graph"),
            }),
        ),
        AnalysisStatus::LimitExceeded => (CompilationStatus::LimitExceeded, None),
        _ if unsupported_version => {
            for diagnostic in &mut diagnostics {
                if diagnostic.code == "ILI-UNSUPPORTED" {
                    diagnostic.code = "ILI-VERSION".into();
                    diagnostic.severity = Severity::Error;
                }
            }
            (CompilationStatus::Rejected, None)
        }
        AnalysisStatus::Rejected => (CompilationStatus::Rejected, None),
        AnalysisStatus::Incomplete if internal_error => {
            diagnostics.push(Diagnostic {
                code: "ILI-INTERNAL".into(),
                message: "Model checking did not produce a graph or a language diagnostic".into(),
                severity: Severity::Error,
                primary: None,
                related: Vec::new(),
            });
            (CompilationStatus::InternalError, None)
        }
        AnalysisStatus::Incomplete => (CompilationStatus::Incomplete, None),
    };
    Ok(CompilationReport {
        sources,
        diagnostics,
        status,
        schema,
    })
}

fn analyze_models(
    request: CompileRequest,
    roots: Option<&[String]>,
    scoped: bool,
) -> Result<AnalysisReport, RequestError> {
    if let Some(roots) = roots
        && (roots.is_empty()
            || roots.len() > request.limits.max_syntax_nodes as usize
            || roots.iter().any(|name| {
                name.len() > 255
                    || !name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
            || roots
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != roots.len())
    {
        return Err(RequestError::new(
            "REQUEST_ROOTS",
            "Expected a nonempty list of distinct model names",
        ));
    }
    let mut frontend = frontend(request.sources, &request.limits, scoped)?;
    let graph = if frontend.status() == SyntaxStatus::Parsed {
        semantic::analyze(
            &frontend.documents,
            roots,
            &request.limits,
            &mut frontend.diagnostics,
            frontend.failed_sources,
        )
    } else {
        None
    };
    let diagnostics = frontend.diagnostics;

    let status = if diagnostics.limited {
        AnalysisStatus::LimitExceeded
    } else if diagnostics.rejected {
        AnalysisStatus::Rejected
    } else if diagnostics.incomplete {
        AnalysisStatus::Incomplete
    } else if graph.is_some() {
        AnalysisStatus::CheckedSubset
    } else {
        AnalysisStatus::Incomplete
    };
    Ok(AnalysisReport {
        sources: frontend.sources,
        status,
        unsupported_version: diagnostics.unsupported_version,
        internal_error: status == AnalysisStatus::Incomplete && !diagnostics.incomplete,
        diagnostics: diagnostics.into_sorted(),
        graph,
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
        for (text, status, code) in [
            ("", AnalysisStatus::Rejected, "ILI-SYNTAX"),
            (
                "INTERLIS 2.2;",
                AnalysisStatus::Incomplete,
                "ILI-UNSUPPORTED",
            ),
        ] {
            let report = analyze(request(text)).unwrap();
            assert_eq!(report.status(), status);
            assert_eq!(report.diagnostics()[0].code, code);
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
        assert_eq!(analyze(input).unwrap().status(), AnalysisStatus::Rejected);
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
