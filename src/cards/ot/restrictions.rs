use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::cards::restrictions::{parse_restriction, restriction_for};

#[derive(Debug)]
pub(super) struct ForbiddenLists {
    ocg: HashMap<i64, i64>,
    tcg: HashMap<i64, i64>,
}

impl ForbiddenLists {
    pub(super) fn for_card(&self, id: i64, alias: i64) -> Vec<i64> {
        vec![
            restriction_for(&self.ocg, id, alias),
            restriction_for(&self.tcg, id, alias),
        ]
    }
}

pub(super) fn read_forbidden_lists(path: &Path) -> Result<ForbiddenLists> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read forbidden list {}", path.display()))?;
    parse_forbidden_lists(&text)
}

fn parse_forbidden_lists(text: &str) -> Result<ForbiddenLists> {
    let mut ocg = None;
    let mut tcg = None;
    let mut current_region = None;
    let mut current_entries = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line.strip_prefix('!') {
            finish_current_list(current_region, &mut current_entries, &mut ocg, &mut tcg);
            if ocg.is_some() && tcg.is_some() {
                break;
            }

            let region = if name.contains("TCG") {
                Region::Tcg
            } else {
                Region::Ocg
            };
            current_region = match region {
                Region::Ocg if ocg.is_none() => Some(region),
                Region::Tcg if tcg.is_none() => Some(region),
                _ => None,
            };
            continue;
        }

        if current_region.is_none() || line.starts_with('#') {
            continue;
        }

        if let Some((id, count)) = parse_restriction(line) {
            current_entries.insert(id, count);
        }
    }

    finish_current_list(current_region, &mut current_entries, &mut ocg, &mut tcg);

    Ok(ForbiddenLists {
        ocg: ocg.context("missing latest OCG forbidden list")?,
        tcg: tcg.context("missing latest TCG forbidden list")?,
    })
}

fn finish_current_list(
    region: Option<Region>,
    entries: &mut HashMap<i64, i64>,
    ocg: &mut Option<HashMap<i64, i64>>,
    tcg: &mut Option<HashMap<i64, i64>>,
) {
    match region {
        Some(Region::Ocg) if ocg.is_none() => {
            *ocg = Some(std::mem::take(entries));
        }
        Some(Region::Tcg) if tcg.is_none() => {
            *tcg = Some(std::mem::take(entries));
        }
        _ => entries.clear(),
    }
}

#[derive(Debug, Clone, Copy)]
enum Region {
    Ocg,
    Tcg,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_ocg_and_tcg_lists() {
        let lists = parse_forbidden_lists(
            "
            #[2026.4][2026.5 TCG]
            !2026.4
            #forbidden
            11111111 0 --A
            #limit
            22222222 1 --B
            !2026.5 TCG
            #semi limit
            22222222 2 --B
            33333333 0 --C
            !2026.1
            11111111 3 --old
            ",
        )
        .unwrap();

        assert_eq!(lists.for_card(11111111, 0), vec![0, 3]);
        assert_eq!(lists.for_card(22222222, 0), vec![1, 2]);
        assert_eq!(lists.for_card(33333333, 0), vec![3, 0]);
        assert_eq!(lists.for_card(44444444, 0), vec![3, 3]);
        assert_eq!(lists.for_card(55555555, 11111111), vec![0, 3]);
    }

    #[test]
    fn parses_restriction_lines() {
        assert_eq!(
            parse_restriction("08903700 0 --儀式魔人リリーサー"),
            Some((8903700, 0))
        );
        assert_eq!(parse_restriction("5318639 1 --旋风"), Some((5318639, 1)));
        assert_eq!(parse_restriction("#limit"), None);
        assert_eq!(parse_restriction("12345678 4 --invalid"), None);
    }
}
