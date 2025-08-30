use crate::StoredDoc;

/// Опции превью.
pub struct PreviewOpts<'a> {
    /// Поля по приоритету (будет взято первое существующее).
    pub preferred_fields: &'a [&'a str],
    /// Целевое окно вывода в символах (не байтах).
    pub max_len: usize,
    /// Подсветить первую встречу иглы (уже нормализованную). Если None — просто обрезка.
    pub highlight_needle: Option<&'a str>,
}

fn pick_field_text<'a>(doc: &'a StoredDoc, preferred: &[&str]) -> Option<&'a String> {
    for f in preferred {
        if let Some(t) = doc.fields.get(*f) {
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

/// Построить превью с подсветкой вокруг первого вхождения `highlight_needle`
/// или усечённый текст, если игла не найдена. Всегда работает по границам UTF-8 символов.
pub fn build_preview(doc: &StoredDoc, opts: PreviewOpts<'_>) -> String {
    let binding = String::new();
    let text = pick_field_text(doc, opts.preferred_fields)
        .or_else(|| doc.fields.values().next())
        .unwrap_or(&binding);

    // ИЩЕМ БЕЗ УЧЁТА РЕГИСТРА (UTF-8 safe).
    match opts.highlight_needle.and_then(|n| find_substr_ci_utf8(text, n)) {
        Some((m_start_byte, m_end_byte)) => {
            snippet_with_highlight(text, m_start_byte, m_end_byte, opts.max_len)
        }
        None => truncate_chars_with_ellipsis(text, opts.max_len),
    }
}

/// Case-insensitive поиск подстроки по UTF-8.
/// Возвращает (start_byte, end_byte) в исходной строке.
fn find_substr_ci_utf8(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }

    // Lowercase с поддержкой многосимвольных маппингов.
    let h_low: String = haystack.chars().flat_map(|c| c.to_lowercase()).collect();
    let n_low: String = needle.chars().flat_map(|c| c.to_lowercase()).collect();

    // Ищем в нижнем регистре.
    let start_b_low = h_low.find(&n_low)?;

    // Индексация по символам.
    let (h_low_byte_of_char, _) = index_chars(&h_low);
    let (h_orig_byte_of_char, h_orig_chars) = index_chars(haystack);
    let n_len_chars = n_low.chars().count();

    // Переводим в индекс символа.
    let start_c = byte_to_char_idx(&h_low_byte_of_char, start_b_low);
    let end_c = start_c.saturating_add(n_len_chars);
    if end_c > h_orig_chars {
        return None;
    }

    // Байтовые границы в исходной строке.
    let start_b = h_orig_byte_of_char[start_c];
    let end_b = h_orig_byte_of_char[end_c];
    Some((start_b, end_b))
}

/// Возвращает сниппет вокруг матча, гарантируя границы по символам и подсветку скобками.
fn snippet_with_highlight(s: &str, m_start_b: usize, m_end_b: usize, max_chars: usize) -> String {
    if max_chars == 0 || s.is_empty() {
        return String::new();
    }

    // Таблица: char_idx -> byte_offset
    let (byte_of_char, total_chars) = index_chars(s);

    let m_start_c = byte_to_char_idx(&byte_of_char, m_start_b);
    let m_end_c = byte_to_char_idx(&byte_of_char, m_end_b);
    let match_len_c = m_end_c.saturating_sub(m_start_c);

    // Сколько контекста дать по краям
    let budget = max_chars.saturating_sub(match_len_c + 2);
    let ctx = budget / 2;

    let from_c = m_start_c.saturating_sub(ctx);
    let to_c = (m_end_c + ctx).min(total_chars);

    let (from_b, to_b) = (byte_of_char[from_c], byte_of_char[to_c]);

    let mut out = String::new();
    if from_c > 0 {
        out.push('…');
    }
    out.push_str(&s[from_b..m_start_b]);
    out.push('[');
    out.push_str(&s[m_start_b..m_end_b]);
    out.push(']');
    out.push_str(&s[m_end_b..to_b]);
    if to_c < total_chars {
        out.push('…');
    }

    ensure_max_chars(&out, max_chars + 4) // небольшой запас
}

fn truncate_chars_with_ellipsis(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let (byte_of_char, total_chars) = index_chars(s);
    if total_chars <= max_chars {
        return s.to_string();
    }
    let end_b = byte_of_char[max_chars];
    let mut out = String::from(&s[..end_b]);
    out.push('…');
    out
}

/// Строит массив byte_of_char: для каждого индекса символа — смещение в байтах.
/// Возвращает (таблица, количество символов).
fn index_chars(s: &str) -> (Vec<usize>, usize) {
    let mut byte_of_char = Vec::with_capacity(s.len() + 1);
    for (b, _) in s.char_indices() {
        byte_of_char.push(b);
    }
    byte_of_char.push(s.len());
    let total_chars = byte_of_char.len() - 1;
    (byte_of_char, total_chars)
}

/// Преобразует байтовый индекс в индекс символа (находит ближайшую левую границу символа).
fn byte_to_char_idx(byte_of_char: &[usize], byte_idx: usize) -> usize {
    match byte_of_char.binary_search(&byte_idx) {
        Ok(i) => i,
        Err(pos) => pos.saturating_sub(1),
    }
}

/// Гарантия верхней границы по символам (без паники).
fn ensure_max_chars(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let mut out = String::new();
    for _ in 0..max_chars {
        if let Some(ch) = it.next() {
            out.push(ch);
        } else {
            return out;
        }
    }
    if it.next().is_some() {
        out.push('…');
    }
    out
}
