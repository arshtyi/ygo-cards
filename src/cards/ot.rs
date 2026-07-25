mod limits;
mod normalization;
mod types;

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use super::{LfSummary, WriteReport, images::ImageResolver};
use crate::json::write_pretty_sorted;

use self::{
    limits::{LfLists, read_lf_lists},
    normalization::{
        normalize_atk, normalize_def, normalize_description, normalize_level,
        normalize_link_marker, normalize_link_value, normalize_pendulum_description,
        normalize_pendulum_scale, normalize_rank,
    },
    types::{normalize_attribute, parse_card_type},
};

const CARDS_DB: &str = "assets/ot/cards.cdb";
const LFLIST: &str = "assets/ot/lflist.conf";
const OUTPUT_JSON: &str = "output/ot.json";

#[derive(Debug, Serialize)]
struct OtCard {
    id: i64,
    name: String,
    attribute: i64,
    image: i64,
    description: String,
    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "pendulumDescription"
    )]
    pendulum_description: Option<String>,
    alias: i64,
    r#type: Vec<String>,
    lf: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atk: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    def: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rank: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "pendulumScale")]
    pendulum_scale: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "linkValue")]
    link_value: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "linkMarker")]
    link_marker: Option<Vec<i64>>,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    description: Option<String>,
    attribute: i64,
    alias: i64,
    card_type: i64,
    race: i64,
    atk: i64,
    defense: i64,
    level: i64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub check_images: bool,
    pub skip_image_failures: bool,
}

pub fn write_json(options: BuildOptions) -> Result<WriteReport> {
    let lf_lists = read_lf_lists(Path::new(LFLIST))?;
    let mut images =
        ImageResolver::new(options.check_images, options.skip_image_failures)?;
    let read_report = read_cards(Path::new(CARDS_DB), &lf_lists, &mut images)?;
    let path = PathBuf::from(OUTPUT_JSON);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    write_pretty_sorted(&path, &read_report.cards)?;

    Ok(WriteReport {
        label: "OT",
        path,
        cards_written: read_report.cards.len(),
        cards_skipped: read_report.cards_skipped,
        lf_summaries: summarize_lf(&read_report.cards),
        image_summary: images.summary(),
        image_failures: images.failures().to_vec(),
    })
}

struct ReadCardsReport {
    cards: Vec<OtCard>,
    cards_skipped: usize,
}

fn read_cards(
    db_path: &Path,
    lf_lists: &LfLists,
    images: &mut ImageResolver,
) -> Result<ReadCardsReport> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open cards database {}", db_path.display()))?;
    let total_rows = connection
        .query_row("select count(*) from datas", [], |row| row.get::<_, i64>(0))
        .context("failed to count OT card rows")? as usize;
    images.start_progress(total_rows, "checking OT images");

    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, texts.desc, datas.attribute, datas.alias, datas.type, datas.race, datas.atk, datas.def, datas.level
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
                description: row.get(2)?,
                attribute: row.get(3)?,
                alias: row.get(4)?,
                card_type: row.get(5)?,
                race: row.get(6)?,
                atk: row.get(7)?,
                defense: row.get(8)?,
                level: row.get(9)?,
            })
        })
        .context("failed to query cards")?;

    let mut cards = Vec::new();
    let mut cards_skipped = 0;
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_lists, images) {
                    cards.push(card);
                } else {
                    cards_skipped += 1;
                }
            }
            Err(error) => {
                cards_skipped += 1;
                eprintln!("skip OT card row: failed to read database row: {error}");
            }
        }
        images.advance_progress();
    }
    images.finish_progress();

    Ok(ReadCardsReport {
        cards,
        cards_skipped,
    })
}

