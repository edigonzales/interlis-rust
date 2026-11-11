use crate::{Limits, SourceSet, Span, diagnostic::Diagnostics, limits::SyntaxBudget, number};
use logos::Logos;
use std::ops::Range;

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\r\n]+")]
enum Raw {
    #[regex(r"[A-Za-z][A-Za-z0-9_]*")]
    Word,
    #[regex(r"[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?")]
    Number,
    #[token("\"")]
    Quote,
    #[token("/*")]
    Comment,
    #[token("!!")]
    Line,
    #[token("//")]
    Explanation,
    #[token(";")]
    #[token(",")]
    #[token(".")]
    #[token("..")]
    #[token("=")]
    #[token("==")]
    #[token("!=")]
    #[token("<>")]
    #[token("<")]
    #[token(">")]
    #[token("<=")]
    #[token(">=")]
    #[token("(")]
    #[token(")")]
    #[token("[")]
    #[token("]")]
    #[token("{")]
    #[token("}")]
    #[token(":")]
    #[token(":=")]
    #[token("*")]
    #[token("+")]
    #[token("-")]
    #[token("/")]
    #[token("->")]
    #[token("--")]
    #[token("-<>")]
    #[token("-<#>")]
    #[token("=>")]
    #[token("#")]
    #[token("%")]
    #[token("@")]
    #[token("\\")]
    #[token("~")]
    Symbol,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Kind {
    Word,
    Number,
    String(String),
    Explanation(String),
    Symbol,
    Invalid,
}
#[derive(Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

// Source-preserving syntax data; comments have no model-checking semantics.
#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct Trivia {
    pub kind: TriviaKind,
    pub span: Span,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TriviaKind {
    LineComment,
    MetaAttribute,
    BlockComment,
}
pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub trivia: Vec<Trivia>,
}

fn hex(source: &str, at: usize) -> Option<u16> {
    let text = source.get(at..at + 4)?;
    if text.bytes().all(|b| b.is_ascii_hexdigit()) {
        u16::from_str_radix(text, 16).ok()
    } else {
        None
    }
}
// INTERLIS escapes, deliberately not a JSON string decoder. Return only UTF-8 boundaries.
fn string(source: &str, mut at: usize) -> Result<(String, usize), usize> {
    let mut out = String::new();
    while let Some(c) = source[at..].chars().next() {
        at += c.len_utf8();
        match c {
            '"' => return Ok((out, at)),
            '\r' | '\n' => return Err(at),
            '\\' => {
                let Some(c) = source[at..].chars().next() else {
                    return Err(at);
                };
                at += c.len_utf8();
                match c {
                    '"' | '\\' => out.push(c),
                    'u' => {
                        let Some(first) = hex(source, at) else {
                            return Err(at);
                        };
                        at += 4;
                        let value = if (0xd800..=0xdbff).contains(&first) {
                            if source.get(at..at + 2) != Some("\\u") {
                                return Err(at);
                            };
                            at += 2;
                            let Some(second) = hex(source, at) else {
                                return Err(at);
                            };
                            at += 4;
                            if !(0xdc00..=0xdfff).contains(&second) {
                                return Err(at);
                            }
                            0x10000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second)
                                - 0xdc00
                        } else {
                            u32::from(first)
                        };
                        let Some(c) = char::from_u32(value) else {
                            return Err(at);
                        };
                        out.push(c);
                    }
                    _ => return Err(at),
                }
            }
            _ => out.push(c),
        }
    }
    Err(at)
}

fn push_trivia(
    output: &mut Vec<Trivia>,
    sources: &SourceSet,
    file: usize,
    kind: TriviaKind,
    range: Range<usize>,
    budget: &mut SyntaxBudget,
    diagnostics: &mut Diagnostics,
) -> bool {
    if !budget.node() {
        diagnostics.limit(
            "Syntax node/work budget exceeded",
            sources.span(file, range.start, range.end),
        );
        return false;
    }
    output.push(Trivia {
        kind,
        span: sources
            .span(file, range.start, range.end)
            .expect("original lexer boundaries"),
    });
    true
}

