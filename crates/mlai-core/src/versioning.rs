use std::cmp::Ordering;

/// Dotted-version comparison ("1.2.0" vs "1.10.0", element-wise as integers,
/// non-numeric segments treated as 0, the shorter side implicitly
/// zero-padded). Ported from cinepipe-installer's `compare_version`.
pub fn compare_version(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| -> Vec<i64> {
        if s.is_empty() {
            vec![0]
        } else {
            s.split('.').map(|seg| seg.parse().unwrap_or(0)).collect()
        }
    };
    let pa = parts(a);
    let pb = parts(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn equal_versions_compare_equal() {
        assert_eq!(compare_version("1.2.0", "1.2.0"), Ordering::Equal);
    }

    #[test]
    fn compares_numerically_not_lexically() {
        // Lexical comparison would put "1.10.0" before "1.2.0" — must not happen.
        assert_eq!(compare_version("1.10.0", "1.2.0"), Ordering::Greater);
    }

    #[test]
    fn shorter_version_is_implicitly_zero_padded() {
        assert_eq!(compare_version("1.2", "1.2.0"), Ordering::Equal);
        assert_eq!(compare_version("1.2.1", "1.2"), Ordering::Greater);
    }

    #[test]
    fn non_numeric_segments_are_treated_as_zero() {
        assert_eq!(compare_version("1.x.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn empty_string_compares_as_all_zero() {
        assert_eq!(compare_version("", "0.0.0"), Ordering::Equal);
    }
}
