// crates/grepzilla_segment/src/verify/regex_impl.rs
use anyhow::Result;
use regex::Regex;

use super::{VerifyEngine, wildcard_to_regex_case_insensitive};

/// Базовая реализация VerifyEngine на regex
pub struct RegexVerify {
    rx: Regex,
}

impl RegexVerify {
    pub fn compile_regex(pat: &str) -> Result<Self> {
        Ok(Self {
            rx: Regex::new(pat)?,
        })
    }

    pub fn compile_wildcard(wildcard: &str) -> Result<Self> {
        let pat = wildcard_to_regex_case_insensitive(wildcard);
        Self::compile_regex(&pat)
    }
}

impl VerifyEngine for RegexVerify {
    #[inline]
    fn is_match(&self, text: &str) -> bool {
        self.rx.is_match(text)
    }

    #[inline]
    fn find(&self, text: &str) -> Option<(usize, usize)> {
        self.rx.find(text).map(|m| (m.start(), m.end()))
    }
}
