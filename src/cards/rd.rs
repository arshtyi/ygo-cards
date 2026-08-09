mod classification;
mod normalization;
mod restrictions;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use self::{
    classification::{is_legend, map_attribute, map_card_types},
    normalization::{
        monster_stat, normalize_description, normalize_maximum, normalize_maximum_atk,
    },
    restrictions::{ForbiddenList, read_forbidden_list},
};
use super::{
    CardCollection, DatasetReport, GenerationOptions, images::ImageResolver,
    mappings::ensure_rd_mappings, rejection, write_dataset,
};
use crate::environment::Environment;

const DATABASE_PATH: &str = "assets/rd/rd_standard.cdb";
const FORBIDDEN_LIST_PATH: &str = "assets/rd/lflist.conf";
const OUTPUT_PATH: &str = "output/rd.json";
const NON_CARD_HEADER_ROWS: i64 = 4;

#[derive(Debug, Serialize)]
struct Card {
    id: i64,
    name: String,
    attribute: i64,
    image: i64,
    description: String,
    legend: bool,
    r#type: Vec<String>,
    #[serde(rename = "lf")]
    restriction: i64,
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
struct DatabaseRow {
    id: i64,
    name: Option<String>,
    description: Option<String>,
    attribute_code: i64,
    type_flags: i64,
    race_code: i64,
    alias: i64,
    atk: i64,
    defense: i64,
    level: i64,
}

pub(crate) fn generate(options: GenerationOptions) -> Result<DatasetReport> {
    ensure_rd_mappings()?;
    let forbidden_list = read_forbidden_list(Path::new(FORBIDDEN_LIST_PATH))?;
    let mut images = ImageResolver::new(options)?;
    let collection = read_cards(Path::new(DATABASE_PATH), &forbidden_list, &mut images)?;
    let path = PathBuf::from(OUTPUT_PATH);

    write_dataset(&path, &collection.cards)?;

    Ok(DatasetReport {
        environment: Environment::Rd,
        path,
        cards_written: collection.cards.len(),
        cards_skipped: collection.skipped,
        image_summary: images.summary(),
        image_failures: images.failures().to_vec(),
    })
}

fn read_cards(
    db_path: &Path,
    forbidden_list: &ForbiddenList,
    images: &mut ImageResolver,
) -> Result<CardCollection<Card>> {
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
            [NON_CARD_HEADER_ROWS],
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
        .query_map([NON_CARD_HEADER_ROWS], |row| {
            Ok(DatabaseRow {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                attribute_code: row.get(3)?,
                type_flags: row.get(4)?,
                race_code: row.get(5)?,
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
                if let Some(card) = build_card(row, forbidden_list, images) {
                    cards.push(card);
                } else {
                    cards_skipped += 1;
                }
            }
            Err(error) => {
                cards_skipped += 1;
                rejection::database_row(Environment::Rd, row_index + 1, &error);
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
    forbidden_list: &ForbiddenList,
    images: &mut ImageResolver,
) -> Option<Card> {
    if row.id <= 0 {
        rejection::card(
            Environment::Rd,
            row.id,
            row.name.as_deref(),
            format_args!("ID must be positive"),
        );
        return None;
    }

    let Some(name) = row.name else {
        rejection::card(
            Environment::Rd,
            row.id,
            None,
            format_args!("name is missing from the texts table"),
        );
        return None;
    };
    let rejection = rejection::Card::new(Environment::Rd, row.id, &name);

    if name.trim().is_empty() {
        rejection.warning(format_args!("name is empty or whitespace-only"));
        return None;
    }

    let Some(raw_description) = row.description.as_deref() else {
        rejection.warning(format_args!("description is missing from the texts table"));
        return None;
    };

    let description = normalize_description(raw_description);

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

    let atk = monster_stat(row.atk, &card_type);
    let defense = monster_stat(row.defense, &card_type);
    let level = monster_stat(row.level, &card_type);
    let Some(maximum) = normalize_maximum(&name, &card_type, raw_description) else {
        rejection.warning(format_args!(
            "maximum position could not be normalized for card_type={card_type:?}"
        ));
        return None;
    };
    let Some(maximum_atk) = normalize_maximum_atk(maximum, raw_description) else {
        rejection.warning(format_args!(
            "maximum ATK could not be parsed; maximum_position={maximum:?}"
        ));
        return None;
    };
    let image = images.resolve(Environment::Rd, row.id, &name, row.alias)?;

    Some(Card {
        id: row.id,
        name,
        attribute,
        image,
        description,
        legend: is_legend(row.type_flags),
        r#type: card_type,
        restriction: forbidden_list.for_card(row.id, row.alias),
        alias: row.alias,
        atk,
        def: defense,
        level,
        maximum,
        maximum_atk,
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
            id: 120100001,
            name: String::from("大道魔法-爆发"),
            attribute: 0,
            image: 120100001,
            description: String::from("【条件】\n无"),
            legend: false,
            r#type: labels(&["魔法"]),
            restriction: 3,
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
    fn serializes_monster_stats() {
        let card = Card {
            id: 120105001,
            name: String::from("七星道魔术师"),
            attribute: 0,
            image: 120105001,
            description: String::from("【条件】\n无"),
            legend: false,
            r#type: labels(&["怪兽", "魔法师族", "效果"]),
            restriction: 3,
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
        let card = Card {
            id: 120150002,
            name: String::from("超魔机神 大霸道王"),
            attribute: 1,
            image: 120150002,
            description: String::from("可以和其他卡集齐作极大召唤。"),
            legend: false,
            r#type: labels(&["怪兽", "机械族", "极大", "效果"]),
            restriction: 3,
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
        let forbidden_list = ForbiddenList::default();
        let mut images = ImageResolver::new(GenerationOptions::default()).unwrap();

        assert!(
            build_card(
                DatabaseRow {
                    id: 0,
                    name: None,
                    description: None,
                    attribute_code: 0,
                    type_flags: 0,
                    race_code: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                DatabaseRow {
                    id: 120100001,
                    name: None,
                    description: None,
                    attribute_code: 0,
                    type_flags: 0,
                    race_code: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                DatabaseRow {
                    id: 120100001,
                    name: Some(String::from("  ")),
                    description: None,
                    attribute_code: 0,
                    type_flags: 0,
                    race_code: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                DatabaseRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: Some(String::from("RD/SJMP-JP001\r\n【条件】")),
                    attribute_code: 0x40,
                    type_flags: 0,
                    race_code: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                DatabaseRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: Some(String::from("RD/SJMP-JP001\r\n【条件】")),
                    attribute_code: 0,
                    type_flags: 0,
                    race_code: 0,
                    alias: -1,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
        assert!(
            build_card(
                DatabaseRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    description: None,
                    attribute_code: 0,
                    type_flags: 0x2,
                    race_code: 0,
                    alias: 0,
                    atk: 0,
                    defense: 0,
                    level: 0,
                },
                &forbidden_list,
                &mut images
            )
            .is_none()
        );
    }

    #[test]
    fn keeps_empty_descriptions() {
        let forbidden_list = ForbiddenList::default();
        let mut images = ImageResolver::new(GenerationOptions::default()).unwrap();
        let card = build_card(
            DatabaseRow {
                id: 120287001,
                name: Some(String::from("杰拉")),
                description: Some(String::from("RD/AP01-JP001")),
                attribute_code: 0x20,
                type_flags: 0x21,
                race_code: 0x8,
                alias: 0,
                atk: 2800,
                defense: 2300,
                level: 8,
            },
            &forbidden_list,
            &mut images,
        )
        .unwrap();

        assert_eq!(card.description, "");
    }
}
