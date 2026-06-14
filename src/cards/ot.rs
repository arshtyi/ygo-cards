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
    r#type: Vec<&'static str>,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    attribute: i64,
    alias: i64,
    card_type: i64,
    race: i64,
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
            select datas.id, texts.name, datas.attribute, datas.alias, datas.type, datas.race
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
                card_type: row.get(4)?,
                race: row.get(5)?,
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

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!("skip card {}: invalid type {}", row.id, row.card_type);
        return None;
    };

    Some(OtCard {
        id: row.id,
        name,
        attribute,
        alias: row.alias,
        r#type: card_type,
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

fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<&'static str>> {
    if raw_type < 0 {
        return None;
    }

    if raw_type & !known_type_mask() != 0 {
        return None;
    }

    let primary = primary_type(raw_type)?;
    let subtype_flags = matched_subtype_flags(raw_type);
    if primary == PrimaryType::Monster && subtype_flags == ["衍生物"] {
        return None;
    }

    let mut card_type = match primary {
        PrimaryType::Monster => {
            let mut card_type = vec!["怪兽"];
            if let Some(race) = normalize_race(raw_race) {
                card_type.push(race);
            }
            card_type
        }
        PrimaryType::Spell => vec!["魔法"],
        PrimaryType::Trap => vec!["陷阱"],
    };
    card_type.extend(subtype_flags);

    Some(card_type)
}

fn known_type_mask() -> i64 {
    PRIMARY_TYPE_FLAGS
        .iter()
        .chain(SUBTYPE_FLAGS.iter())
        .fold(0, |mask, flag| mask | flag.bit)
}

#[derive(Debug)]
struct TypeFlag {
    bit: i64,
    label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryType {
    Monster,
    Spell,
    Trap,
}

fn primary_type(raw_type: i64) -> Option<PrimaryType> {
    let mut primary = None;

    for (bit, card_type) in [
        (0x1, PrimaryType::Monster),
        (0x2, PrimaryType::Spell),
        (0x4, PrimaryType::Trap),
    ] {
        if raw_type & bit == 0 {
            continue;
        }
        if primary.replace(card_type).is_some() {
            return None;
        }
    }

    if primary.is_none() && raw_type & 0x4000 != 0 && matched_subtype_flags(raw_type).len() > 1 {
        primary = Some(PrimaryType::Monster);
    }

    primary
}

fn matched_subtype_flags(raw_type: i64) -> Vec<&'static str> {
    SUBTYPE_FLAGS
        .iter()
        .filter_map(|flag| {
            if raw_type & flag.bit != 0 {
                Some(flag.label)
            } else {
                None
            }
        })
        .collect()
}

fn normalize_race(raw_race: i64) -> Option<&'static str> {
    match raw_race {
        0x1 => Some("战士族"),
        0x2 => Some("魔法师族"),
        0x4 => Some("天使族"),
        0x8 => Some("恶魔族"),
        0x10 => Some("不死族"),
        0x20 => Some("机械族"),
        0x40 => Some("水族"),
        0x80 => Some("炎族"),
        0x100 => Some("岩石族"),
        0x200 => Some("鸟兽族"),
        0x400 => Some("植物族"),
        0x800 => Some("昆虫族"),
        0x1000 => Some("雷族"),
        0x2000 => Some("龙族"),
        0x4000 => Some("兽族"),
        0x8000 => Some("兽战士族"),
        0x10000 => Some("恐龙族"),
        0x20000 => Some("鱼族"),
        0x40000 => Some("海龙族"),
        0x80000 => Some("爬虫类族"),
        0x100000 => Some("念动力族"),
        0x200000 => Some("幻神兽族"),
        0x400000 => Some("创造神族"),
        0x800000 => Some("幻龙族"),
        0x1000000 => Some("电子界族"),
        0x2000000 => Some("幻想魔族"),
        _ => None,
    }
}

