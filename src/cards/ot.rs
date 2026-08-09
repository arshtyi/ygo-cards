mod classification;
mod normalization;
mod restrictions;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use super::{
    CardCollection, DatasetReport, GenerationOptions, images::ImageResolver,
    mappings::ensure_ot_mappings, rejection, write_dataset,
};
use crate::environment::Environment;

use self::{
    classification::{map_attribute, map_card_types},
    normalization::{
        normalize_atk, normalize_def, normalize_description, normalize_level,
        normalize_link_markers, normalize_link_value, normalize_pendulum_description,
        normalize_pendulum_scale, normalize_rank,
    },
    restrictions::{ForbiddenLists, read_forbidden_lists},
};

const DATABASE_PATH: &str = "assets/ot/cards.cdb";
const FORBIDDEN_LIST_PATH: &str = "assets/ot/lflist.conf";
const OUTPUT_PATH: &str = "output/ot.json";

#[derive(Debug, Serialize)]
struct Card {
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
    #[serde(rename = "lf")]
    restrictions: Vec<i64>,
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
    link_markers: Option<Vec<i64>>,
}

#[derive(Debug)]
struct DatabaseRow {
    id: i64,
    name: Option<String>,
    description: Option<String>,
    attribute_code: i64,
    alias: i64,
    type_flags: i64,
    race_code: i64,
    atk: i64,
    defense: i64,
    level: i64,
}

pub(crate) fn generate(options: GenerationOptions) -> Result<DatasetReport> {
    ensure_ot_mappings()?;
    let forbidden_lists = read_forbidden_lists(Path::new(FORBIDDEN_LIST_PATH))?;
    let mut images = ImageResolver::new(options)?;
    let collection = read_cards(Path::new(DATABASE_PATH), &forbidden_lists, &mut images)?;
    let path = PathBuf::from(OUTPUT_PATH);

    write_dataset(&path, &collection.cards)?;

    Ok(DatasetReport {
        environment: Environment::Ot,
        path,
        cards_written: collection.cards.len(),
        cards_skipped: collection.skipped,
        image_summary: images.summary(),
        image_failures: images.failures().to_vec(),
    })
}

fn read_cards(
    db_path: &Path,
    forbidden_lists: &ForbiddenLists,
    images: &mut ImageResolver,
) -> Result<CardCollection<Card>> {
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
        .context("failed to prepare OT card query")?;
    let rows = statement
        .query_map([], |row| {
            Ok(DatabaseRow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                attribute_code: row.get(3)?,
                alias: row.get(4)?,
                type_flags: row.get(5)?,
                race_code: row.get(6)?,
                atk: row.get(7)?,
                defense: row.get(8)?,
                level: row.get(9)?,
            })
        })
        .context("failed to query OT cards")?;

    let mut cards = Vec::new();
    let mut cards_skipped = 0;
    for (row_index, row) in rows.enumerate() {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, forbidden_lists, images) {
                    cards.push(card);
                } else {
                    cards_skipped += 1;
                }
            }
            Err(error) => {
                cards_skipped += 1;
                rejection::database_row(Environment::Ot, row_index + 1, &error);
            }
        }
        images.advance_progress();
    }
    images.finish_progress();

    Ok(CardCollection {
        cards,
        skipped: cards_skipped,
    })
}

