use unicode_normalization::UnicodeNormalization;

/// Lowercase, NFKC, strip accents
pub fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let nfkc = lower.nfkc().collect::<String>();
    strip_accents(&nfkc)
}

fn strip_accents(s: &str) -> String {
    // Decompose and drop combining marks (Mn)
    s.nfd()
        .filter(|c| !is_mark(*c))
        .collect()
}

fn is_mark(c: char) -> bool { unicode_general_category::get_general_category(c).is_mark() }

mod unicode_general_category {
    use std::ops::RangeInclusive;
    // Very small helper: treat U+0300..U+036F as combining marks; not perfect, but enough for MVP.
    pub fn get_general_category(c: char) -> Category {
        if ('\u{0300}'..='\u{036F}').contains(&c) { Category::Mark } else { Category::Other }
    }
    pub enum Category { Mark, Other }
    impl Category { pub fn is_mark(&self) -> bool { matches!(self, Category::Mark) } }
}