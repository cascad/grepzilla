use anyhow::Result;
use std::sync::Arc;

use super::{RegexVerify, VerifyEngine};

/// Фабрика движков верификации.
pub trait VerifyFactory: Send + Sync {
    /// Компилирует движок под нормализованный wildcard-паттерн.
    fn compile(&self, wildcard_normalized: &str) -> Result<Arc<dyn VerifyEngine>>;
}

/// Выбор движка по ENV, с безопасным фолбэком на regex.
/// Поддерживаем только `regex` (остальное — в будущих эпиках).
pub struct EnvVerifyFactory {
    engine: String,
}

impl EnvVerifyFactory {
    pub fn from_env() -> Self {
        let v = std::env::var("GZ_VERIFY_ENGINE").unwrap_or_else(|_| "regex".to_string());
        Self {
            engine: v.to_lowercase(),
        }
    }
}

impl Default for EnvVerifyFactory {
    fn default() -> Self {
        Self::from_env()
    }
}

impl VerifyFactory for EnvVerifyFactory {
    fn compile(&self, wildcard_normalized: &str) -> Result<Arc<dyn VerifyEngine>> {
        match self.engine.as_str() {
            "regex" | _ => {
                let eng = RegexVerify::compile_wildcard(wildcard_normalized)?;
                Ok(Arc::new(eng))
            }
        }
    }
}
