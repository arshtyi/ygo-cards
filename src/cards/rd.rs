use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

const CARDS_DB: &str = "assets/rd/rd_standard.cdb";
const OUTPUT_JSON: &str = "output/rd.json";
const USELESS_HEADER_ROWS: i64 = 4;

#[derive(Debug, Serialize)]
struct RdCard {
    id: i64,
    name: String,
    attribute: i64,
    alias: i64,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    attribute: i64,
    alias: i64,
}

#[derive(Debug)]
pub struct WriteReport {
    pub path: PathBuf,
    pub cards_written: usize,
}

pub fn write_json() -> Result<WriteReport> {
    let cards = read_cards(Path::new(CARDS_DB))?;
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

fn read_cards(db_path: &Path) -> Result<Vec<RdCard>> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open RD cards database {}", db_path.display()))?;
    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, datas.attribute, datas.alias
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
                alias: row.get(3)?,
            })
        })
        .context("failed to query RD cards")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row) {
                    cards.push(card);
                }
            }
            Err(error) => eprintln!("skip RD card: failed to read row: {error}"),
        }
    }

    Ok(cards)
}

fn build_card(row: CardRow) -> Option<RdCard> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_general_properties() {
        let card = RdCard {
            id: 120100001,
            name: String::from("大道魔法-爆发"),
            attribute: 0,
            alias: 0,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120100001,"name":"大道魔法-爆发","attribute":0,"alias":0}"#
        );
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
    fn rejects_invalid_rows() {
        assert!(
            build_card(CardRow {
                id: 0,
                name: None,
                attribute: 0,
                alias: 0,
            })
            .is_none()
        );
        assert!(
            build_card(CardRow {
                id: 120100001,
                name: None,
                attribute: 0,
                alias: 0,
            })
            .is_none()
        );
        assert!(
            build_card(CardRow {
                id: 120100001,
                name: Some(String::from("  ")),
                attribute: 0,
                alias: 0,
            })
            .is_none()
        );
        assert!(
            build_card(CardRow {
                id: 120100001,
                name: Some(String::from("大道魔法-爆发")),
                attribute: 0x40,
                alias: 0,
            })
            .is_none()
        );
        assert!(
            build_card(CardRow {
                id: 120100001,
                name: Some(String::from("大道魔法-爆发")),
                attribute: 0,
                alias: -1,
            })
            .is_none()
        );
    }
}
