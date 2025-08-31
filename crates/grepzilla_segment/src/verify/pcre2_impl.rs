// crates/grepzilla_segment/src/verify/pcre2_impl.rs
use super::{VerifyEngine, VerifyFactory};
use anyhow::Result;
use pcre2::bytes::{Regex, RegexBuilder};
use std::sync::Arc;

/// PCRE2-движок. Предполагаем, что входной шаблон — уже НОРМАЛИЗОВАННЫЙ wildcard
/// (звёздочки/вопросы преобразуются в regex в фабрике ниже).
pub struct Pcre2Engine {
    rx: Regex,
}

impl VerifyEngine for Pcre2Engine {
    #[inline]
    fn is_match(&self, text: &str) -> bool {
        // PCRE2 bytes API: работаем по UTF-8 как по байтам; индексы совпадают с байтовыми.
        self.rx.is_match(text.as_bytes()).unwrap_or(false)
    }

    #[inline]
    fn find(&self, text: &str) -> Option<(usize, usize)> {
        match self.rx.find(text.as_bytes()) {
            Ok(Some(m)) => Some((m.start(), m.end())), // байтовые индексы
            _ => None,
        }
    }
}

pub struct Pcre2Factory;

impl VerifyFactory for Pcre2Factory {
    fn compile(&self, normalized_wildcard: &str) -> Result<Arc<dyn VerifyEngine>> {
        let pat = wildcard_to_regex_pattern(normalized_wildcard);
        let rx = RegexBuilder::new()
            .caseless(true) // (?i)
            .dotall(true)   // (?s)
            .build(&pat)
            .map_err(|e| anyhow::anyhow!("pcre2 compile error: {e}"))?;
        Ok(Arc::new(Pcre2Engine { rx }))
    }
}

/// Преобразование normalized_wildcard -> regex-паттерн:
/// '*' => '.*', '?' => '.', остальные символы — экранируем.
/// (Флаги (?si) задаются через RegexBuilder, поэтому их здесь НЕ добавляем.)
fn wildcard_to_regex_pattern(norm_wildcard: &str) -> String {
    let mut out = String::new();
    for ch in norm_wildcard.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            _ => {
                // экранируем спецсимволы PCRE
                if "\\.^$|()[]{}+*?".contains(ch) {
                    out.push('\\');
                }
                out.push(ch);
            }
        }
    }
    out
}
