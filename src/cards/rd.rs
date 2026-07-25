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

use self::{
    limits::{LfList, read_lf_list},
    normalization::{
        monster_value, normalize_description, normalize_maximum, normalize_maximum_atk,
    },
    types::{is_legend, normalize_attribute, parse_card_type},
};
use super::{LfSummary, WriteReport, images::ImageResolver, masks::ensure_rd_masks};
use crate::{diagnostics, json::write_pretty_sorted};

const CARDS_DB: &str = "assets/rd/rd_standard.cdb";
const LFLIST: &str = "assets/rd/lflist.conf";
const OUTPUT_JSON: &str = "output/rd.json";
const USELESS_HEADER_ROWS: i64 = 4;

#[derive(Debug, Serialize)]
struct RdCard {
    id: i64,
    name: String,
    attribute: i64,
    image: i64,
    description: String,
    legend: bool,
    r#type: Vec<String>,
    lf: i64,
    alias: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    atk: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    def: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maximum: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "maximumAtk")]
    maximum_atk: Option<i64>,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    description: Option<String>,
    attribute: i64,
    card_type: i64,
    race: i64,
    alias: i64,
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
    ensure_rd_masks()?;
    let lf_list = read_lf_list(Path::new(LFLIST))?;
    let mut images =
        ImageResolver::new(options.check_images, options.skip_image_failures)?;
    let read_report = read_cards(Path::new(CARDS_DB), &lf_list, &mut images)?;
    let path = PathBuf::from(OUTPUT_JSON);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }

    write_pretty_sorted(&path, &read_report.cards)?;

    Ok(WriteReport {
        label: "RD",
        path,
        cards_written: read_report.cards.len(),
        cards_skipped: read_report.cards_skipped,
        lf_summaries: summarize_lf(&read_report.cards),
        image_summary: images.summary(),
        image_failures: images.failures().to_vec(),
    })
}

struct ReadCardsReport {
    cards: Vec<RdCard>,
    cards_skipped: usize,
}

fn read_cards(
    db_path: &Path,
    lf_list: &LfList,
    images: &mut ImageResolver,
) -> Result<ReadCardsReport> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open RD cards database {}", db_path.display()))?;
    let total_rows = connection
        .query_row(
            "
            select count(*)
            from datas
            where datas.id not in (
                select id from datas order by id limit ?1
            )
            and datas.id not in (
                select id from texts order by id limit ?1
            )
            ",
            [USELESS_HEADER_ROWS],
            |row| row.get::<_, i64>(0),
        )
        .context("failed to count RD card rows")? as usize;
    images.start_progress(total_rows, "checking RD images");

    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, texts.desc, datas.attribute, datas.type, datas.race, datas.alias, datas.atk, datas.def, datas.level
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
                description: row.get(2)?,
                attribute: row.get(3)?,
                card_type: row.get(4)?,
                race: row.get(5)?,
                alias: row.get(6)?,
                atk: row.get(7)?,
                defense: row.get(8)?,
                level: row.get(9)?,
            })
        })
        .context("failed to query RD cards")?;

    let mut cards = Vec::new();
    let mut cards_skipped = 0;
    for (row_index, row) in rows.enumerate() {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_list, images) {
                    cards.push(card);
                } else {
                    cards_skipped += 1;
                }
            }
            Err(error) => {
                cards_skipped += 1;
                diagnostics::warning(format_args!(
                    "skip RD database row: row_number={} reason=failed to decode SQLite row: {error}",
                    row_index + 1
                ));
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
    lf_list: &LfList,
    images: &mut ImageResolver,
) -> Option<RdCard> {
    if row.id <= 0 {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={:?} reason=ID must be positive",
            row.id,
            row.name.as_deref()
        ));
        return None;
    }

    let Some(name) = row.name else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} reason=name is missing from the texts table",
            row.id
        ));
        return None;
    };

    if name.trim().is_empty() {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=name is empty or whitespace-only",
            row.id
        ));
        return None;
    }

    let Some(raw_description) = row.description.as_deref() else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=description is missing from the texts table",
            row.id
        ));
        return None;
    };

    let description = normalize_description(raw_description);

    let Some(attribute) = normalize_attribute(row.attribute) else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=unsupported attribute value={} ({:#x})",
            row.id, row.attribute, row.attribute
        ));
        return None;
    };

    if row.alias < 0 {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=alias must be non-negative; alias={}",
            row.id, row.alias
        ));
        return None;
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=unsupported type or race bitmask; type={} ({:#x}) race={} ({:#x})",
            row.id, row.card_type, row.card_type, row.race, row.race
        ));
        return None;
    };

    let atk = monster_value(row.atk, &card_type);
    let defense = monster_value(row.defense, &card_type);
    let level = monster_value(row.level, &card_type);
    let Some(maximum) = normalize_maximum(&name, &card_type, raw_description) else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=maximum position could not be normalized for card_type={card_type:?}",
            row.id
        ));
        return None;
    };
    let Some(maximum_atk) = normalize_maximum_atk(maximum, raw_description) else {
        diagnostics::warning(format_args!(
            "skip RD card: id={} name={name:?} reason=maximum ATK could not be parsed; maximum_position={maximum:?}",
            row.id
        ));
        return None;
    };
    let image = images.resolve("RD", row.id, &name, row.alias)?;

    Some(RdCard {
        id: row.id,
        name,
        attribute,
        image,
        description,
        legend: is_legend(row.card_type),
        r#type: card_type,
        lf: lf_list.for_card(row.id, row.alias),
        alias: row.alias,
        atk,
        def: defense,
        level,
        maximum,
        maximum_atk,
    })
}

