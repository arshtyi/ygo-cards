use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use super::images::ImageResolver;

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
    legend: bool,
    r#type: Vec<&'static str>,
    lf: i64,
    alias: i64,
}

#[derive(Debug)]
struct CardRow {
    id: i64,
    name: Option<String>,
    attribute: i64,
    card_type: i64,
    race: i64,
    alias: i64,
}

#[derive(Debug)]
pub struct WriteReport {
    pub path: PathBuf,
    pub cards_written: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BuildOptions {
    pub check_images: bool,
}

pub fn write_json(options: BuildOptions) -> Result<WriteReport> {
    let lf_list = read_lf_list(Path::new(LFLIST))?;
    let mut images = ImageResolver::new(options.check_images)?;
    let cards = read_cards(Path::new(CARDS_DB), &lf_list, &mut images)?;
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

fn read_cards(db_path: &Path, lf_list: &LfList, images: &mut ImageResolver) -> Result<Vec<RdCard>> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open RD cards database {}", db_path.display()))?;
    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, datas.attribute, datas.type, datas.race, datas.alias
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
                race: row.get(4)?,
                alias: row.get(5)?,
            })
        })
        .context("failed to query RD cards")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_list, images)? {
                    cards.push(card);
                }
            }
            Err(error) => eprintln!("skip RD card: failed to read row: {error}"),
        }
    }

    Ok(cards)
}

fn build_card(
    row: CardRow,
    lf_list: &LfList,
    images: &mut ImageResolver,
) -> Result<Option<RdCard>> {
    if row.id <= 0 {
        eprintln!("skip RD card: invalid id {}", row.id);
        return Ok(None);
    }

    let Some(name) = row.name else {
        eprintln!("skip RD card {}: missing name", row.id);
        return Ok(None);
    };

    if name.trim().is_empty() {
        eprintln!("skip RD card {}: empty name", row.id);
        return Ok(None);
    }

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!(
            "skip RD card {}: invalid attribute {}",
            row.id, row.attribute
        );
        return Ok(None);
    };

    if row.alias < 0 {
        eprintln!("skip RD card {}: invalid alias {}", row.id, row.alias);
        return Ok(None);
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!(
            "skip RD card {}: invalid type {} or race {}",
            row.id, row.card_type, row.race
        );
        return Ok(None);
    };

    let image = images.resolve(row.id, row.alias)?;

    Ok(Some(RdCard {
        id: row.id,
        name,
        attribute,
        image,
        legend: is_legend(row.card_type),
        r#type: card_type,
        lf: lf_list.for_card(row.id, row.alias),
        alias: row.alias,
    }))
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

fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<&'static str>> {
    if raw_type < 0 {
        return None;
    }

    if raw_type & !known_type_mask() != 0 {
        return None;
    }

    let primary = primary_type(raw_type)?;
    let mut card_type = match primary {
        PrimaryType::Monster => vec!["怪兽", normalize_race(raw_race)?],
        PrimaryType::Spell => vec!["魔法"],
        PrimaryType::Trap => vec!["陷阱"],
    };
    card_type.extend(matched_subtype_flags(raw_type, primary));

    Some(card_type)
}

fn known_type_mask() -> i64 {
    PRIMARY_TYPE_FLAGS
        .iter()
        .chain(SUBTYPE_FLAGS.iter())
        .fold(LEGEND_TYPE_FLAG, |mask, flag| mask | flag.bit)
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

    primary
}