pub(crate) fn lex(
    sources: &SourceSet,
    file: usize,
    limits: &Limits,
    budget: &mut SyntaxBudget,
    diagnostics: &mut Diagnostics,
) -> Lexed {
    let source = sources.files()[file].text();
    let mut output = Lexed {
        tokens: vec![],
        trivia: vec![],
    };
    if !budget.work(u32::try_from(source.len()).expect("addressable source")) {
        diagnostics.limit("Lexer byte/work budget exceeded", None);
        return output;
    }
    let mut lexer = Raw::lexer(source);
    if source.starts_with('\u{feff}') {
        lexer.bump(3)
    }
    while let Some(raw) = lexer.next() {
        let mut range = lexer.span();
        // Reserve a token before decoding strings or inspecting its payload.
        // Trivia uses its own syntax-node charge below.
        if !matches!(&raw, Ok(Raw::Line | Raw::Comment)) && !budget.token() {
            diagnostics.limit(
                "Token budget exceeded",
                sources.span(file, range.start, range.end),
            );
            return output;
        }
        let kind = match raw {
            Ok(Raw::Line) => {
                let kind = if source[range.end..].starts_with('@') {
                    TriviaKind::MetaAttribute
                } else {
                    TriviaKind::LineComment
                };
                let n = lexer
                    .remainder()
                    .find(['\r', '\n'])
                    .unwrap_or(lexer.remainder().len());
                lexer.bump(n);
                if !push_trivia(
                    &mut output.trivia,
                    sources,
                    file,
                    kind,
                    range.start..range.end + n,
                    budget,
                    diagnostics,
                ) {
                    return output;
                }
                continue;
            }
            Ok(Raw::Comment) => {
                let mut at = range.end;
                let mut depth = 1_u32;
                while at < source.len() && depth > 0 {
                    if source.as_bytes().get(at..at + 2) == Some(b"/*") {
                        if depth == limits.max_nesting {
                            diagnostics.limit(
                                "Comment nesting budget exceeded",
                                sources.span(file, range.start, at + 2),
                            );
                            return output;
                        }
                        depth += 1;
                        at += 2;
                    } else if source.as_bytes().get(at..at + 2) == Some(b"*/") {
                        depth -= 1;
                        at += 2;
                    } else {
                        at += 1;
                    }
                }
                if depth != 0 {
                    diagnostics.syntax_error(
                        "Unfinished outer block comment",
                        sources.span(file, range.start, source.len()),
                    );
                    return output;
                }
                lexer.bump(at - range.end);
                if !push_trivia(
                    &mut output.trivia,
                    sources,
                    file,
                    TriviaKind::BlockComment,
                    range.start..at,
                    budget,
                    diagnostics,
                ) {
                    return output;
                }
                continue;
            }
            Ok(Raw::Word) => {
                if range.len() > 255 {
                    diagnostics.syntax_error(
                        "Identifier exceeds 255 characters",
                        sources.span(file, range.start, range.end),
                    );
                    Kind::Invalid
                } else {
                    Kind::Word
                }
            }
            Ok(Raw::Number) => match number::check_spelling(&source[range.clone()], limits) {
                Ok(()) => Kind::Number,
                Err(number::NumberError::Limit) => {
                    diagnostics.limit(
                        "Numeric literal budget exceeded",
                        sources.span(file, range.start, range.end),
                    );
                    return output;
                }
                Err(number::NumberError::Form) => {
                    diagnostics.syntax_error(
                        "Invalid INTERLIS numeric spelling",
                        sources.span(file, range.start, range.end),
                    );
                    Kind::Invalid
                }
                Err(number::NumberError::Work) => {
                    unreachable!("spelling inspection performs no semantic work")
                }
            },
            Ok(Raw::Symbol) => Kind::Symbol,
            Ok(Raw::Explanation) => {
                if let Some(n) = lexer.remainder().find("//") {
                    let value = source[range.end..range.end + n].to_owned();
                    lexer.bump(n + 2);
                    range.end += n + 2;
                    Kind::Explanation(value)
                } else {
                    diagnostics.syntax_error(
                        "Unfinished explanation",
                        sources.span(file, range.start, source.len()),
                    );
                    lexer.bump(source.len() - range.end);
                    range.end = source.len();
                    Kind::Invalid
                }
            }
            Ok(Raw::Quote) => match string(source, range.end) {
                Ok((value, end)) => {
                    lexer.bump(end - range.end);
                    range.end = end;
                    Kind::String(value)
                }
                Err(end) => {
                    lexer.bump(end - range.end);
                    range.end = end;
                    diagnostics.syntax_error(
                        "Invalid or unfinished string",
                        sources.span(file, range.start, end),
                    );
                    Kind::Invalid
                }
            },
            Err(_) => {
                diagnostics.syntax_error(
                    "Invalid character",
                    sources.span(file, range.start, range.end),
                );
                Kind::Invalid
            }
        };
        output.tokens.push(Token { kind, range });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SourceInput;
    fn run(text: &str, limits: Limits) -> (SourceSet, Lexed, Diagnostics) {
        let sources = SourceSet::new(vec![SourceInput {
            name: "memory:///lex".into(),
            text: text.into(),
        }])
        .unwrap();
        let mut diagnostics = Diagnostics::new(&limits);
        let mut budget = SyntaxBudget::new(&limits);
        let lexed = lex(&sources, 0, &limits, &mut budget, &mut diagnostics);
        (sources, lexed, diagnostics)
    }
    #[test]
    fn comments_explanations_and_strings_preserve_original_ranges() {
        let text = "\u{feff}!! ordinary\r\n!!@ key=\"😀\"\r/* outer /* inner */ tail */\n\"/*//!!😀\" // /* raw */ !! 😀 / raw //";
        let (sources, lexed, d) = run(text, Limits::default());
        assert!(d.entries.is_empty());
        assert_eq!(
            lexed.trivia.iter().map(|t| &t.kind).collect::<Vec<_>>(),
            [
                &TriviaKind::LineComment,
                &TriviaKind::MetaAttribute,
                &TriviaKind::BlockComment
            ]
        );
        let original = |span: &Span| {
            let l = sources.location(span).unwrap();
            &text[l.start_byte as usize..l.end_byte as usize]
        };
        assert_eq!(original(&lexed.trivia[0].span), "!! ordinary");
        assert_eq!(original(&lexed.trivia[1].span), "!!@ key=\"😀\"");
        assert_eq!(
            original(&lexed.trivia[2].span),
            "/* outer /* inner */ tail */"
        );
        assert_eq!(lexed.tokens.len(), 2);
        assert_eq!(lexed.tokens[0].kind, Kind::String("/*//!!😀".into()));
        assert_eq!(
            lexed.tokens[1].kind,
            Kind::Explanation(" /* raw */ !! 😀 / raw ".into())
        );
        assert_eq!(
            &text[lexed.tokens[1].range.clone()],
            "// /* raw */ !! 😀 / raw //"
        );
    }
    #[test]
    fn composite_symbols_keep_their_exact_lexical_boundaries() {
        let text = "-- -<> -<#> -> => == <= >= <> != := .. - - < > / @ % # \\ ~";
        let (_, lexed, d) = run(text, Limits::default());
        assert!(d.entries.is_empty());
        assert!(lexed.tokens.iter().all(|t| t.kind == Kind::Symbol));
        assert_eq!(
            lexed
                .tokens
                .iter()
                .map(|t| &text[t.range.clone()])
                .collect::<Vec<_>>(),
            [
                "--", "-<>", "-<#>", "->", "=>", "==", "<=", ">=", "<>", "!=", ":=", "..", "-",
                "-", "<", ">", "/", "@", "%", "#", "\\", "~"
            ]
        );
    }
    #[test]
    fn identifier_and_comment_limits_are_checked_at_the_boundary() {
        let (_, a, d) = run(&"A".repeat(255), Limits::default());
        assert!(d.entries.is_empty());
        assert_eq!(a.tokens[0].kind, Kind::Word);
        let (_, a, d) = run(&"A".repeat(256), Limits::default());
        assert!(d.syntax_invalid);
        assert_eq!(a.tokens[0].kind, Kind::Invalid);
        let limits = Limits {
            max_nesting: 2,
            ..Limits::default()
        };
        assert!(!run("/* /* */ */", limits.clone()).2.limited);
        let d = run("/* /* /* */ */ */", limits).2;
        assert!(d.limited);
        assert!(!d.syntax_invalid);
        let d = run("/* outer /* inner */", Limits::default()).2;
        assert!(d.syntax_invalid);
        assert!(!d.limited);
        assert!(run("ordinary", Limits::default()).2.entries.is_empty());
    }
    #[test]
    fn literal_form_and_resource_checks_precede_numeric_allocation() {
        for text in ["0.10E-2", "0.000E+2", "1234"] {
            assert!(run(text, Limits::default()).2.entries.is_empty(), "{text}");
        }
        for text in [
            "1E2",
            "1.0e2",
            "0.001e2",
            r#""bad\n""#,
            r#""bad\uD800""#,
            r#""bad\u12😃""#,
            "// unfinished",
        ] {
            let (_, _, d) = run(text, Limits::default());
            assert!(d.syntax_invalid, "{text}");
            assert!(!d.limited, "{text}");
        }
        let limits = Limits {
            max_numeric_digits: 4,
            ..Limits::default()
        };
        assert!(!run("1234", limits.clone()).2.limited);
        assert!(run("12345", limits).2.limited);
        let limits = Limits {
            max_abs_exponent: 4,
            ..Limits::default()
        };
        assert!(!run("0.1E+4", limits.clone()).2.limited);
        assert!(run("0.1E+5", limits).2.limited);
        let (_, lexed, d) = run(r#""\uD83D\uDE00""#, Limits::default());
        assert!(d.entries.is_empty());
        assert_eq!(lexed.tokens[0].kind, Kind::String("😀".into()));
    }
    #[test]
    fn trivia_token_and_work_charges_happen_before_publication() {
        let limits = Limits {
            max_tokens: 2,
            ..Limits::default()
        };
        assert_eq!(run("A B", limits.clone()).1.tokens.len(), 2);
        let (_, lexed, d) = run("A B C", limits);
        assert!(d.limited);
        assert_eq!(lexed.tokens.len(), 2);
        let limits = Limits {
            max_syntax_nodes: 1,
            ..Limits::default()
        };
        assert!(!run("!! one", limits.clone()).2.limited);
        let (_, lexed, d) = run("!! one\n!! two", limits);
        assert!(d.limited);
        assert_eq!(lexed.trivia.len(), 1);
        let limits = Limits {
            max_parser_steps: 2,
            ..Limits::default()
        };
        assert!(!run("AB", limits.clone()).2.limited);
        let (_, lexed, d) = run("ABC", limits);
        assert!(d.limited);
        assert!(lexed.tokens.is_empty());
        assert!(lexed.trivia.is_empty());
    }
}
