// crates/grepzilla_segment/src/common/preview.rs
use crate::StoredDoc;

/// Опции превью.
pub struct PreviewOpts<'a> {
    /// Поля по приоритету (будет взято первое существующее).
    pub preferred_fields: &'a [&'a str],
    /// Целевое окно вывода в символах (не байтах).
    pub max_len: usize,
    /// Подсветить первую встречу иглы (уже нормализованную).
    /// Если None — просто обрезка.
    pub highlight_needle: Option<&'a str>,
}

/// Построить превью с подсветкой вокруг первого вхождения `highlight_needle`
/// (нечувствительно к регистру, с fallback по укорачиванию до длины >= 3),
/// или усечённый текст, если игла не найдена. Всегда по границам UTF-8.
pub fn build_preview(doc: &StoredDoc, opts: PreviewOpts<'_>) -> String {
    // 1) выбрать источник текста
    let binding = String::new();
    let text = pick_field_text(doc, opts.preferred_fields)
        .or_else(|| doc.fields.values().next())
        .unwrap_or(&binding);

    // 2) если игла есть и найдена — делаем сниппет по матчу; иначе — просто усечение
    match opts
        .highlight_needle
        .and_then(|n| find_ci_with_fallback(text, n, 3))
    {
        Some((m_start_b, m_end_b)) => {
            snippet_with_highlight(text, m_start_b, m_end_b, opts.max_len)
        }
        None => truncate_chars_with_ellipsis(text, opts.max_len),
    }
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

/// Регистронезависимый поиск подстроки с «снижением» иглы:
/// сначала ищем целиком, если не нашли — обрезаем с конца до min_len (в символах).
/// Возвращает (start_byte, end_byte) в ОРИГИНАЛЬНОЙ строке.
fn find_ci_with_fallback(hay: &str, needle: &str, min_len: usize) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }

    // Быстрый путь: точное совпадение (регистронезависимо).
    if let Some((s, e)) = find_ci(hay, needle) {
        return Some((s, e));
    }

    // Fallback: постепенное укорачивание иглы до min_len символов.
    let mut chars: Vec<char> = needle.chars().collect();
    while chars.len() > min_len {
        chars.pop();
        let shorter: String = chars.iter().collect();
        if let Some((s, e)) = find_ci(hay, &shorter) {
            return Some((s, e));
        }
    }
    None
}

/// Регистронезависимый поиск needle в hay, возвращает байтовые границы оригинальной строки.
/// Для кириллицы регистр-преобразование сохраняет длину символа, так что смещения совпадают.
fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let hay_lc = hay.to_lowercase();
    let nee_lc = needle.to_lowercase();
    if let Some(pos) = hay_lc.find(&nee_lc) {
        // Предполагаем, что длина в байтах совпадает на этих языках (кириллица/латиница без специальных кейсов).
        let end = pos + nee_lc.len();
        // Перестраховка: убедимся, что это валидные границы UTF-8 оригинала.
        if is_char_boundary(hay, pos) && is_char_boundary(hay, end) {
            return Some((pos, end));
        }
        // Если вдруг границы не совпали (экзотичные кейсы) — медленный путь через индексы символов.
        return find_ci_safe(hay, &nee_lc);
    }
    None
}

/// Медленный, но безопасный CI-поиск: сравниваем посимвольно в нижнем регистре.
fn find_ci_safe(hay: &str, nee_lc: &str) -> Option<(usize, usize)> {
    let (byte_of_char, total_chars) = index_chars(hay);
    let nee_chars: Vec<char> = nee_lc.chars().collect();
    let nee_len = nee_chars.len();

    if nee_len == 0 {
        return None;
    }

    // Проходим окно длиной nee_len по символам.
    for start_c in 0..=total_chars.saturating_sub(nee_len) {
        let end_c = start_c + nee_len;
        // Получим срез по символам и сравним в нижнем регистре.
        let s_b = byte_of_char[start_c];
        let e_b = byte_of_char[end_c];
        let frag = &hay[s_b..e_b];
        if frag.to_lowercase() == nee_lc {
            return Some((s_b, e_b));
        }
    }
    None
}

#[inline]
fn is_char_boundary(s: &str, b: usize) -> bool {
    b == s.len() || s.is_char_boundary(b)
}

/// Возвращает сниппет вокруг матча, гарантируя границы по символам и подсветку скобками.
pub fn snippet_with_highlight(
    s: &str,
    m_start_b: usize,
    m_end_b: usize,
    max_chars: usize,
) -> String {
    if max_chars == 0 || s.is_empty() {
        return String::new();
    }

    // Построим таблицу: char_idx -> byte_offset
    let (byte_of_char, total_chars) = index_chars(s);

    // Переведём байтовые индексы матча в индекс символов (через бинарный поиск).
    let m_start_c = byte_to_char_idx(&byte_of_char, m_start_b);
    let m_end_c = byte_to_char_idx(&byte_of_char, m_end_b);
    let match_len_c = m_end_c.saturating_sub(m_start_c);

    // Сколько контекста дать по краям, учитывая скобки и возможные многоточия.
    // +2 символа — это '[' и ']'.
    let budget = max_chars.saturating_sub(match_len_c + 2);
    let ctx = budget / 2;

    let from_c = m_start_c.saturating_sub(ctx);
    let to_c = (m_end_c + ctx).min(total_chars);

    // Переведём обратно в байтовые границы.
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

    // Если по какой-то причине получился слишком длинный — подстрахуемся усечением
    ensure_max_chars(&out, max_chars + 4) // небольшой запас на «…»/скобки
}

/// Усечение по символам с «…».
pub fn truncate_chars_with_ellipsis(s: &str, max_chars: usize) -> String {
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
    // Пример: "привет" -> [0,2,4,6,8,10], len=6; в конце добавляем s.len() как «конечную» границу
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
    // Если есть остаток — добавим многоточие, чтобы явно показать усечение.
    if it.next().is_some() {
        out.push('…');
    }
    out
}
