use crate::{Limits, SourceSet, diagnostic::Diagnostics};
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
    Symbol,
}
#[derive(Debug, PartialEq)]
pub(crate) enum Kind {
    Word,
    Number,
    String(String),
    Symbol,
    Invalid,
}
#[derive(Debug)]
pub(crate) struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
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

pub(crate) fn lex(
    sources: &SourceSet,
    file: usize,
    limits: &Limits,
    remaining: &mut u32,
    diagnostics: &mut Diagnostics,
) -> Vec<Token> {
    let source = sources.files()[file].text();
    let mut lexer = Raw::lexer(source);
    if source.starts_with('\u{feff}') {
        lexer.bump(3)
    }
    let mut tokens = vec![];
    while let Some(raw) = lexer.next() {
        let mut range = lexer.span();
        let kind = match raw {
            Ok(Raw::Line) => {
                let n = lexer
                    .remainder()
                    .find(['\r', '\n'])
                    .unwrap_or(lexer.remainder().len());
                lexer.bump(n);
                continue;
            }
            Ok(Raw::Comment) => {
                let mut at = range.end;
                let mut depth = 1_u32;
                while at < source.len() && depth > 0 {
                    if source.as_bytes().get(at..at + 2) == Some(b"/*") {
                        depth += 1;
                        at += 2;
                    } else if source.as_bytes().get(at..at + 2) == Some(b"*/") {
                        depth -= 1;
                        at += 2;
                    } else {
                        at += 1;
                    }
                    if depth > limits.max_nesting {
                        diagnostics.push(
                            "ILI-LIMIT",
                            "Comment nesting budget exceeded",
                            sources.span(file, range.start, at),
                        );
                        return tokens;
                    }
                }
                if depth != 0 {
                    diagnostics.push(
                        "ILI-SYNTAX",
                        "Unfinished outer block comment",
                        sources.span(file, range.start, source.len()),
                    );
                    return tokens;
                }
                lexer.bump(at - range.end);
                continue;
            }
            Ok(Raw::Word) => Kind::Word,
            Ok(Raw::Number) => Kind::Number,
            Ok(Raw::Symbol) => Kind::Symbol,
            Ok(Raw::Quote) => match string(source, range.end) {
                Ok((value, end)) => {
                    lexer.bump(end - range.end);
                    range.end = end;
                    Kind::String(value)
                }
                Err(end) => {
                    lexer.bump(end - range.end);
                    range.end = end;
                    diagnostics.push(
                        "ILI-SYNTAX",
                        "Invalid or unfinished string",
                        sources.span(file, range.start, end),
                    );
                    Kind::Invalid
                }
            },
            Err(_) => {
                diagnostics.push(
                    "ILI-SYNTAX",
                    "Invalid character",
                    sources.span(file, range.start, range.end),
                );
                Kind::Invalid
            }
        };
        if *remaining == 0 {
            diagnostics.push(
                "ILI-LIMIT",
                "Token budget exceeded",
                sources.span(file, range.start, range.end),
            );
            return tokens;
        }
        *remaining -= 1;
        tokens.push(Token { kind, range });
        if diagnostics.limited {
            break;
        }
    }
    tokens
}
