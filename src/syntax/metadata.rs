use std::{collections::BTreeMap, ops::Range};

use crate::{
    DocumentationComment, MetaAttribute, SourceMetadata, SourceSet,
    diagnostic::Diagnostics,
    lexer::{self, Token, Trivia, TriviaKind},
    limits::SyntaxBudget,
};

pub(super) fn read(
    sources: &SourceSet,
    file: usize,
    tokens: &[Token],
    trivia: &[Trivia],
    budget: &mut SyntaxBudget,
    diagnostics: &mut Diagnostics,
) -> Option<BTreeMap<u32, SourceMetadata>> {
    let mut metadata = BTreeMap::<u32, SourceMetadata>::new();
    let source = sources.files()[file].text();
    let mut tokens = tokens
        .iter()
        .filter(|token| {
            !matches!(
                &source[token.range.clone()],
                "DOMAIN" | "UNIT" | "CONTEXT" | "ATTRIBUTE" | "PARAMETER"
            )
        })
        .peekable();
    let mut comments = trivia.iter().peekable();
    while let Some(comment) = comments.next() {
        let range = comment_range(comment);
        let (end, attributes, documentation) = match comment.kind {
            TriviaKind::MetaAttribute => {
                let mut annotation = Annotation::default();
                annotation.append(source, range.start + 3..range.end);
                let mut end = range.end;
                while annotation.quoted {
                    let Some(next) = comments
                        .peek()
                        .filter(|c| c.kind == TriviaKind::MetaAttribute)
                    else {
                        break;
                    };
                    let next = comment_range(next);
                    if !source[end..next.start].chars().all(char::is_whitespace) {
                        break;
                    }
                    annotation.append(source, end..next.start);
                    annotation.append(source, next.start + 3..next.end);
                    end = next.end;
                    comments.next();
                }
                let attributes = annotation.read(sources, file, budget, diagnostics)?;
                (end, attributes, None)
            }
            TriviaKind::BlockComment if source[range.clone()].starts_with("/**") => {
                let content = if range.len() <= 4 {
                    ""
                } else {
                    &source[range.start + 3..range.end - 2]
                };
                let text = content
                    .lines()
                    .map(|line| {
                        line.trim_start()
                            .strip_prefix('*')
                            .map_or(line, |rest| rest.strip_prefix(' ').unwrap_or(rest))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    range.end,
                    Vec::new(),
                    Some(DocumentationComment {
                        text: text.trim().to_owned(),
                        span: comment.span.clone(),
                    }),
                )
            }
            _ => continue,
        };
        while tokens.peek().is_some_and(|token| token.range.start < end) {
            tokens.next();
        }
        if let Some(token) = tokens.peek()
            && (!attributes.is_empty() || documentation.is_some())
        {
            let entry = metadata.entry(token.range.start as u32).or_default();
            entry.attributes.extend(attributes);
            entry.documentation.extend(documentation);
        }
    }
    Some(metadata)
}

fn comment_range(trivia: &Trivia) -> Range<usize> {
    let (_, start, end) = trivia.span.sort_key();
    start as usize..end as usize
}

/// Maps text spanning multiple comments to its original byte ranges.
#[derive(Default)]
struct Annotation {
    text: String,
    chunks: Vec<(usize, usize)>,
    quoted: bool,
    escaped: bool,
}

impl Annotation {
    fn append(&mut self, source: &str, range: Range<usize>) {
        self.chunks.push((self.text.len(), range.start));
        let content = &source[range];
        for c in content.chars() {
            if self.escaped {
                self.escaped = false;
            } else if self.quoted && c == '\\' {
                self.escaped = true;
            } else if c == '"' {
                self.quoted = !self.quoted;
            }
        }
        self.text.push_str(content);
    }

    fn original(&self, at: usize) -> usize {
        let (text, source) = self.chunks[self.chunks.partition_point(|&(text, _)| text <= at) - 1];
        source + (at - text)
    }

    fn read(
        &self,
        sources: &SourceSet,
        file: usize,
        budget: &mut SyntaxBudget,
        diagnostics: &mut Diagnostics,
    ) -> Option<Vec<MetaAttribute>> {
        fn skip(text: &str, at: usize, predicate: impl Fn(char) -> bool) -> usize {
            text[at..]
                .find(|c| !predicate(c))
                .map_or(text.len(), |n| at + n)
        }
        let span =
            |start, end| sources.span(file, self.original(start), self.original(end - 1) + 1);
        let bare = |c: char| !c.is_whitespace() && !matches!(c, '=' | ';' | ',' | '"');
        let text = &self.text;
        let mut attributes = Vec::new();
        let mut at = 0;
        loop {
            at = skip(text, at, |c| c == ';' || c.is_whitespace());
            if at == text.len() {
                return Some(attributes);
            }
            let start = at;
            let key_end = skip(text, at, bare);
            at = skip(text, key_end, char::is_whitespace);
            if start == key_end || text.as_bytes().get(at) != Some(&b'=') {
                break;
            }
            at = skip(text, at + 1, char::is_whitespace);
            let (value, end) = if text.as_bytes().get(at) == Some(&b'"') {
                match lexer::string(text, at + 1, true) {
                    Ok(decoded) => decoded,
                    Err(end) => {
                        at = end;
                        break;
                    }
                }
            } else {
                let end = skip(text, at, bare);
                if end == at {
                    break;
                }
                (text[at..end].to_owned(), end)
            };
            at = skip(text, end, char::is_whitespace);
            if at < text.len() && text.as_bytes()[at] != b';' {
                break;
            }
            if !budget.node() {
                diagnostics.limit("Syntax node/work budget exceeded", span(start, end));
                return None;
            }
            attributes.push(MetaAttribute {
                key: text[start..key_end].to_owned(),
                text: value,
                span: span(start, end).expect("annotation uses original UTF-8 boundaries"),
            });
            if at == text.len() {
                return Some(attributes);
            }
            at += 1;
        }
        diagnostics.syntax_error(
            "Expected a metadata assignment of the form key=value",
            sources.span(file, self.original(at), self.original(at)),
        );
        None
    }
}
