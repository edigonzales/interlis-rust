#![forbid(unsafe_code)]
use interlis::{AnalysisStatus, CompileRequest, Limits, SourceInput};
use std::{collections::HashSet, ffi::OsString, io::Read, path::PathBuf, process::ExitCode};

const HELP: &str = "interlis-compiler (experimental; no full INTERLIS validation)\n\
Usage:\n  interlis-compiler help\n  interlis-compiler version\n  interlis-compiler probe [--json] FILE.ili [FILE.ili ...]\n  interlis-compiler probe --request FILE.json --json\n";
const MAX_HOST_BYTES: u64 = 32 * 1024 * 1024;

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
    let file = std::fs::File::open(path).map_err(operational)?;
    let mut bytes = Vec::new();
    file.take(MAX_HOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(operational)?;
    if bytes.len() as u64 > MAX_HOST_BYTES {
        return Err(operational("File exceeds 32 MiB host read limit"));
    }
    String::from_utf8(bytes).map_err(|_| operational("File is not valid UTF-8"))
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
        Some("probe") => probe(args),
        _ => Err(usage(HELP)),
    }
}

fn probe(mut args: impl Iterator<Item = OsString>) -> Result<u8, CliError> {
    let mut json = false;
    let mut request = None;
    let mut paths = vec![];
    let mut literal_paths = false;
    while let Some(arg) = args.next() {
        if literal_paths {
            paths.push(PathBuf::from(arg));
            continue;
        }
        match arg.to_str() {
            Some("--") => literal_paths = true,
            Some("--json") if !json => json = true,
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
    let report = if let Some(path) = request {
        if !paths.is_empty() || !json {
            return Err(usage("--request requires --json and no source files"));
        }
        interlis::analyze_request_json(&read_bounded(&path)?).map_err(|e| usage(&e.to_string()))?
    } else {
        if paths.is_empty() || paths.len() > 4096 {
            return Err(usage("Expected 1 to 4096 explicit source files"));
        }
        let mut names = HashSet::new();
        let mut sources = vec![];
        let mut total = 0_u64;
        for path in paths {
            let canonical = std::fs::canonicalize(&path).map_err(operational)?;
            if !names.insert(canonical.clone()) {
                return Err(usage("Duplicate physical source file"));
            }
            let name = canonical
                .to_str()
                .ok_or_else(|| usage("Source path must be valid Unicode"))?
                .to_owned();
            let text = read_bounded(&canonical)?;
            total += text.len() as u64;
            if total > MAX_HOST_BYTES {
                return Err(operational("Files exceed 32 MiB host read limit"));
            }
            sources.push(SourceInput { name, text });
        }
        interlis::analyze(CompileRequest {
            sources,
            limits: Limits::default(),
        })
        .map_err(|e| usage(&e.to_string()))?
    };
    if json {
        println!("{}", report.to_json().map_err(operational)?);
    } else {
        println!("{:?} (i0-subset; full validation: false)", report.status());
        for diagnostic in report.diagnostics() {
            eprintln!(
                "{}: {}",
                diagnostic.code,
                diagnostic.message.escape_default()
            );
        }
    }
    Ok(match report.status() {
        AnalysisStatus::CheckedSubset => 0,
        AnalysisStatus::Rejected => 1,
        AnalysisStatus::Incomplete | AnalysisStatus::LimitExceeded => 4,
    })
}
