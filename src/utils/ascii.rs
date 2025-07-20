use std::cmp::Ordering;

const LIMIT: usize = 25;

pub fn cmp_ignore_ascii_case(a: &str, b: &str) -> Ordering {
    let mut ai = a.bytes();
    let mut bi = b.bytes();
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                if x == y {
                    continue;
                }
                let x = x.to_ascii_lowercase();
                let y = y.to_ascii_lowercase();
                if x != y {
                    return x.cmp(&y);
                }
            }
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (None, None) => return Ordering::Equal,
        }
    }
}

pub fn ascii_starts_with_icase(hay: &str, needle: &str) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    hay.as_bytes()
        .get(..needle.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(needle.as_bytes()))
}

pub fn ascii_contains_icase(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    let nlen = nb.len();
    if nlen == 0 {
        return true;
    }
    if nlen > hb.len() {
        return false;
    }
    let first = nb[0].to_ascii_lowercase();
    let mut i = 0;
    let end = hb.len() - nlen;
    while i <= end {
        if hb[i].to_ascii_lowercase() == first {
            let mut j = 1;
            while j < nlen && hb[i + j].eq_ignore_ascii_case(&nb[j]) {
                j += 1;
            }
            if j == nlen {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn collect_prefix_icase<T, F>(data: &[T], prefix: &str, get: F) -> Vec<String>
where
    F: Fn(&T) -> &str,
{
    if prefix.is_empty() {
        let mut out = Vec::with_capacity(LIMIT);
        for s in data.iter().take(LIMIT).map(&get) {
            out.push(s.to_owned());
        }
        return out;
    }
    let start = data.partition_point(|e| cmp_ignore_ascii_case(get(e), prefix) == Ordering::Less);
    if start == data.len() || !ascii_starts_with_icase(get(&data[start]), prefix) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(LIMIT);
    for e in &data[start..] {
        let s = get(e);
        if !ascii_starts_with_icase(s, prefix) {
            break;
        }
        out.push(s.to_owned());
        if out.len() == LIMIT {
            break;
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/ascii.rs"]
mod tests;
