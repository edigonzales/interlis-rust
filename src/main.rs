#![forbid(unsafe_code)]
use interlis::{
    AnalysisStatus, CompilationStatus, CompileRequest, Limits, SourceInput, SourceSet,
    SyntaxStatus,
    repository::{self, LocalSource},
};
use std::{
    collections::HashSet,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "interlis-compiler (experimental)\n\
Usage:\n  interlis-compiler help\n  interlis-compiler version\n  interlis-compiler check [--json] [--root MODEL ...] [FILE.ili ...]\n  interlis-compiler compile [--json] [--root MODEL ...] [FILE.ili ...]\n  interlis-compiler compile --request FILE.json --json\n  interlis-compiler probe [--json] [--root MODEL ...] FILE.ili [FILE.ili ...]\n  interlis-compiler probe --request FILE.json --json\n  interlis-compiler parse [--json] FILE.ili [FILE.ili ...]\n  interlis-compiler parse --request FILE.json --json\n\n\
Repository options for check and compile:\n\
  --repository LOCATION   Search a directory, base URL or XML index (repeatable)\n\
  --repositories LIST     Search a semicolon-separated list of locations\n\
  --cache DIRECTORY       Use this persistent repository cache\n\
  --no-cache              Disable the persistent repository cache\n\
  --offline               Use local files and verified cached HTTP sources\n";
const MAX_HOST_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug)]
struct CliError {
    exit: u8,
    message: String,
}
fn usage(message: &str) -> CliError {
    CliError {
        exit: 2,
        message: message.into(),
    }
}
fn operational(error: impl std::fmt::Display) -> CliError {
    CliError {
        exit: 3,
        message: error.to_string().escape_default().to_string(),
    }
}

fn input_error(path: &Path, error: impl std::fmt::Display) -> CliError {
    operational(format!("{}: {error}", path.display()))
}

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::from(error.exit)
        }
    }
}

fn read_bounded(path: &PathBuf) -> Result<String, CliError> {
    if !std::fs::metadata(path)
        .map_err(|error| input_error(path, error))?
        .is_file()
    {
        return Err(input_error(path, "Expected a regular input file"));
    }
    let file = std::fs::File::open(path).map_err(|error| input_error(path, error))?;
    read_file_bounded(&file, path)
}

