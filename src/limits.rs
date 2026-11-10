use serde::Deserialize;

use crate::RequestError;

/// Deterministic per-request budgets. All fields use the same width on native and wasm32.
/// Byte limits concern original UTF-8; other fields count their named operations.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_source_bytes: u32,
    pub max_total_source_bytes: u32,
    pub max_tokens: u32,
    pub max_nesting: u32,
    pub max_diagnostics: u32,
    pub max_numeric_digits: u32,
    pub max_abs_exponent: u32,
    pub max_semantic_steps: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_source_bytes: 2 * 1024 * 1024,
            max_total_source_bytes: 8 * 1024 * 1024,
            max_tokens: 250_000,
            max_nesting: 128,
            max_diagnostics: 100,
            max_numeric_digits: 4096,
            max_abs_exponent: 10_000,
            max_semantic_steps: 1_000_000,
        }
    }
}

impl Limits {
    /// Rejects zero budgets and bounds allocations and future analysis work.
    pub fn validate(&self) -> Result<(), RequestError> {
        let fields = [
            (self.max_source_bytes, 8 * 1024 * 1024),
            (self.max_total_source_bytes, 32 * 1024 * 1024),
            (self.max_tokens, 1_000_000),
            (self.max_nesting, 256),
            (self.max_diagnostics, 1000),
            (self.max_numeric_digits, 16_384),
            (self.max_abs_exponent, 100_000),
            (self.max_semantic_steps, 10_000_000),
        ];
        if fields.iter().any(|&(value, cap)| value == 0 || value > cap) {
            return Err(RequestError::new(
                "REQUEST_LIMITS",
                "Invalid analysis budgets",
            ));
        }
        Ok(())
    }
}

/// Actual deterministic semantic work: syntax collection/binding hops and numeric
/// digits/alignment distance. Separate from source/token/parser budgets.
pub(crate) struct WorkBudget {
    remaining: u32,
}
impl WorkBudget {
    pub fn new(limits: &Limits) -> Self {
        Self {
            remaining: limits.max_semantic_steps,
        }
    }
    pub fn charge(&mut self, amount: u32) -> Result<(), ()> {
        self.remaining = self.remaining.checked_sub(amount).ok_or(())?;
        Ok(())
    }
}
