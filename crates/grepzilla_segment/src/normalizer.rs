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
    ('\u{0300}'..='\u{036F}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_normalize_basic() {
        assert_eq!(normalize("КоШКи"), "кошки");
    }
}