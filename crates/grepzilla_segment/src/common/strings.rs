use serde_json::Value;

/// Ровно та же логика, что была в V1 collect_strings.
/// Верните сюда ваш фактический код нормализации, чтобы V1 и V2 совпадали.
pub fn collect_strings(json: &Value) -> Vec<(String, String)> {
    // TODO: замените на реальную V1-логику; ниже — безопасный заглушечный пример
    fn norm(s: &str) -> String {
        s.to_lowercase()
    }

    let mut out = Vec::new();
    if let Some(obj) = json.as_object() {
        for (k, v) in obj {
            match v {
                Value::String(s) => out.push((k.clone(), norm(s))),
                Value::Object(_) | Value::Array(_) => {
                    // рекурсивный обход, ключи через точку
                    collect_inner(k, v, &mut out, &norm);
                }
                _ => {}
            }
        }
    }
    out
}

fn collect_inner<F: Fn(&str) -> String>(
    prefix: &str,
    v: &serde_json::Value,
    out: &mut Vec<(String, String)>,
    norm: &F,
) {
    match v {
        serde_json::Value::String(s) => out.push((prefix.to_string(), norm(s))),
        serde_json::Value::Object(map) => {
            for (k, vv) in map {
                let key = format!("{prefix}.{k}");
                collect_inner(&key, vv, out, norm);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, vv) in arr.iter().enumerate() {
                let key = format!("{prefix}[{i}]");
                collect_inner(&key, vv, out, norm);
            }
        }
        _ => {}
    }
}