fn build_card(
    row: CardRow,
    lf_lists: &LfLists,
    images: &mut ImageResolver,
) -> Option<OtCard> {
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

    let Some(raw_description) = row.description else {
        eprintln!("skip OT card {} ({}): missing description", row.id, name);
        return None;
    };

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!(
            "skip OT card {} ({}): invalid attribute {} ({:#x})",
            row.id, name, row.attribute, row.attribute
        );
        return None;
    };

    if row.alias < 0 {
        eprintln!(
            "skip OT card {} ({}): invalid alias {}",
            row.id, name, row.alias
        );
        return None;
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!(
            "skip OT card {} ({}): invalid type {} ({:#x}) or race {} ({:#x})",
            row.id, name, row.card_type, row.card_type, row.race, row.race
        );
        return None;
    };

    let Some(description) = normalize_description(&raw_description, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid description for type {:?}",
            row.id, name, card_type
        );
        return None;
    };
    let Some(pendulum_description) = normalize_pendulum_description(&raw_description, &card_type)
    else {
        eprintln!(
            "skip OT card {} ({}): invalid pendulum description",
            row.id, name
        );
        return None;
    };
    let Some(atk) = normalize_atk(row.atk, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid atk {}",
            row.id, name, row.atk
        );
        return None;
    };
    let Some(def) = normalize_def(row.defense, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid def {}",
            row.id, name, row.defense
        );
        return None;
    };
    let Some(level) = normalize_level(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid level {}",
            row.id, name, row.level
        );
        return None;
    };
    let Some(rank) = normalize_rank(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid rank {}",
            row.id, name, row.level
        );
        return None;
    };
    let Some(pendulum_scale) = normalize_pendulum_scale(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid pendulum scale {}",
            row.id, name, row.level
        );
        return None;
    };
    let Some(link_value) = normalize_link_value(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid link value {}",
            row.id, name, row.level
        );
        return None;
    };
    let Some(link_marker) = normalize_link_marker(row.defense, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid link marker {}",
            row.id, name, row.defense
        );
        return None;
    };

    let Some(image) = images.resolve("OT", row.id, &name, row.alias) else {
        eprintln!(
            "skip OT card {} ({}): primary and alias image checks failed",
            row.id, name
        );
        return None;
    };

    Some(OtCard {
        id: row.id,
        name,
        attribute,
        image,
        description,
        pendulum_description,
        alias: row.alias,
        r#type: card_type,
        lf: lf_lists.for_card(row.id, row.alias),
        atk,
        def,
        level,
        rank,
        pendulum_scale,
        link_value,
        link_marker,
    })
}

fn summarize_lf(cards: &[OtCard]) -> Vec<LfSummary> {
    let mut ocg = [0; 4];
    let mut tcg = [0; 4];

    for card in cards.iter().filter(|card| card.alias == 0) {
        if let Some(limit) = card
            .lf
            .first()
            .and_then(|limit| usize::try_from(*limit).ok())
        {
            if let Some(count) = ocg.get_mut(limit) {
                *count += 1;
            }
        }
        if let Some(limit) = card
            .lf
            .get(1)
            .and_then(|limit| usize::try_from(*limit).ok())
        {
            if let Some(count) = tcg.get_mut(limit) {
                *count += 1;
            }
        }
    }

    vec![
        LfSummary {
            label: "OT OCG",
            counts: ocg,
        },
        LfSummary {
            label: "OT TCG",
            counts: tcg,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    #[test]
    fn serializes_general_properties() {
        let card = OtCard {
            id: 89631139,
            name: String::from("Blue-Eyes White Dragon"),
            attribute: 1,
            image: 89631139,
            description: String::from("A legendary dragon."),
            pendulum_description: None,
            alias: 0,
            r#type: labels(&["怪兽", "龙族", "通常"]),
            lf: vec![3, 1],
            atk: Some(3000),
            def: Some(2500),
            level: Some(8),
            rank: None,
            pendulum_scale: None,
            link_value: None,
            link_marker: None,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":89631139,"name":"Blue-Eyes White Dragon","attribute":1,"image":89631139,"description":"A legendary dragon.","alias":0,"type":["怪兽","龙族","通常"],"lf":[3,1],"atk":3000,"def":2500,"level":8}"#
        );
    }

    #[test]
    fn summarizes_limits_for_original_cards_only() {
        let card = |alias, lf| OtCard {
            id: 1,
            name: String::new(),
            attribute: 0,
            image: 1,
            description: String::new(),
            pendulum_description: None,
            alias,
            r#type: Vec::new(),
            lf,
            atk: None,
            def: None,
            level: None,
            rank: None,
            pendulum_scale: None,
            link_value: None,
            link_marker: None,
        };
        let summaries = summarize_lf(&[
            card(0, vec![0, 1]),
            card(123, vec![0, 1]),
            card(0, vec![2, 3]),
        ]);

        assert_eq!(summaries[0].counts, [1, 0, 1, 0]);
        assert_eq!(summaries[1].counts, [0, 1, 0, 1]);
    }
}
