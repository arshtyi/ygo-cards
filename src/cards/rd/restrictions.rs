use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::cards::restrictions::{parse_restriction, restriction_for};

#[derive(Debug, Default)]
pub(super) struct ForbiddenList {
    entries: HashMap<i64, i64>,
}

impl ForbiddenList {
    pub(super) fn for_card(&self, id: i64, alias: i64) -> i64 {
        restriction_for(&self.entries, id, alias)
    }
}

pub(super) fn read_forbidden_list(path: &Path) -> Result<ForbiddenList> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read RD forbidden list {}", path.display()))?;
    Ok(parse_forbidden_list(&text))
}

fn parse_forbidden_list(text: &str) -> ForbiddenList {
    let mut entries = HashMap::new();
    let mut section = Section::Other;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            section = Section::from_header(line);
            continue;
        }

        if !section.is_restriction() {
            continue;
        }

        if let Some((id, count)) = parse_restriction(line) {
            entries.insert(id, count);
        }
    }

    ForbiddenList { entries }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Restriction,
    Other,
}

impl Section {
    fn from_header(line: &str) -> Self {
        match line.trim_start_matches('#').to_ascii_lowercase().as_str() {
            "forbidden" | "limit" | "semi-limit" | "semi limit" => Self::Restriction,
            _ => Self::Other,
        }
    }

    fn is_restriction(self) -> bool {
        self == Self::Restriction
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forbidden_list_restrictions() {
        let list = parse_forbidden_list(
            "
            #[RD]
            !RD
            #Legend
            $legend_monster 1
            111111111 $legend_monster 1 --legend
            222222222 1 --legend placeholder
            #Forbidden
            333333333 0 --forbidden
            #Limit
            444444444 1 --limit
            #Semi-Limit
            555555555 2 --semi
            #Other
            666666666 0 --ignored
            ",
        );

        assert_eq!(list.for_card(111111111, 0), 3);
        assert_eq!(list.for_card(222222222, 0), 3);
        assert_eq!(list.for_card(333333333, 0), 0);
        assert_eq!(list.for_card(444444444, 0), 1);
        assert_eq!(list.for_card(555555555, 0), 2);
        assert_eq!(list.for_card(666666666, 0), 3);
        assert_eq!(list.for_card(777777777, 333333333), 0);
    }

    #[test]
    fn parses_restriction_lines() {
        assert_eq!(
            parse_restriction("120226013 0 -- 业火之结界像"),
            Some((120226013, 0))
        );
        assert_eq!(
            parse_restriction("120217035 2 -- 革新制壶陶艺家"),
            Some((120217035, 2))
        );
        assert_eq!(
            parse_restriction("120102002 $legend_monster 1 -- 时间魔术师"),
            None
        );
        assert_eq!(parse_restriction("120102002 4 --invalid"), None);
    }
}