fn build_card(
    row: DatabaseRow,
    forbidden_lists: &ForbiddenLists,
    images: &mut ImageResolver,
) -> Option<Card> {
    if row.id <= 0 {
        rejection::card(
            Environment::Ot,
            row.id,
            row.name.as_deref(),
            format_args!("ID must be positive"),
        );
        return None;
    }

    let Some(name) = row.name else {
        rejection::card(
            Environment::Ot,
            row.id,
            None,
            format_args!("name is missing from the texts table"),
        );
        return None;
    };
    let rejection = rejection::Card::new(Environment::Ot, row.id, &name);

    if name.trim().is_empty() {
        rejection.warning(format_args!("name is empty or whitespace-only"));
        return None;
    }

    let Some(raw_description) = row.description else {
        rejection.warning(format_args!("description is missing from the texts table"));
        return None;
    };

    let Some(attribute) = map_attribute(row.attribute_code) else {
        rejection.warning(format_args!(
            "unsupported attribute value={} ({:#x})",
            row.attribute_code, row.attribute_code
        ));
        return None;
    };

    if row.alias < 0 {
        rejection.warning(format_args!(
            "alias must be non-negative; alias={}",
            row.alias
        ));
        return None;
    }

    let Some(card_type) = map_card_types(row.type_flags, row.race_code) else {
        rejection.warning(format_args!(
            "unsupported type or race bitmask; type={} ({:#x}) race={} ({:#x})",
            row.type_flags, row.type_flags, row.race_code, row.race_code
        ));
        return None;
    };

    let Some(description) = normalize_description(&raw_description, &card_type) else {
        rejection.warning(format_args!(
            "description format is invalid for card_type={card_type:?}"
        ));
        return None;
    };
    let Some(pendulum_description) = normalize_pendulum_description(&raw_description, &card_type)
    else {
        rejection.warning(format_args!(
            "pendulum description format is invalid for card_type={card_type:?}"
        ));
        return None;
    };
    let Some(atk) = normalize_atk(row.atk, &card_type) else {
        rejection.warning(format_args!(
            "invalid ATK value={} for card_type={card_type:?}",
            row.atk
        ));
        return None;
    };
    let Some(def) = normalize_def(row.defense, &card_type) else {
        rejection.warning(format_args!(
            "invalid DEF value={} for card_type={card_type:?}",
            row.defense
        ));
        return None;
    };
    let Some(level) = normalize_level(row.level, &card_type) else {
        rejection.warning(format_args!(
            "invalid packed level value={} for card_type={card_type:?}",
            row.level
        ));
        return None;
    };
    let Some(rank) = normalize_rank(row.level, &card_type) else {
        rejection.warning(format_args!(
            "invalid packed rank value={} for card_type={card_type:?}",
            row.level
        ));
        return None;
    };
    let Some(pendulum_scale) = normalize_pendulum_scale(row.level, &card_type) else {
        rejection.warning(format_args!(
            "invalid packed pendulum scale value={} for card_type={card_type:?}",
            row.level
        ));
        return None;
    };
    let Some(link_value) = normalize_link_value(row.level, &card_type) else {
        rejection.warning(format_args!(
            "invalid packed link value={} for card_type={card_type:?}",
            row.level
        ));
        return None;
    };
    let Some(link_markers) = normalize_link_markers(row.defense, &card_type) else {
        rejection.warning(format_args!(
            "invalid link marker bitmask={} ({:#x}) for card_type={card_type:?}",
            row.defense, row.defense
        ));
        return None;
    };

    let image = images.resolve(Environment::Ot, row.id, &name, row.alias)?;

    Some(Card {
        id: row.id,
        name,
        attribute,
        image,
        description,
        pendulum_description,
        alias: row.alias,
        r#type: card_type,
        restrictions: forbidden_lists.for_card(row.id, row.alias),
        atk,
        def,
        level,
        rank,
        pendulum_scale,
        link_value,
        link_markers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    #[test]
    fn serializes_general_properties() {
        let card = Card {
            id: 89631139,
            name: String::from("Blue-Eyes White Dragon"),
            attribute: 1,
            image: 89631139,
            description: String::from("A legendary dragon."),
            pendulum_description: None,
            alias: 0,
            r#type: labels(&["怪兽", "龙族", "通常"]),
            restrictions: vec![3, 1],
            atk: Some(3000),
            def: Some(2500),
            level: Some(8),
            rank: None,
            pendulum_scale: None,
            link_value: None,
            link_markers: None,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":89631139,"name":"Blue-Eyes White Dragon","attribute":1,"image":89631139,"description":"A legendary dragon.","alias":0,"type":["怪兽","龙族","通常"],"lf":[3,1],"atk":3000,"def":2500,"level":8}"#
        );
    }
}
