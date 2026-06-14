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
        .prepare("select id from datas order by id")
        .context("failed to prepare card id query")?;
    let rows = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .context("failed to query card ids")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(id) if id > 0 => cards.push(OtCard { id }),
            Ok(id) => eprintln!("skip card: invalid id {id}"),
            Err(error) => eprintln!("skip card: failed to read id: {error}"),
        }
    }

    Ok(cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_id_as_integer_property() {
        let card = OtCard { id: 89631139 };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(json, r#"{"id":89631139}"#);
    }
}
