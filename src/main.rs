#![forbid(unsafe_code)]
use interlis::{AnalysisStatus, CompileRequest, Limits, SourceInput};
use std::{
    collections::HashSet,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::ExitCode,
};

const HELP: &str = "interlis-compiler (experimental; no full INTERLIS validation)\n\
Usage:\n  interlis-compiler help\n  interlis-compiler version\n  interlis-compiler probe [--json] FILE.ili [FILE.ili ...]\n  interlis-compiler probe --request FILE.json --json\n";
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
    if !std::fs::metadata(path).map_err(operational)?.is_file() {
        return Err(operational("Expected a regular input file"));
    }
    let file = std::fs::File::open(path).map_err(operational)?;
    read_file_bounded(&file)
}

fn read_file_bounded(file: &std::fs::File) -> Result<String, CliError> {
    if !file.metadata().map_err(operational)?.is_file() {
        return Err(operational("Expected a regular input file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_HOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(operational)?;
    if bytes.len() as u64 > MAX_HOST_BYTES {
        return Err(operational("File exceeds 32 MiB host read limit"));
    }
    String::from_utf8(bytes).map_err(|_| operational("File is not valid UTF-8"))
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

fn read_sources(paths: &[PathBuf]) -> Result<Vec<SourceInput>, CliError> {
    let mut ordered_paths = paths
        .iter()
        .map(|path| {
            std::fs::canonicalize(path)
                .map(|canonical| (canonical, path))
                .map_err(operational)
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
        if !std::fs::metadata(canonical).map_err(operational)?.is_file() {
            return Err(operational("Expected a regular source file"));
        }
        let handle = same_file::Handle::from_path(canonical).map_err(operational)?;
        if identities.contains(&handle) {
            return Err(usage("Duplicate physical source file"));
        }
        let text = read_file_bounded(handle.as_file())?;
        identities.insert(handle);
        total += text.len() as u64;
        if total > MAX_HOST_BYTES {
            return Err(operational("Files exceed 32 MiB host read limit"));
        }
        sources.push(SourceInput { name, text });
    }
    Ok(sources)
}

fn render_diagnostic(
    report: &interlis::AnalysisReport,
    diagnostic: &interlis::Diagnostic,
) -> String {
    let place = diagnostic
        .primary
        .as_ref()
        .and_then(|span| report.sources().location(span));
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
        interlis::analyze(CompileRequest {
            sources: read_sources(&paths)?,
            limits: Limits::default(),
        })
        .map_err(|e| usage(&e.to_string()))?
    };
    if json {
        println!("{}", report.to_json().map_err(operational)?);
    } else {
        println!("{:?} (i0-subset; full validation: false)", report.status());
        for diagnostic in report.diagnostics() {
            eprintln!("{}", render_diagnostic(&report, diagnostic));
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
