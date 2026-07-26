mod limits;
mod normalization;
mod types;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use super::{
    BuildOptions, LfStatisticsOptions, LfSummary, WriteReport, images::ImageResolver,
    masks::ensure_ot_masks, rejection, write_cards,
};

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

pub fn write_json(options: BuildOptions) -> Result<WriteReport> {
    write_json_with_lf_statistics(options, LfStatisticsOptions::default())
}

pub fn write_json_with_lf_statistics(
    options: BuildOptions,
    lf_statistics_options: LfStatisticsOptions,
) -> Result<WriteReport> {
    ensure_ot_masks()?;
    let lf_lists = read_lf_lists(Path::new(LFLIST))?;
    let mut images = ImageResolver::new(options)?;
    let read_report = read_cards(Path::new(CARDS_DB), &lf_lists, &mut images)?;
    let path = PathBuf::from(OUTPUT_JSON);

    write_cards(&path, &read_report.cards)?;

    Ok(WriteReport {
        label: "OT",
        path,
        cards_written: read_report.cards.len(),
        cards_skipped: read_report.cards_skipped,
        lf_statistics_options,
        lf_summaries: summarize_lf(&read_report.cards, lf_statistics_options),
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
        .context("failed to prepare OT card query")?;
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
        .context("failed to query OT cards")?;

    let mut cards = Vec::new();
    let mut cards_skipped = 0;
    for (row_index, row) in rows.enumerate() {
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
                rejection::database_row("OT", row_index + 1, &error);
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
        rejection::card(
            "OT",
            row.id,
            row.name.as_deref(),
            format_args!("ID must be positive"),
        );
        return None;
    }

    let Some(name) = row.name else {
        rejection::card(
            "OT",
            row.id,
            None,
            format_args!("name is missing from the texts table"),
        );
        return None;
    };
    let rejection = rejection::Card::new("OT", row.id, &name);

    if name.trim().is_empty() {
        rejection.warning(format_args!("name is empty or whitespace-only"));
        return None;
    }

    let Some(raw_description) = row.description else {
        rejection.warning(format_args!("description is missing from the texts table"));
        return None;
    };

    let Some(attribute) = normalize_attribute(row.attribute) else {
        rejection.warning(format_args!(
            "unsupported attribute value={} ({:#x})",
            row.attribute, row.attribute
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

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        rejection.warning(format_args!(
            "unsupported type or race bitmask; type={} ({:#x}) race={} ({:#x})",
            row.card_type, row.card_type, row.race, row.race
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
    let Some(link_marker) = normalize_link_marker(row.defense, &card_type) else {
        rejection.warning(format_args!(
            "invalid link marker bitmask={} ({:#x}) for card_type={card_type:?}",
            row.defense, row.defense
        ));
        return None;
    };

    let image = images.resolve("OT", row.id, &name, row.alias)?;

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

fn summarize_lf(cards: &[OtCard], options: LfStatisticsOptions) -> Vec<LfSummary> {
    let mut ocg = [0; 4];
    let mut tcg = [0; 4];

    for card in cards
        .iter()
        .filter(|card| !options.ignore_aliases || card.alias == 0)
    {
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
    fn summarizes_limits_with_alias_filtering() {
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
        let cards = [
            card(0, vec![0, 1]),
            card(123, vec![0, 1]),
            card(0, vec![2, 3]),
        ];
        let summaries = summarize_lf(&cards, LfStatisticsOptions::default());

        assert_eq!(summaries[0].counts, [1, 0, 1, 0]);
        assert_eq!(summaries[1].counts, [0, 1, 0, 1]);

        let summaries = summarize_lf(
            &cards,
            LfStatisticsOptions {
                ignore_aliases: false,
            },
        );

        assert_eq!(summaries[0].counts, [2, 0, 1, 0]);
        assert_eq!(summaries[1].counts, [0, 2, 0, 1]);
    }
}
