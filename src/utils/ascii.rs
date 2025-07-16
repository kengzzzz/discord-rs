use std::cmp::Ordering;

const LIMIT: usize = 25;

pub fn ascii_fold(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + (b'a' - b'A')
    } else {
        b
    }
}

pub fn ascii_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b | 0x20 } else { b }
}

pub fn ascii_upper(b: u8) -> u8 {
    if b.is_ascii_lowercase() { b & !0x20 } else { b }
}

pub fn ascii_eq_ignore_case(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .all(|(x, y)| ascii_fold(x) == ascii_fold(y))
}

pub fn cmp_ignore_ascii_case(a: &str, b: &str) -> Ordering {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let n = ab.len().min(bb.len());

    for i in 0..n {
        let ac = ascii_fold(ab[i]);
        let bc = ascii_fold(bb[i]);
        if ac < bc {
            return Ordering::Less;
        } else if ac > bc {
            return Ordering::Greater;
        }
    }

    ab.len().cmp(&bb.len())
}

pub fn ascii_starts_with_icase(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let nb = needle.as_bytes();
    if nb.len() > hb.len() {
        return false;
    }
    for i in 0..nb.len() {
        if ascii_fold(hb[i]) != ascii_fold(nb[i]) {
            return false;
        }
    }
    true
}

pub fn collect_prefix_icase<T>(data: &[T], prefix: &str, get: impl Fn(&T) -> &str) -> Vec<String> {
    let start = data.partition_point(|e| cmp_ignore_ascii_case(get(e), prefix) == Ordering::Less);
    if start == data.len() || !ascii_starts_with_icase(get(&data[start]), prefix) {
        return Vec::new();
    }
    let end = data.partition_point(|e| {
        let s = get(e);
        match cmp_ignore_ascii_case(s, prefix) {
            Ordering::Less => true,
            _ => ascii_starts_with_icase(s, prefix),
        }
    });
    data[start..end]
        .iter()
        .take(LIMIT)
        .map(|e| get(e).to_owned())
        .collect()
}
