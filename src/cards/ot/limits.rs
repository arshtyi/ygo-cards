use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};

use crate::cards::limit::{limit_for, parse_entry as parse_lf_entry};

#[derive(Debug)]
pub(super) struct LfLists {
    ocg: HashMap<i64, i64>,
    tcg: HashMap<i64, i64>,
}

impl LfLists {
    pub(super) fn for_card(&self, id: i64, alias: i64) -> Vec<i64> {
        vec![
            limit_for(&self.ocg, id, alias),
            limit_for(&self.tcg, id, alias),
        ]
    }
}

pub(super) fn read_lf_lists(path: &Path) -> Result<LfLists> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read forbidden list {}", path.display()))?;
    parse_lf_lists(&text)
}

fn parse_lf_lists(text: &str) -> Result<LfLists> {
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
            finish_lf_list(current_region, &mut current_entries, &mut ocg, &mut tcg);
            if ocg.is_some() && tcg.is_some() {
                break;
            }

            let region = if name.contains("TCG") {
                LfRegion::Tcg
            } else {
                LfRegion::Ocg
            };
            current_region = match region {
                LfRegion::Ocg if ocg.is_none() => Some(region),
                LfRegion::Tcg if tcg.is_none() => Some(region),
                _ => None,
            };
            continue;
        }

        if current_region.is_none() || line.starts_with('#') {
            continue;
        }

        if let Some((id, count)) = parse_lf_entry(line) {
            current_entries.insert(id, count);
        }
    }

    finish_lf_list(current_region, &mut current_entries, &mut ocg, &mut tcg);

    Ok(LfLists {
        ocg: ocg.context("missing latest OCG forbidden list")?,
        tcg: tcg.context("missing latest TCG forbidden list")?,
    })
}

fn finish_lf_list(
    region: Option<LfRegion>,
    entries: &mut HashMap<i64, i64>,
    ocg: &mut Option<HashMap<i64, i64>>,
    tcg: &mut Option<HashMap<i64, i64>>,
) {
    match region {
        Some(LfRegion::Ocg) if ocg.is_none() => {
            *ocg = Some(std::mem::take(entries));
        }
        Some(LfRegion::Tcg) if tcg.is_none() => {
            *tcg = Some(std::mem::take(entries));
        }
        _ => entries.clear(),
    }
}

#[derive(Debug, Clone, Copy)]
enum LfRegion {
    Ocg,
    Tcg,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_latest_ocg_and_tcg_lists() {
        let lists = parse_lf_lists(
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
    fn parses_lf_entry_lines() {
        assert_eq!(
            parse_lf_entry("08903700 0 --儀式魔人リリーサー"),
            Some((8903700, 0))
        );
        assert_eq!(parse_lf_entry("5318639 1 --旋风"), Some((5318639, 1)));
        assert_eq!(parse_lf_entry("#limit"), None);
        assert_eq!(parse_lf_entry("12345678 4 --invalid"), None);
    }
}
