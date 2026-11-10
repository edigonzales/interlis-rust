use crate::{Diagnostic, Limits, Severity, Span};

/// Run-local outcome flags are independent of the bounded visible buffer.
pub(crate) struct Diagnostics {
    pub entries: Vec<Diagnostic>,
    pub incomplete: bool,
    pub limited: bool,
    pub rejected: bool,
    cap: usize,
}
impl Diagnostics {
    pub fn new(limits: &Limits) -> Self {
        Self {
            entries: vec![],
            incomplete: false,
            limited: false,
            rejected: false,
            cap: limits.max_diagnostics as usize,
        }
    }
    pub fn push(&mut self, code: &str, message: &str, primary: Option<Span>) {
        match code {
            "ILI-LIMIT" => self.limited = true,
            "ILI-UNSUPPORTED" => self.incomplete = true,
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
            related: vec![],
        };
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
