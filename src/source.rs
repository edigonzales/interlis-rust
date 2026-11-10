use serde::{Deserialize, Serialize};

use crate::RequestError;

/// One exact UTF-8 source and a logical identifier. The core never dereferences the name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub name: String,
    pub text: String,
}

/// Half-open UTF-8 byte range, interpreted only with its originating report.
#[derive(Clone, Debug)]
pub struct Span {
    source: u32,
    start: u32,
    end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub column_utf16: u32,
}

#[derive(Debug)]
pub struct SourceFile {
    name: String,
    text: String,
    line_starts: Option<Vec<u32>>,
}

impl SourceFile {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// One-based line and UTF-16 column for a UTF-8 character boundary, including EOF.
    /// Leading BOM is invisible; CRLF is one break. The LF byte in CRLF has the
    /// preceding line's end column. Invalid offsets return None.
    pub fn position(&self, byte: u32) -> Option<Position> {
        let offset = usize::try_from(byte).ok()?;
        if offset > self.text.len() || !self.text.is_char_boundary(offset) {
            return None;
        }
        let (line, start) = if let Some(starts) = &self.line_starts {
            let i = starts
                .partition_point(|&start| start <= byte)
                .checked_sub(1)?;
            (
                u32::try_from(i).ok()?.checked_add(1)?,
                usize::try_from(starts[i]).ok()?,
            )
        } else {
            // Limit reports retain text without allocating proportional line indices.
            let mut line = 1_u32;
            let mut start = 0;
            for end in line_ends(&self.text).take_while(|&end| end <= offset) {
                line = line.checked_add(1)?;
                start = end;
            }
            (line, start)
        };
        let prefix = &self.text[start..offset];
        let units = prefix
            .chars()
            .filter(|&c| c != '\r' && c != '\n')
            .map(char::len_utf16)
            .sum::<usize>();
        let bom = usize::from(start == 0 && prefix.starts_with('\u{feff}'));
        Some(Position {
            line,
            column_utf16: u32::try_from(units.checked_sub(bom)?)
                .ok()?
                .checked_add(1)?,
        })
    }
}

fn line_ends(text: &str) -> impl Iterator<Item = usize> + '_ {
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .filter_map(move |(i, &byte)| match byte {
            b'\n' => Some(i + 1),
            b'\r' if bytes.get(i + 1) != Some(&b'\n') => Some(i + 1),
            _ => None,
        })
}

#[derive(Debug)]
pub struct SourceSet {
    files: Vec<SourceFile>,
}

/// A source location projected from an original byte range.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    pub source: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub start_column_utf16: u32,
    pub end_line: u32,
    pub end_column_utf16: u32,
}

