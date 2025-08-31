// crates/grepzilla_segment/src/verify/mod.rs
use anyhow::Result;
use std::sync::Arc;

#[cfg(feature = "engine-pcre2")]
mod pcre2_impl;
mod regex_impl;

pub mod factory;
pub use factory::{EnvVerifyFactory, VerifyFactory};

pub use regex_impl::RegexVerify; // оставляем для совместимости там, где он явно использовался

/// Унифицированный интерфейс верификации совпадений.
pub trait VerifyEngine: Send + Sync {
    /// Быстрая проверка совпадения.
    fn is_match(&self, text: &str) -> bool;

    /// Первая зона совпадения (байтовые индексы) — для подсветки.
    /// Возвращает (start_byte, end_byte) либо None.
    fn find(&self, text: &str) -> Option<(usize, usize)>;
}

/// Wildcard → regex-строка с семантикой (?si) (dotall + case-insensitive)
/// Нужен как утилита в некоторых местах.
pub fn wildcard_to_regex_case_insensitive(pat: &str) -> String {
    let mut rx = String::from("(?si)");
    for ch in pat.chars() {
        match ch {
            '*' => rx.push_str(".*"),
            '?' => rx.push('.'),
            c => {
                if "\\.^$|()[]{}+*?".contains(c) {
                    rx.push('\\');
                }
                rx.push(c);
            }
        }
    }
    rx
}

/// Компиляция движка из wildcard с учётом переменной окружения GZ_VERIFY.
/// Возвращает `Arc<dyn VerifyEngine>` для прозрачно-заменяемой реализации.
///
/// Примечание: нормализацию wildcard делаем здесь же, чтобы call-site всегда
/// передавал «сырой» паттерн.
pub fn compile_wildcard_engine(raw_wildcard: &str) -> Result<Arc<dyn VerifyEngine>> {
    let norm = crate::normalizer::normalize(raw_wildcard);
    EnvVerifyFactory::from_env().compile(&norm)
}
