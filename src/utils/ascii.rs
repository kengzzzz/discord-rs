use std::cmp::Ordering;

const LIMIT: usize = 25;

#[inline]
pub fn ascii_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b | 0x20 } else { b }
}

#[inline]
pub fn ascii_upper(b: u8) -> u8 {
    if b.is_ascii_lowercase() { b & !0x20 } else { b }
}

pub fn cmp_ignore_ascii_case(a: &str, b: &str) -> Ordering {
    let mut ai = a.bytes();
    let mut bi = b.bytes();
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                if x == y {
                    continue;
                }
                let fx = ascii_lower(x);
                let fy = ascii_lower(y);
                if fx != fy {
                    return fx.cmp(&fy);
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
    hay.bytes()
        .take(needle.len())
        .zip(needle.bytes())
        .all(|(h, n)| h == n || h.eq_ignore_ascii_case(&n))
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
    let first = ascii_lower(nb[0]);
    let mut i = 0;
    let end = hb.len() - nlen;
    while i <= end {
        if ascii_lower(hb[i]) == first {
            let mut j = 1;
            while j < nlen && ascii_lower(hb[i + j]) == ascii_lower(nb[j]) {
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
        for s in data.iter().take(LIMIT).map(|e| get(e)) {
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
