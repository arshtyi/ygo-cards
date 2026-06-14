use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

const CARDS_DB: &str = "assets/ot/cards.cdb";
const OUTPUT_JSON: &str = "output/ot.json";

#[derive(Debug, Serialize)]
struct OtCard {
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

fn read_cards(db_path: &Path) -> Result<Vec<OtCard>> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open cards database {}", db_path.display()))?;
    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, datas.attribute, datas.alias
            from datas
            left join texts on texts.id = datas.id
            order by datas.id
            ",
        )
        .context("failed to prepare card query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(CardRow {
                id: row.get(0)?,
                name: row.get(1)?,
                attribute: row.get(2)?,
                alias: row.get(3)?,
            })
        })
        .context("failed to query cards")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row) {
                    cards.push(card);
                }
            }
            Err(error) => eprintln!("skip card: failed to read row: {error}"),
        }
    }

    Ok(cards)
}

fn build_card(row: CardRow) -> Option<OtCard> {
    if row.id <= 0 {
        eprintln!("skip card: invalid id {}", row.id);
        return None;
    }

    let Some(name) = row.name else {
        eprintln!("skip card {}: missing name", row.id);
        return None;
    };

    if name.trim().is_empty() {
        eprintln!("skip card {}: empty name", row.id);
        return None;
    }

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!("skip card {}: invalid attribute {}", row.id, row.attribute);
        return None;
    };

    if row.alias < 0 {
        eprintln!("skip card {}: invalid alias {}", row.id, row.alias);
        return None;
    }

    Some(OtCard {
        id: row.id,
        name,
        attribute,
        alias: row.alias,
    })
}

fn normalize_attribute(raw_attribute: i64) -> Option<i64> {
    match raw_attribute {
        0x00 => Some(0),
        0x40 => Some(0),
        0x10 => Some(1),
        0x20 => Some(2),
        0x08 => Some(3),
        0x01 => Some(4),
        0x04 => Some(5),
        0x02 => Some(6),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_general_properties() {
        let card = OtCard {
            id: 89631139,
            name: String::from("Blue-Eyes White Dragon"),
            attribute: 1,
            alias: 0,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":89631139,"name":"Blue-Eyes White Dragon","attribute":1,"alias":0}"#
        );
    }

    #[test]
    fn normalizes_attribute_codes() {
        assert_eq!(normalize_attribute(0x00), Some(0));
        assert_eq!(normalize_attribute(0x40), Some(0));
        assert_eq!(normalize_attribute(0x10), Some(1));
        assert_eq!(normalize_attribute(0x20), Some(2));
        assert_eq!(normalize_attribute(0x08), Some(3));
        assert_eq!(normalize_attribute(0x01), Some(4));
        assert_eq!(normalize_attribute(0x04), Some(5));
        assert_eq!(normalize_attribute(0x02), Some(6));
        assert_eq!(normalize_attribute(0x30), None);
    }
}
