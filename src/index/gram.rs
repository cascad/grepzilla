pub fn trigrams(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.chars();
    let Some(mut a) = it.next() else { return out };
    let Some(mut b) = it.next() else { return out };
    for c in it {
        let mut tri = String::new();
        tri.push(a); tri.push(b); tri.push(c);
        out.push(tri);
        a = b; b = c;
    }
    out
}

// todo rm тут больше аллокаций
// /// Возвращает все триграммы строки как Vec<String>.
// /// Без внешних ссылок — безопасно по лайфтаймам.
// pub fn trigrams(s: &str) -> Vec<String> {
//     let chars: Vec<char> = s.chars().collect();
//     let mut out = Vec::with_capacity(chars.len().saturating_sub(2));
//     for w in chars.windows(3) {
//         out.push(w.iter().collect());
//     }
//     out
// }