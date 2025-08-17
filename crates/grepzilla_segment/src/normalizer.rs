use unicode_normalization::UnicodeNormalization;

pub fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let nfkc = lower.nfkc().collect::<String>();
    strip_accents(&nfkc)
}

fn strip_accents(s: &str) -> String {
    s.nfd().filter(|c| !is_mark(*c)).collect()
}

fn is_mark(c: char) -> bool {
    // Упрощённый детектор комбинирующих знаков — достаточно для MVP
    ('\u{0300}'..='\u{036F}').contains(&c)
}