fn matched_subtype_flags(raw_type: i64, primary: PrimaryType) -> Vec<&'static str> {
    SUBTYPE_FLAGS
        .iter()
        .filter_map(|flag| {
            if primary == PrimaryType::Monster
                && raw_type & RITUAL_TYPE_FLAG != 0
                && flag.bit == FUSION_TYPE_FLAG
            {
                return None;
            }

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
        0x4000000 => Some("魔导骑士族"),
        0x8000000 => Some("多头龙族"),
        0x10000000 => Some("欧米茄念动力族"),
        0x20000000 => Some("天界战士族"),
        0x40000000 => Some("银河族"),
        0x80000000 | -0x80000000 => Some("电子人族"),
        _ => None,
    }
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

const LEGEND_TYPE_FLAG: i64 = 0x8;
const FUSION_TYPE_FLAG: i64 = 0x40;
const RITUAL_TYPE_FLAG: i64 = 0x80;

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
        bit: 0x80000,
        label: "场地",
    },
    TypeFlag {
        bit: 0x40000,
        label: "装备",
    },
    TypeFlag {
        bit: 0x8000,
        label: "极限",
    },
    TypeFlag {
        bit: RITUAL_TYPE_FLAG,
        label: "仪式",
    },
    TypeFlag {
        bit: FUSION_TYPE_FLAG,
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
            image: 120100001,
            legend: false,
            r#type: vec!["魔法"],
            lf: 3,
            alias: 0,
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":120100001,"name":"大道魔法-爆发","attribute":0,"image":120100001,"legend":false,"type":["魔法"],"lf":3,"alias":0}"#
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
    fn parses_type_bits_after_primary_from_high_to_low() {
        assert_eq!(parse_card_type(0x2, 0), Some(vec!["魔法"]));
        assert_eq!(parse_card_type(0x80002, 0), Some(vec!["魔法", "场地"]));
        assert_eq!(parse_card_type(0x40002, 0), Some(vec!["魔法", "装备"]));
        assert_eq!(parse_card_type(0x82, 0), Some(vec!["魔法", "仪式"]));
        assert_eq!(parse_card_type(0xc, 0), Some(vec!["陷阱"]));
        assert_eq!(
            parse_card_type(0x29, 0x2),
            Some(vec!["怪兽", "魔法师族", "效果"])
        );
        assert_eq!(
            parse_card_type(0x61, 0x2000),
            Some(vec!["怪兽", "龙族", "融合", "效果"])
        );
        assert_eq!(
            parse_card_type(0xc1, 0x1),
            Some(vec!["怪兽", "战士族", "仪式"])
        );
        assert_eq!(
            parse_card_type(0xe1, 0x20000000),
            Some(vec!["怪兽", "天界战士族", "仪式", "效果"])
        );
        assert_eq!(
            parse_card_type(0x8021, 0x40000000),
            Some(vec!["怪兽", "银河族", "极限", "效果"])
        );
    }

    #[test]
    fn rejects_invalid_type_bits() {
        assert_eq!(parse_card_type(0, 0), None);
        assert_eq!(parse_card_type(0x3, 0), None);
        assert_eq!(parse_card_type(0x1000001, 0x1), None);
        assert_eq!(parse_card_type(0x1, 0), None);
    }

    #[test]
    fn normalizes_monster_races() {
        assert_eq!(normalize_race(0x1), Some("战士族"));
        assert_eq!(normalize_race(0x2000), Some("龙族"));
        assert_eq!(normalize_race(0x40000000), Some("银河族"));
        assert_eq!(normalize_race(0x80000000), Some("电子人族"));
        assert_eq!(normalize_race(-0x80000000), Some("电子人族"));
        assert_eq!(normalize_race(0), None);
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
        let mut images = ImageResolver::new(false).unwrap();

        assert!(
            build_card(
                CardRow {
                    id: 0,
                    name: None,
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                },
                &lf_list,
                &mut images
            )
            .unwrap()
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: None,
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                },
                &lf_list,
                &mut images
            )
            .unwrap()
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("  ")),
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                },
                &lf_list,
                &mut images
            )
            .unwrap()
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    attribute: 0x40,
                    card_type: 0,
                    race: 0,
                    alias: 0,
                },
                &lf_list,
                &mut images
            )
            .unwrap()
            .is_none()
        );
        assert!(
            build_card(
                CardRow {
                    id: 120100001,
                    name: Some(String::from("大道魔法-爆发")),
                    attribute: 0,
                    card_type: 0,
                    race: 0,
                    alias: -1,
                },
                &lf_list,
                &mut images
            )
            .unwrap()
            .is_none()
        );
    }
}
