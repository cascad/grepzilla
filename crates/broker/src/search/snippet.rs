use regex::Regex;

pub fn build_snippet(rx: &Regex, text: &str, window: usize) -> String {
    if let Some(m) = rx.find(text) {
        let start = m.start();
        let end = m.end();
        let ctx = window.saturating_sub((end - start).min(window) + 2) / 2;
        let from = start.saturating_sub(ctx);
        let to = (end + ctx).min(text.len());
        let mut out = String::new();
        if from > 0 { out.push('…'); }
        out.push_str(&text[from..start]);
        out.push('[');
        out.push_str(&text[start..end]);
        out.push(']');
        out.push_str(&text[end..to]);
        if to < text.len() { out.push('…'); }
        out
    } else if text.len() > window {
        format!("{}…", &text[..window])
    } else {
        text.to_string()
    }
}