fn read_file_bounded(file: &std::fs::File, path: &Path) -> Result<String, CliError> {
    if !file
        .metadata()
        .map_err(|error| input_error(path, error))?
        .is_file()
    {
        return Err(input_error(path, "Expected a regular input file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_HOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| input_error(path, error))?;
    if bytes.len() as u64 > MAX_HOST_BYTES {
        return Err(input_error(path, "File exceeds 32 MiB host read limit"));
    }
    String::from_utf8(bytes).map_err(|_| input_error(path, "File is not valid UTF-8"))
}

fn logical_source_name(path: &Path, ordinal: usize) -> Result<String, CliError> {
    path.to_str()
        .ok_or_else(|| usage("Source path must be valid Unicode"))?;
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| usage("Expected a source filename"))?;
    let mut name = format!("cli:///{ordinal}/");
    use std::fmt::Write;
    for character in basename.chars() {
        let mut buffer = [0; 4];
        let bytes = character.encode_utf8(&mut buffer).as_bytes();
        let unreserved =
            bytes.len() == 1 && (bytes[0].is_ascii_alphanumeric() || b"-._~".contains(&bytes[0]));
        let encoded_len = if unreserved { 1 } else { 3 * bytes.len() };
        // Reserve one byte for a visible truncation marker. Ordinals retain
        // uniqueness even when displayed basenames have the same prefix.
        if name.len() + encoded_len > 1023 {
            name.push('~');
            break;
        }
        for &byte in bytes {
            if unreserved {
                name.push(char::from(byte));
            } else {
                write!(name, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        }
    }
    Ok(name)
}

fn read_sources(paths: &[PathBuf]) -> Result<Vec<LocalSource>, CliError> {
    let mut ordered_paths = paths
        .iter()
        .map(|path| {
            std::fs::canonicalize(path)
                .map(|canonical| (canonical, path))
                .map_err(|error| input_error(path, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ordered_paths.sort_by(|a, b| a.0.cmp(&b.0));
    let mut identities = HashSet::new();
    let mut sources = Vec::new();
    let mut total = 0_u64;
    for (index, (canonical, path)) in ordered_paths.iter().enumerate() {
        let name = logical_source_name(path, index + 1)?;
        // Keep the open identity through this read set: symlinks and hardlinks
        // are duplicates too. Host paths never become logical source names.
        if !std::fs::metadata(canonical)
            .map_err(|error| input_error(path, error))?
            .is_file()
        {
            return Err(input_error(path, "Expected a regular source file"));
        }
        let handle =
            same_file::Handle::from_path(canonical).map_err(|error| input_error(path, error))?;
        if identities.contains(&handle) {
            return Err(usage("Duplicate physical source file"));
        }
        let text = read_file_bounded(handle.as_file(), path)?;
        identities.insert(handle);
        total += text.len() as u64;
        if total > MAX_HOST_BYTES {
            return Err(operational("Files exceed 32 MiB host read limit"));
        }
        sources.push(LocalSource {
            path: canonical.clone(),
            source: SourceInput { name, text },
        });
    }
    Ok(sources)
}

fn render_diagnostic(sources: &SourceSet, diagnostic: &interlis::Diagnostic) -> String {
    let place = diagnostic
        .primary
        .as_ref()
        .and_then(|span| sources.location(span));
    let prefix = place
        .map(|p| {
            format!(
                "{}:{}:{}: ",
                p.source.escape_default(),
                p.start_line,
                p.start_column_utf16
            )
        })
        .unwrap_or_default();
    format!(
        "{prefix}{}: {}",
        diagnostic.code,
        diagnostic.message.escape_default()
    )
}

fn run(mut args: impl Iterator<Item = OsString>) -> Result<u8, CliError> {
    let Some(command) = args.next() else {
        return Err(usage(HELP));
    };
    match command.to_str() {
        Some("help" | "--help" | "-h") => {
            if args.next().is_some() {
                return Err(usage("Unexpected arguments"));
            }
            print!("{HELP}");
            Ok(0)
        }
        Some("version" | "--version") => {
            if args.next().is_some() {
                return Err(usage("Unexpected arguments"));
            }
            println!("{}", interlis::compiler_version());
            Ok(0)
        }
        Some("compile") => run_request(args, Operation::Compile),
        Some("check") => run_request(args, Operation::Check),
        Some("probe") => run_request(args, Operation::Probe),
        Some("parse") => run_request(args, Operation::Parse),
        _ => Err(usage(HELP)),
    }
}

enum Input {
    Json(String),
    Files(Vec<LocalSource>),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Operation {
    Compile,
    Check,
    Probe,
    Parse,
}

fn run_request(
    mut args: impl Iterator<Item = OsString>,
    operation: Operation,
) -> Result<u8, CliError> {
    let mut json = false;
    let mut request = None;
    let mut paths = vec![];
    let mut roots = vec![];
    let mut repositories = repository::Options::default();
    let semantic = matches!(operation, Operation::Check | Operation::Compile);
    let mut literal_paths = false;
    while let Some(arg) = args.next() {
        if literal_paths {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => literal_paths = true,
            Some("--json") if !json => json = true,
            Some("--root") if operation != Operation::Parse => {
                roots.push(
                    args.next()
                        .and_then(|arg| arg.into_string().ok())
                        .ok_or_else(|| usage("--root requires a model name"))?,
                );
            }
            Some("--repository" | "--repositories") if semantic => {
                let value = args
                    .next()
                    .and_then(|arg| arg.into_string().ok())
                    .ok_or_else(|| usage("A repository option requires a location"))?;
                if arg == "--repositories" {
                    repositories
                        .repositories
                        .extend(value.split(';').map(|entry| entry.trim().to_owned()));
                } else {
                    repositories.repositories.push(value);
                }
            }
            Some("--cache") if semantic && repositories.cache.is_none() => {
                repositories.cache = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("--cache requires a directory"))?,
                ));
            }
            Some("--no-cache") if semantic && !repositories.no_cache => {
                repositories.no_cache = true
            }
            Some("--offline") if semantic && !repositories.offline => repositories.offline = true,
            Some("--request") if request.is_none() => {
                request = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| usage("--request requires a file"))?,
                ))
            }
            Some(s) if s.starts_with('-') => return Err(usage("Unknown or repeated option")),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let input = if let Some(path) = request {
        if !paths.is_empty()
            || !roots.is_empty()
            || !json
            || operation == Operation::Check
            || !repositories.repositories.is_empty()
            || repositories.cache.is_some()
            || repositories.offline
            || repositories.no_cache
        {
            return Err(usage(
                "--request requires --json without source, root or repository arguments",
            ));
        }
        Input::Json(read_bounded(&path)?)
    } else {
        if paths.len() > 4096 || paths.is_empty() && (!semantic || roots.is_empty()) {
            return Err(usage("Expected source files or a named root model"));
        }
        Input::Files(read_sources(&paths)?)
    };
    let explicit = |files: Vec<LocalSource>| CompileRequest {
        sources: files.into_iter().map(|file| file.source).collect(),
        limits: Limits::default(),
    };
    if operation == Operation::Parse {
        let report = match input {
            Input::Json(text) => interlis::parse_request_json(&text),
            Input::Files(files) => interlis::parse(explicit(files)),
        }
        .map_err(|e| usage(&e.to_string()))?;
        if json {
            println!("{}", report.to_json().map_err(operational)?);
        } else {
            if report.status() == SyntaxStatus::Parsed {
                println!("Syntax parsed. Semantic validity was not checked.");
            }
            for diagnostic in report.diagnostics() {
                eprintln!("{}", render_diagnostic(report.sources(), diagnostic));
            }
        }
        return Ok(match report.status() {
            SyntaxStatus::Parsed => 0,
            SyntaxStatus::Invalid => 1,
            SyntaxStatus::UnsupportedVersion | SyntaxStatus::LimitExceeded => 4,
        });
    }
    if semantic {
        let report = match input {
            Input::Json(text) => interlis::compile_request_json(&text),
            Input::Files(files) => repository::compile(
                files,
                (!roots.is_empty()).then_some(roots.as_slice()),
                repositories,
            ),
        }
        .map_err(|e| usage(&e.to_string()))?;
        if json {
            let output = if operation == Operation::Check {
                repository::check_json(&report)
            } else {
                report.to_json()
            };
            println!("{}", output.map_err(operational)?);
        } else {
            if operation == Operation::Compile {
                println!("{:?}", report.status());
            }
            for diagnostic in report.diagnostics() {
                eprintln!("{}", render_diagnostic(report.sources(), diagnostic));
            }
        }
        return Ok(match report.status() {
            CompilationStatus::Compiled => 0,
            CompilationStatus::Rejected => 1,
            CompilationStatus::InternalError => 3,
            CompilationStatus::Incomplete => 4,
            CompilationStatus::LimitExceeded => 5,
        });
    }
    let report = match input {
        Input::Json(text) => interlis::analyze_request_json(&text),
        Input::Files(files) if !roots.is_empty() => {
            interlis::analyze_roots(explicit(files), &roots)
        }
        Input::Files(files) => interlis::analyze(explicit(files)),
    }
    .map_err(|e| usage(&e.to_string()))?;
    if json {
        println!("{}", report.to_json().map_err(operational)?);
    } else {
        println!("{:?} (i0-subset; full validation: false)", report.status());
        for diagnostic in report.diagnostics() {
            eprintln!("{}", render_diagnostic(report.sources(), diagnostic));
        }
    }
    Ok(match report.status() {
        AnalysisStatus::CheckedSubset => 0,
        AnalysisStatus::Rejected => 1,
        AnalysisStatus::Incomplete | AnalysisStatus::LimitExceeded => 4,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_windows_length_names_fit_logical_limit_without_splitting_characters() {
        for basename in [
            format!("{}.ili", "界".repeat(200)),
            format!("{}.ili", "😀".repeat(120)),
        ] {
            let name = logical_source_name(Path::new(&basename), 4096).unwrap();
            assert!(name.len() <= 1024);
            assert!(name.ends_with('~'));
            let encoded_character = if basename.starts_with('界') {
                "%E7%95%8C"
            } else {
                "%F0%9F%98%80"
            };
            let body = name
                .strip_prefix("cli:///4096/")
                .unwrap()
                .strip_suffix('~')
                .unwrap();
            assert!(!body.is_empty());
            assert_eq!(
                body,
                encoded_character.repeat(body.len() / encoded_character.len())
            );
        }
    }
}
