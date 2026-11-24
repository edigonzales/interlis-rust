use crate::{Diagnostic, Limits, RelatedLocation, Severity, Span};

/// Run-local outcome flags are independent of the bounded visible buffer.
pub(crate) struct Diagnostics {
    pub entries: Vec<Diagnostic>,
    pub incomplete: bool,
    pub limited: bool,
    pub rejected: bool,
    pub syntax_invalid: bool,
    pub unsupported_version: bool,
    cap: usize,
}
impl Diagnostics {
    pub fn absorb(&mut self, other: Self) {
        self.limited |= other.limited;
        self.incomplete |= other.incomplete;
        self.syntax_invalid |= other.syntax_invalid;
        self.unsupported_version |= other.unsupported_version;
        self.rejected |= other.rejected;
        for entry in other.entries {
            self.push_related(&entry.code, &entry.message, entry.primary, entry.related);
        }
    }

    pub fn new(limits: &Limits) -> Self {
        Self {
            entries: vec![],
            incomplete: false,
            limited: false,
            rejected: false,
            syntax_invalid: false,
            unsupported_version: false,
            cap: limits.max_diagnostics as usize,
        }
    }
    pub fn into_sorted(mut self) -> Vec<Diagnostic> {
        self.entries.sort_by(|a, b| {
            let location = |d: &Diagnostic| d.primary.as_ref().map(Span::sort_key);
            location(a)
                .cmp(&location(b))
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.message.cmp(&b.message))
        });
        self.entries
    }
    pub fn syntax_error(&mut self, message: &str, primary: Option<Span>) {
        self.syntax_invalid = true;
        self.push("ILI-SYNTAX", message, primary);
    }
    pub fn unsupported_version(&mut self, message: &str, primary: Option<Span>) {
        self.unsupported_version = true;
        self.push("ILI-UNSUPPORTED", message, primary);
    }
    pub fn limit(&mut self, message: &str, primary: Option<Span>) {
        self.push("ILI-LIMIT", message, primary);
    }
    pub fn push(&mut self, code: &str, message: &str, primary: Option<Span>) {
        self.push_related(code, message, primary, vec![]);
    }
    pub fn push_related(
        &mut self,
        code: &str,
        message: &str,
        primary: Option<Span>,
        related: Vec<RelatedLocation>,
    ) {
        match code {
            "ILI-LIMIT" => self.limited = true,
            "ILI-UNSUPPORTED" => self.incomplete = true,
            "ILI-SYNTAX" => {
                self.syntax_invalid = true;
                self.rejected = true;
            }
            _ => self.rejected = true,
        }
        let diagnostic = Diagnostic {
            code: code.into(),
            severity: if code == "ILI-UNSUPPORTED" {
                Severity::Warning
            } else {
                Severity::Error
            },
            message: message.into(),
            primary,
            related,
        };
        self.record(diagnostic);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "native-cli"))]
    pub fn warning(&mut self, code: &str, message: &str, primary: Option<Span>) {
        self.record(Diagnostic {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            primary,
            related: Vec::new(),
        });
    }

    fn record(&mut self, diagnostic: Diagnostic) {
        if self.entries.len() < self.cap {
            self.entries.push(diagnostic);
        } else {
            self.limited = true;
            // Keep a visible explanation even when the triggering diagnostic cannot fit.
            self.entries[self.cap - 1] = Diagnostic {
                code: "ILI-LIMIT".into(),
                severity: Severity::Error,
                message: "Diagnostic budget exceeded".into(),
                primary: diagnostic.primary,
                related: vec![],
            };
        }
    }
}
