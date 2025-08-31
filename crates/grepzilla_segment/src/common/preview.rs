// crates/grepzilla_segment/src/common/preview.rs
use crate::StoredDoc;

/// Опции превью.
pub struct PreviewOpts<'a> {
    /// Поля по приоритету (будет взято первое подходящее).
    pub preferred_fields: &'a [&'a str],
    /// Целевое окно вывода в символах (не байтах).
    pub max_len: usize,
    /// Подсветить первую встречу иглы (уже нормализованной). Если None — просто обрезка.
    pub highlight_needle: Option<&'a str>,
}

/// Построить превью с подсветкой вокруг первого вхождения `highlight_needle`
/// или усечённый текст, если игла не найдена. Всегда работает по границам UTF-8 символов.
///
/// Выбор поля:
/// 1) если есть игла — берём первое поле из `preferred_fields`, где она встречается;
/// 2) если среди preferred нет, ищем любую пару (поле, текст) в документе, где она встречается;
/// 3) иначе — первое существующее поле из `preferred_fields`;
/// 4) иначе — первое попавшееся поле документа;
pub fn build_preview(doc: &StoredDoc, opts: PreviewOpts<'_>) -> String {
    // 1) выберем источник текста по правилам (см. докстринг)
    let binding = String::new();
    let text = pick_text_for_preview(doc, opts.preferred_fields, opts.highlight_needle)
        .unwrap_or(&binding);

    // 2) если игла есть и найдена — делаем сниппет по матчу; иначе — просто усечение
    match opts
        .highlight_needle
        .and_then(|n| (!n.is_empty()).then_some(n))
        .and_then(|n| find_substr(text, n))
    {
        Some((m_start_b, m_end_b)) => snippet_with_highlight(text, m_start_b, m_end_b, opts.max_len),
        None => truncate_chars_with_ellipsis(text, opts.max_len),
    }
}

/// Выбор текста для превью по приоритетам и наличию иглы.
///
/// Возвращает ссылку на строку из `doc.fields`.
fn pick_text_for_preview<'a>(
    doc: &'a StoredDoc,
    preferred: &[&str],
    needle: Option<&str>,
) -> Option<&'a String> {
    // Если есть игла: сперва пробуем preferred-поля, где она встречается
    if let Some(n) = needle {
        if !n.is_empty() {
            for f in preferred {
                if let Some(t) = doc.fields.get(*f) {
                    if find_substr(t, n).is_some() {
                        return Some(t);
                    }
                }
            }
            // затем любое поле с матчем
            for (_k, t) in &doc.fields {
                if find_substr(t, n).is_some() {
                    return Some(t);
                }
            }
        }
    }

    // Иначе (или если нигде нет иглы) — первое существующее из preferred
    for f in preferred {
        if let Some(t) = doc.fields.get(*f) {
            return Some(t);
        }
    }

    // В крайнем случае — первое поле документа
    doc.fields.values().next()
}

/// Нахождение подстроки (байтовые индексы) — строки уже нормализованы, можно использовать `find`.
fn find_substr(haystack: &str, needle: &str) -> Option<(usize, usize)> {
    haystack.find(needle).map(|s| (s, s + needle.len()))
}

/// Возвращает сниппет вокруг матча, гарантируя границы по символам и подсветку скобками.
fn snippet_with_highlight(s: &str, m_start_b: usize, m_end_b: usize, max_chars: usize) -> String {
    if max_chars == 0 || s.is_empty() {
        return String::new();
    }

    // Построим таблицу: char_idx -> byte_offset
    let (byte_of_char, total_chars) = index_chars(s);

    // Переведём байтовые индексы матча в индекс символов.
    let m_start_c = byte_to_char_idx(&byte_of_char, m_start_b);
    let m_end_c = byte_to_char_idx(&byte_of_char, m_end_b);
    let match_len_c = m_end_c.saturating_sub(m_start_c);

    // Бюджет на контекст по краям, учитывая скобки.
    let budget = max_chars.saturating_sub(match_len_c + 2);
    let ctx = budget / 2;

    let from_c = m_start_c.saturating_sub(ctx);
    let to_c = (m_end_c + ctx).min(total_chars);

    // Обратно в байты.
    let (from_b, to_b) = (byte_of_char[from_c], byte_of_char[to_c]);

    let mut out = String::new();

    if from_c > 0 {
        out.push('…');
    }
    // левая часть
    out.push_str(&s[from_b..m_start_b]);
    // подсветка
    out.push('[');
    out.push_str(&s[m_start_b..m_end_b]);
    out.push(']');
    // правая часть
    out.push_str(&s[m_end_b..to_b]);
    if to_c < total_chars {
        out.push('…');
    }

    // Подстраховка на случай округлений: уложим в max_chars+4 (запас на «…»/скобки)
    ensure_max_chars(&out, max_chars + 4)
}

/// Усечение по символам с многоточием; безопасно для UTF-8.
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
    // Пример: "привет" -> [0,2,4,6,8,10], len=6; добавляем s.len() как «конечную» границу.
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