fn summarize_lf(cards: &[RdCard]) -> Vec<LfSummary> {
    let mut counts = [0; 4];

    for card in cards.iter().filter(|card| card.alias == 0) {
        if let Ok(limit) = usize::try_from(card.lf) {
            if let Some(count) = counts.get_mut(limit) {
                *count += 1;
            }
        }
    }

    vec![LfSummary {
        label: "RD",
        counts,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    #[test]
    fn serializes_general_properties() {
        let card = RdCard {
            id: 120100001,
            name: String::from("大道魔法-爆发"),
            attribute: 0,
            image: 120100001,
            description: String::from("【条件】\n无"),
            legend: false,
            r#type: labels(&["魔法"]),
            lf: 3,
            alias: 0,
            atk: None,
            def: None,
            level: None,
            maximum: None,
            maximum_atk: None,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120100001,"name":"大道魔法-爆发","attribute":0,"image":120100001,"description":"【条件】\n无","legend":false,"type":["魔法"],"lf":3,"alias":0}"#
        );
    }

    #[test]
    fn summarizes_limits_for_original_cards_only() {
        let card = |alias, lf| RdCard {
            id: 1,
            name: String::new(),
            attribute: 0,
            image: 1,
            description: String::new(),
            legend: false,
            r#type: Vec::new(),
            lf,
            alias,
            atk: None,
            def: None,
            level: None,
            maximum: None,
            maximum_atk: None,
        };
        let summaries = summarize_lf(&[card(0, 0), card(123, 0), card(0, 2)]);

        assert_eq!(summaries[0].counts, [1, 0, 1, 0]);
    }

    #[test]
    fn serializes_monster_stats() {
        let card = RdCard {
            id: 120105001,
            name: String::from("七星道魔术师"),
            attribute: 0,
            image: 120105001,
            description: String::from("【条件】\n无"),
            legend: false,
            r#type: labels(&["怪兽", "魔法师族", "效果"]),
            lf: 3,
            alias: 0,
            atk: Some(2100),
            def: Some(1500),
            level: Some(7),
            maximum: None,
            maximum_atk: None,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120105001,"name":"七星道魔术师","attribute":0,"image":120105001,"description":"【条件】\n无","legend":false,"type":["怪兽","魔法师族","效果"],"lf":3,"alias":0,"atk":2100,"def":1500,"level":7}"#
        );
    }

    #[test]
    fn serializes_maximum_monster_fields() {
        let card = RdCard {
            id: 120150002,
            name: String::from("超魔机神 大霸道王"),
            attribute: 1,
            image: 120150002,
            description: String::from("可以和其他卡集齐作极大召唤。"),
            legend: false,
            r#type: labels(&["怪兽", "机械族", "极大", "效果"]),
            lf: 3,
            alias: 0,
            atk: Some(1900),
            def: Some(0),
            level: Some(10),
            maximum: Some(1),
            maximum_atk: Some(3500),
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120150002,"name":"超魔机神 大霸道王","attribute":1,"image":120150002,"description":"可以和其他卡集齐作极大召唤。","legend":false,"type":["怪兽","机械族","极大","效果"],"lf":3,"alias":0,"atk":1900,"def":0,"level":10,"maximum":1,"maximumAtk":3500}"#
        );
    }

    #[test]
    fn rejects_invalid_rows() {
        let lf_list = LfList::default();
        let mut images = ImageResolver::new(false, false).unwrap();

        assert!(
            build_card(
                CardRow {
                    id: 0,
                    name: None,
                    description: None,
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: None,
                    description: None,
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("  ")),
                    description: None,
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: Some(String::from("RD/SJMP-JP001\r\n【条件】")),
                    attribute: 0x40,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: Some(String::from("RD/SJMP-JP001\r\n【条件】")),
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: -1,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: None,
                    attribute: 0,
                    card_type: 0x2,
                    race: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &lf_list,
                &mut images
            )
            .is_none()
        );
    }

    #[test]
    fn keeps_empty_descriptions() {
        let lf_list = LfList::default();
        let mut images = ImageResolver::new(false, false).unwrap();
        let card = build_card(
            CardRow {
                id: 120287001,
                name: Some(String::from("杰拉")),
                description: Some(String::from("RD/AP01-JP001")),
                attribute: 0x20,
                card_type: 0x21,
                race: 0x8,
                alias: 0,
                atk: 2800,
                defense: 2300,
                level: 8,
            },
            &lf_list,
            &mut images,
        )
        .unwrap();

        assert_eq!(card.description, "");
    }
}
