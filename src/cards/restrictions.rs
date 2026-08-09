use std::collections::HashMap;

pub(super) fn restriction_for(entries: &HashMap<i64, i64>, id: i64, alias: i64) -> i64 {
    entries
        .get(&id)
        .or_else(|| (alias > 0).then(|| entries.get(&alias)).flatten())
        .copied()
        .unwrap_or(3)
}

pub(super) fn parse_restriction(line: &str) -> Option<(i64, i64)> {
    let entry = line.split_once("--").map_or(line, |(entry, _)| entry);
    let mut parts = entry.split_whitespace();
    let id = parts.next()?.parse().ok()?;
    let count = parts.next()?.parse().ok()?;

    (0..=3).contains(&count).then_some((id, count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_entries_and_ignores_comments() {
        assert_eq!(parse_restriction("123 1 -- limited"), Some((123, 1)));
        assert_eq!(parse_restriction("456 3"), Some((456, 3)));
        assert_eq!(parse_restriction("123 4"), None);
        assert_eq!(parse_restriction("invalid"), None);
    }

    #[test]
    fn resolves_direct_alias_and_default_limits() {
        let entries = HashMap::from([(10, 1), (20, 2)]);

        assert_eq!(restriction_for(&entries, 10, 0), 1);
        assert_eq!(restriction_for(&entries, 11, 20), 2);
        assert_eq!(restriction_for(&entries, 11, 0), 3);
    }
}