impl SourceSet {
    /// Takes ownership without normalizing text or building text-sized auxiliary indices.
    /// Names are unique, 1–1024 UTF-8 bytes and contain no control characters.
    /// Requests contain 1–4096 sources, in deterministic UTF-8 name order.
    pub fn new(mut inputs: Vec<SourceInput>) -> Result<Self, RequestError> {
        if inputs.is_empty() || inputs.len() > 4096 {
            return Err(RequestError::new(
                "REQUEST_SOURCES",
                "Expected 1 to 4096 sources",
            ));
        }
        for input in &inputs {
            if input.name.is_empty()
                || input.name.len() > 1024
                || input.name.chars().any(char::is_control)
            {
                return Err(RequestError::new(
                    "REQUEST_SOURCE_NAME",
                    "Invalid source identifier",
                ));
            }
            if input.text.len() > u32::MAX as usize {
                return Err(RequestError::new(
                    "REQUEST_SOURCE_SIZE",
                    "Source exceeds addressable byte range",
                ));
            }
        }
        inputs.sort_by(|a, b| a.name.cmp(&b.name));
        if inputs.windows(2).any(|pair| pair[0].name == pair[1].name) {
            return Err(RequestError::new(
                "REQUEST_DUPLICATE_SOURCE",
                "Source identifiers must be unique",
            ));
        }
        Ok(Self {
            files: inputs
                .into_iter()
                .map(|input| SourceFile {
                    name: input.name,
                    text: input.text,
                    line_starts: None,
                })
                .collect(),
        })
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    pub fn by_name(&self, name: &str) -> Option<&SourceFile> {
        self.files
            .binary_search_by(|file| file.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.files[i])
    }

    /// Resolves a span from this report; rejects invalid byte boundaries and indices.
    pub fn location(&self, span: &Span) -> Option<SourceLocation> {
        let file = self.files.get(usize::try_from(span.source).ok()?)?;
        if span.start > span.end {
            return None;
        }
        let start = file.position(span.start)?;
        let end = file.position(span.end)?;
        Some(SourceLocation {
            source: file.name.clone(),
            start_byte: span.start,
            end_byte: span.end,
            start_line: start.line,
            start_column_utf16: start.column_utf16,
            end_line: end.line,
            end_column_utf16: end.column_utf16,
        })
    }

    pub(crate) fn index_lines(&mut self) {
        for file in &mut self.files {
            let mut starts = vec![0];
            starts.extend(
                line_ends(&file.text).map(|end| u32::try_from(end).expect("bounded source")),
            );
            file.line_starts = Some(starts);
        }
    }

    pub(crate) fn span(&self, source: usize, start: usize, end: usize) -> Option<Span> {
        let file = self.files.get(source)?;
        if start > end
            || end > file.text.len()
            || !file.text.is_char_boundary(start)
            || !file.text.is_char_boundary(end)
        {
            return None;
        }
        Some(Span {
            source: u32::try_from(source).ok()?,
            start: u32::try_from(start).ok()?,
            end: u32::try_from(end).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_text_and_positions_with_and_without_indices() {
        let text = "\u{feff}ä😀\r\nx\ry\nz";
        let mut sources = SourceSet::new(vec![SourceInput {
            name: "memory:///a?exact".into(),
            text: text.into(),
        }])
        .unwrap();
        for indexed in [false, true] {
            if indexed {
                sources.index_lines();
            }
            let file = &sources.files()[0];
            assert_eq!(file.text().as_bytes(), text.as_bytes());
            for (byte, line, column_utf16) in [
                (0, 1, 1),
                (3, 1, 1),
                (5, 1, 2),
                (9, 1, 4),
                (10, 1, 4),
                (11, 2, 1),
                (12, 2, 2),
                (13, 3, 1),
                (15, 4, 1),
                (16, 4, 2),
            ] {
                assert_eq!(
                    file.position(byte),
                    Some(Position { line, column_utf16 }),
                    "{byte}"
                );
            }
            for byte in [1, 4, 6, 17] {
                assert_eq!(file.position(byte), None);
            }
        }
        assert!(sources.by_name("memory:///a").is_none());
        assert!(sources.span(0, 5, 6).is_none());
        assert!(sources.span(1, 0, 0).is_none());
        let eof = sources.span(0, 16, 16).unwrap();
        assert_eq!(sources.location(&eof).unwrap().end_line, 4);
    }

    #[test]
    fn rejects_bad_identifiers_and_duplicates_and_orders_names() {
        for names in [vec![], vec![""], vec!["bad\n"], vec!["a", "a"]] {
            assert!(
                SourceSet::new(
                    names
                        .into_iter()
                        .map(|name| SourceInput {
                            name: name.into(),
                            text: String::new()
                        })
                        .collect()
                )
                .is_err()
            );
        }
        let sources = SourceSet::new(
            ["z", "a"]
                .into_iter()
                .map(|name| SourceInput {
                    name: name.into(),
                    text: String::new(),
                })
                .collect(),
        )
        .unwrap();
        assert_eq!(
            sources
                .files()
                .iter()
                .map(SourceFile::name)
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert_eq!(
            sources.files()[0].position(0),
            Some(Position {
                line: 1,
                column_utf16: 1
            })
        );
    }
}
