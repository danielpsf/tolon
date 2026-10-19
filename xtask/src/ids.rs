//! Pure id extraction: scenario ids (`S1.1`) and acceptance-criteria ids (`AC9.4`).

use std::collections::BTreeSet;

fn is_boundary(prev: Option<u8>) -> bool {
    prev.is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_'))
}

fn digits_end(bytes: &[u8], from: usize) -> usize {
    let mut end = from;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    end
}

/// Every `S<n>.<n>` and `AC<n>.<n>` token in `text`, de-duplicated.
pub fn extract_ids(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut ids = BTreeSet::new();
    for i in 0..bytes.len() {
        let prefix_len = if bytes[i..].starts_with(b"AC") {
            2
        } else if bytes[i] == b'S' {
            1
        } else {
            continue;
        };
        if !is_boundary(i.checked_sub(1).map(|p| bytes[p])) {
            continue;
        }
        let first_end = digits_end(bytes, i + prefix_len);
        if first_end == i + prefix_len || bytes.get(first_end) != Some(&b'.') {
            continue;
        }
        let second_end = digits_end(bytes, first_end + 1);
        if second_end == first_end + 1 {
            continue;
        }
        // Every byte of the token is ASCII, so the slice is on char boundaries.
        ids.insert(text[i..second_end].to_string());
    }
    ids
}

/// `s2_2_dst_change` -> `S2.2`, `ac9_4_x` -> `AC9.4`; anything else -> `None`.
pub fn test_name_to_id(name: &str) -> Option<String> {
    let (prefix, rest) = if let Some(rest) = name.strip_prefix("ac") {
        ("AC", rest)
    } else {
        ("S", name.strip_prefix('s')?)
    };
    let bytes = rest.as_bytes();
    let first_end = digits_end(bytes, 0);
    if first_end == 0 || bytes.get(first_end) != Some(&b'_') {
        return None;
    }
    let second_end = digits_end(bytes, first_end + 1);
    if second_end == first_end + 1 {
        return None;
    }
    if bytes.get(second_end).is_some_and(|b| *b != b'_') {
        return None;
    }
    Some(format!(
        "{prefix}{}.{}",
        &rest[..first_end],
        &rest[first_end + 1..second_end]
    ))
}

/// Ids from the names of every `fn` in a Rust source file.
pub fn rust_test_ids(src: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in src.lines() {
        let mut rest = line;
        while let Some(pos) = rest.find("fn ") {
            let before = rest.as_bytes()[..pos].last().copied();
            let after = &rest[pos + 3..];
            if is_boundary(before) {
                let name: String = after
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                ids.extend(test_name_to_id(&name));
            }
            rest = after;
        }
    }
    ids
}

/// Ids from `it(`/`test(`/`describe(` title lines in a TypeScript test file.
pub fn ts_test_ids(src: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in src.lines() {
        let is_title_line = ["it(", "test(", "describe(", "it.each", "test.each"]
            .iter()
            .any(|marker| line.contains(marker));
        if is_title_line {
            ids.extend(extract_ids(line));
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn extracts_scenario_and_ac_ids() {
        let text = "Scenario: S1.1 add; see AC9.4 and S2.10, (S3.1). ASCII1.2 and xS1.1 do not count; S4 and S4. neither.";
        assert_eq!(extract_ids(text), set(&["S1.1", "AC9.4", "S2.10", "S3.1"]));
    }

    #[test]
    fn maps_test_names_to_ids() {
        assert_eq!(test_name_to_id("s2_2_dst_change").as_deref(), Some("S2.2"));
        assert_eq!(test_name_to_id("ac9_4_start").as_deref(), Some("AC9.4"));
        assert_eq!(test_name_to_id("s10_12").as_deref(), Some("S10.12"));
        assert_eq!(test_name_to_id("s22_dst"), None);
        assert_eq!(test_name_to_id("sync_works"), None);
        assert_eq!(test_name_to_id("s2_2x"), None);
        assert_eq!(test_name_to_id("acme_1_2"), None);
    }

    #[test]
    fn finds_ids_in_rust_fn_names() {
        let src = "#[test]\nfn s2_2_dst_change() {}\n  pub fn helper() {}\nfn ac1_3_logs_clean() {}\nlet prefn s9_9_x";
        assert_eq!(rust_test_ids(src), set(&["S2.2", "AC1.3"]));
    }

    #[test]
    fn finds_ids_in_ts_titles() {
        let src = "describe('x', () => {\n  it('S1.1 adds a feed', () => {});\n  test(\"AC9.4 switch\", () => {});\n});\n// S5.5 in a comment\n";
        assert_eq!(ts_test_ids(src), set(&["S1.1", "AC9.4"]));
    }
}