const PRIMARY_TYPE_FLAGS: &[TypeFlag] = &[
    TypeFlag {
        bit: 0x1,
        label: "怪兽",
    },
    TypeFlag {
        bit: 0x2,
        label: "魔法",
    },
    TypeFlag {
        bit: 0x4,
        label: "陷阱",
    },
];

const SUBTYPE_FLAGS: &[TypeFlag] = &[
    TypeFlag {
        bit: 0x4000000,
        label: "连接",
    },
    TypeFlag {
        bit: 0x2000000,
        label: "特殊召唤",
    },
    TypeFlag {
        bit: 0x1000000,
        label: "灵摆",
    },
    TypeFlag {
        bit: 0x800000,
        label: "超量",
    },
    TypeFlag {
        bit: 0x400000,
        label: "卡通",
    },
    TypeFlag {
        bit: 0x200000,
        label: "反转",
    },
    TypeFlag {
        bit: 0x100000,
        label: "反击",
    },
    TypeFlag {
        bit: 0x80000,
        label: "场地",
    },
    TypeFlag {
        bit: 0x40000,
        label: "装备",
    },
    TypeFlag {
        bit: 0x20000,
        label: "永续",
    },
    TypeFlag {
        bit: 0x10000,
        label: "速攻",
    },
    TypeFlag {
        bit: 0x4000,
        label: "衍生物",
    },
    TypeFlag {
        bit: 0x2000,
        label: "同调",
    },
    TypeFlag {
        bit: 0x1000,
        label: "调整",
    },
    TypeFlag {
        bit: 0x800,
        label: "二重",
    },
    TypeFlag {
        bit: 0x400,
        label: "同盟",
    },
    TypeFlag {
        bit: 0x200,
        label: "灵魂",
    },
    TypeFlag {
        bit: 0x100,
        label: "陷阱怪兽",
    },
    TypeFlag {
        bit: 0x80,
        label: "仪式",
    },
    TypeFlag {
        bit: 0x40,
        label: "融合",
    },
    TypeFlag {
        bit: 0x20,
        label: "效果",
    },
    TypeFlag {
        bit: 0x10,
        label: "通常",
    },
];

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
            r#type: vec!["怪兽", "龙族", "通常"],
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":89631139,"name":"Blue-Eyes White Dragon","attribute":1,"alias":0,"type":["怪兽","龙族","通常"]}"#
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

    #[test]
    fn parses_type_bits_after_primary_from_high_to_low() {
        assert_eq!(parse_card_type(0x2, 0), Some(vec!["魔法"]));
        assert_eq!(parse_card_type(0x10002, 0), Some(vec!["魔法", "速攻"]));
        assert_eq!(parse_card_type(0x100004, 0), Some(vec!["陷阱", "反击"]));
        assert_eq!(
            parse_card_type(0x2101, 0x20),
            Some(vec!["怪兽", "机械族", "同调", "陷阱怪兽"])
        );
        assert_eq!(
            parse_card_type(0x4011, 0x8),
            Some(vec!["怪兽", "恶魔族", "衍生物", "通常"])
        );
    }

    #[test]
    fn rejects_invalid_type_bits() {
        assert_eq!(parse_card_type(0, 0), None);
        assert_eq!(parse_card_type(0x3, 0), None);
        assert_eq!(parse_card_type(0x8000001, 0x1), None);
        assert_eq!(parse_card_type(0x4000, 0x8), None);
        assert_eq!(
            parse_card_type(0x4011, 0),
            Some(vec!["怪兽", "衍生物", "通常"])
        );
    }

    #[test]
    fn normalizes_monster_races() {
        assert_eq!(normalize_race(0x1), Some("战士族"));
        assert_eq!(normalize_race(0x2000), Some("龙族"));
        assert_eq!(normalize_race(0x2000000), Some("幻想魔族"));
        assert_eq!(normalize_race(0), None);
    }
}
