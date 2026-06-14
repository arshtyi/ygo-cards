use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

const CARDS_DB: &str = "assets/rd/rd_standard.cdb";
const LFLIST: &str = "assets/rd/lflist.conf";
const OUTPUT_JSON: &str = "output/rd.json";
const USELESS_HEADER_ROWS: i64 = 4;

#[derive(Debug, Serialize)]
struct RdCard {
    id: i64,
    name: String,
    attribute: i64,
    legend: bool,
    lf: i64,
    alias: i64,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    attribute: i64,
    card_type: i64,
    alias: i64,
}

#[derive(Debug)]
pub struct WriteReport {
    pub path: PathBuf,
    pub cards_written: usize,
}

pub fn write_json() -> Result<WriteReport> {
    let lf_list = read_lf_list(Path::new(LFLIST))?;
    let cards = read_cards(Path::new(CARDS_DB), &lf_list)?;
    let path = PathBuf::from(OUTPUT_JSON);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    let file = File::create(&path)
        .with_context(|| format!("failed to create output file {}", path.display()))?;
    serde_json::to_writer_pretty(file, &cards)
        .with_context(|| format!("failed to write {}", path.display()))?;

    Ok(WriteReport {
        path,
        cards_written: cards.len(),
    })
}

fn read_cards(db_path: &Path, lf_list: &LfList) -> Result<Vec<RdCard>> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open RD cards database {}", db_path.display()))?;
    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, datas.attribute, datas.type, datas.alias
            from datas
            left join texts on texts.id = datas.id
            where datas.id not in (
                select id from datas order by id limit ?1
            )
            and datas.id not in (
                select id from texts order by id limit ?1
            )
            order by datas.id
            ",
        )
        .context("failed to prepare RD card query")?;
    let rows = statement
        .query_map([USELESS_HEADER_ROWS], |row| {
            Ok(CardRow {
                id: row.get(0)?,
                name: row.get(1)?,
                attribute: row.get(2)?,
                card_type: row.get(3)?,
                alias: row.get(4)?,
            })
        })
        .context("failed to query RD cards")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_list) {
                    cards.push(card);
                }
            }
            Err(error) => eprintln!("skip RD card: failed to read row: {error}"),
        }
    }

    Ok(cards)
}

fn build_card(row: CardRow, lf_list: &LfList) -> Option<RdCard> {
    if row.id <= 0 {
        eprintln!("skip RD card: invalid id {}", row.id);
        return None;
    }

    let Some(name) = row.name else {
        eprintln!("skip RD card {}: missing name", row.id);
        return None;
    };

    if name.trim().is_empty() {
        eprintln!("skip RD card {}: empty name", row.id);
        return None;
    }

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!(
            "skip RD card {}: invalid attribute {}",
            row.id, row.attribute
        );
        return None;
    };

    if row.alias < 0 {
        eprintln!("skip RD card {}: invalid alias {}", row.id, row.alias);
        return None;
    }

    Some(RdCard {
        id: row.id,
        name,
        attribute,
        legend: is_legend(row.card_type),
        lf: lf_list.for_card(row.id, row.alias),
        alias: row.alias,
    })
}

fn normalize_attribute(raw_attribute: i64) -> Option<i64> {
    match raw_attribute {
        0x00 => Some(0),
        0x10 => Some(0),
        0x20 => Some(1),
        0x08 => Some(2),
        0x01 => Some(3),
        0x04 => Some(4),
        0x02 => Some(5),
        _ => None,
    }
}

fn is_legend(raw_type: i64) -> bool {
    raw_type & 0x8 != 0
}

#[derive(Debug)]
struct LfList {
    entries: HashMap<i64, i64>,
}

impl LfList {
    fn for_card(&self, id: i64, alias: i64) -> i64 {
        self.entries
            .get(&id)
            .or_else(|| (alias > 0).then(|| self.entries.get(&alias)).flatten())
            .copied()
            .unwrap_or(3)
    }
}

fn read_lf_list(path: &Path) -> Result<LfList> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read RD forbidden list {}", path.display()))?;
    Ok(parse_lf_list(&text))
}

fn parse_lf_list(text: &str) -> LfList {
    let mut entries = HashMap::new();
    let mut section = LfSection::Other;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('#') {
            section = LfSection::from_header(line);
            continue;
        }

        if !section.is_restriction() {
            continue;
        }

        if let Some((id, count)) = parse_lf_entry(line) {
            entries.insert(id, count);
        }
    }

    LfList { entries }
}

fn parse_lf_entry(line: &str) -> Option<(i64, i64)> {
    let entry = line.split_once("--").map_or(line, |(entry, _)| entry);
    let mut parts = entry.split_whitespace();
    let id = parts.next()?.parse().ok()?;
    let count = parts.next()?.parse().ok()?;

    if (0..=3).contains(&count) {
        Some((id, count))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LfSection {
    Restriction,
    Other,
}

impl LfSection {
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
    fn serializes_general_properties() {
        let card = RdCard {
            id: 120100001,
            name: String::from("大道魔法-爆发"),
            attribute: 0,
            legend: false,
            lf: 3,
            alias: 0,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120100001,"name":"大道魔法-爆发","attribute":0,"legend":false,"lf":3,"alias":0}"#
        );
    }

    #[test]
    fn detects_legend_type_bit() {
        assert!(is_legend(0x8));
        assert!(is_legend(0x29));
        assert!(!is_legend(0x21));
    }

    #[test]
    fn normalizes_attribute_codes() {
        assert_eq!(normalize_attribute(0x00), Some(0));
        assert_eq!(normalize_attribute(0x10), Some(0));
        assert_eq!(normalize_attribute(0x20), Some(1));
        assert_eq!(normalize_attribute(0x08), Some(2));
        assert_eq!(normalize_attribute(0x01), Some(3));
        assert_eq!(normalize_attribute(0x04), Some(4));
        assert_eq!(normalize_attribute(0x02), Some(5));
        assert_eq!(normalize_attribute(0x40), None);
    }

    #[test]
    fn parses_forbidden_list_restrictions() {
        let list = parse_lf_list(
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
    fn parses_lf_entry_lines() {
        assert_eq!(
            parse_lf_entry("120226013 0 -- 业火之结界像"),
            Some((120226013, 0))
        );
        assert_eq!(
            parse_lf_entry("120217035 2 -- 革新制壶陶艺家"),
            Some((120217035, 2))
        );
        assert_eq!(
            parse_lf_entry("120102002 $legend_monster 1 -- 时间魔术师"),
            None
        );
        assert_eq!(parse_lf_entry("120102002 4 --invalid"), None);
    }

    #[test]
    fn rejects_invalid_rows() {
        let lf_list = LfList {
            entries: HashMap::new(),
        };

        assert!(
            build_card(
                CardRow {
                    id: 0,
                    name: None,
                    attribute: 0,
                    card_type: 0,
                    alias: 0,
                },
                &lf_list
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: None,
                    attribute: 0,
                    card_type: 0,
                    alias: 0,
                },
                &lf_list
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("  ")),
                    attribute: 0,
                    card_type: 0,
                    alias: 0,
                },
                &lf_list
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    attribute: 0x40,
                    card_type: 0,
                    alias: 0,
                },
                &lf_list
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    attribute: 0,
                    card_type: 0,
                    alias: -1,
                },
                &lf_list
            )
            .is_none()
        );
    }
}
