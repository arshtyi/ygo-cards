use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use reqwest::{
    blocking::{Client, Response},
    header::{CONTENT_TYPE, RANGE},
};
use rusqlite::Connection;
use serde::Serialize;

const CARDS_DB: &str = "assets/ot/cards.cdb";
const LFLIST: &str = "assets/ot/lflist.conf";
const OUTPUT_JSON: &str = "output/ot.json";
const IMAGE_BASE_URL: &str = "https://images.ygoprodeck.com/images/cards_cropped";

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
    r#type: Vec<&'static str>,
    lf: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    atk: Option<i64>,
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
    let lf_lists = read_lf_lists(Path::new(LFLIST))?;
    let mut images = ImageResolver::new(options.check_images)?;
    let cards = read_cards(Path::new(CARDS_DB), &lf_lists, &mut images)?;
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

fn read_cards(
    db_path: &Path,
    lf_lists: &LfLists,
    images: &mut ImageResolver,
) -> Result<Vec<OtCard>> {
    let connection = Connection::open(db_path)
        .with_context(|| format!("failed to open cards database {}", db_path.display()))?;
    let mut statement = connection
        .prepare(
            "
            select datas.id, texts.name, texts.desc, datas.attribute, datas.alias, datas.type, datas.race, datas.atk
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
            })
        })
        .context("failed to query cards")?;

    let mut cards = Vec::new();
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_lists, images)? {
                    cards.push(card);
                }
            }
            Err(error) => eprintln!("skip card: failed to read row: {error}"),
        }
    }

    Ok(cards)
}

fn build_card(
    row: CardRow,
    lf_lists: &LfLists,
    images: &mut ImageResolver,
) -> Result<Option<OtCard>> {
    if row.id <= 0 {
        eprintln!("skip card: invalid id {}", row.id);
        return Ok(None);
    }

    let Some(name) = row.name else {
        eprintln!("skip card {}: missing name", row.id);
        return Ok(None);
    };

    if name.trim().is_empty() {
        eprintln!("skip card {}: empty name", row.id);
        return Ok(None);
    }

    let Some(raw_description) = row.description else {
        eprintln!("skip card {}: missing description", row.id);
        return Ok(None);
    };

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!("skip card {}: invalid attribute {}", row.id, row.attribute);
        return Ok(None);
    };

    if row.alias < 0 {
        eprintln!("skip card {}: invalid alias {}", row.id, row.alias);
        return Ok(None);
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!("skip card {}: invalid type {}", row.id, row.card_type);
        return Ok(None);
    };

    let Some(description) = normalize_description(&raw_description, &card_type) else {
        eprintln!("skip card {}: invalid description", row.id);
        return Ok(None);
    };
    let Some(pendulum_description) = normalize_pendulum_description(&raw_description, &card_type)
    else {
        eprintln!("skip card {}: invalid pendulum description", row.id);
        return Ok(None);
    };
    let Some(atk) = normalize_atk(row.atk, &card_type) else {
        eprintln!("skip card {}: invalid atk {}", row.id, row.atk);
        return Ok(None);
    };

    let image = images.resolve(row.id, row.alias)?;

    Ok(Some(OtCard {
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
    }))
}

#[derive(Debug)]
struct ImageResolver {
    mode: ImageMode,
    cache: HashMap<i64, bool>,
}

impl ImageResolver {
    fn new(check_images: bool) -> Result<Self> {
        let mode = if check_images {
            ImageMode::Checking(
                Client::builder()
                    .user_agent(concat!(
                        env!("CARGO_PKG_NAME"),
                        "/",
                        env!("CARGO_PKG_VERSION")
                    ))
                    .timeout(Duration::from_secs(20))
                    .build()
                    .context("failed to build HTTP image client")?,
            )
        } else {
            ImageMode::UseCardId
        };

        Ok(Self {
            mode,
            cache: HashMap::new(),
        })
    }

    fn resolve(&mut self, id: i64, alias: i64) -> Result<i64> {
        if matches!(self.mode, ImageMode::UseCardId) {
            return Ok(id);
        }

        resolve_image(id, alias, |image_id| self.exists(image_id))
    }

    fn exists(&mut self, id: i64) -> Result<bool> {
        if let Some(exists) = self.cache.get(&id) {
            return Ok(*exists);
        }

        let exists = self.image_exists(id)?;
        self.cache.insert(id, exists);
        Ok(exists)
    }

    fn image_exists(&self, id: i64) -> Result<bool> {
        let ImageMode::Checking(client) = &self.mode else {
            return Ok(true);
        };

        let url = image_url(id);
        let response = client
            .head(&url)
            .send()
            .with_context(|| format!("failed to check image {}", url))?;

        if response.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return self.image_exists_with_get(&url);
        }

        Ok(is_image_response(&response))
    }

    fn image_exists_with_get(&self, url: &str) -> Result<bool> {
        let ImageMode::Checking(client) = &self.mode else {
            return Ok(true);
        };

        let response = client
            .get(url)
            .header(RANGE, "bytes=0-0")
            .send()
            .with_context(|| format!("failed to check image {}", url))?;

        Ok(is_image_response(&response))
    }
}

#[derive(Debug)]
enum ImageMode {
    UseCardId,
    Checking(Client),
}

fn image_url(id: i64) -> String {
    format!("{IMAGE_BASE_URL}/{id}.jpg")
}

fn resolve_image(
    mut id: i64,
    alias: i64,
    mut exists: impl FnMut(i64) -> Result<bool>,
) -> Result<i64> {
    if exists(id)? {
        return Ok(id);
    }

    if alias > 0 && exists(alias)? {
        id = alias;
    } else {
        id = 0;
    }

    Ok(id)
}

fn is_image_response(response: &Response) -> bool {
    response.status().is_success()
        && response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|content_type| content_type.starts_with("image/"))
}

fn normalize_description(description: &str, card_type: &[&str]) -> Option<String> {
    let description = description.replace("\r\n", "\n");

    if !card_type.contains(&"灵摆") {
        return Some(description);
    }

    description
        .split_once("【怪兽效果】")
        .or_else(|| description.split_once("【怪兽描述】"))
        .map(|(_, monster_description)| {
            monster_description
                .strip_prefix('\n')
                .unwrap_or(monster_description)
                .to_string()
        })
}

fn normalize_pendulum_description(description: &str, card_type: &[&str]) -> Option<Option<String>> {
    if !card_type.contains(&"灵摆") {
        return Some(None);
    }

    let description = description.replace("\r\n", "\n");
    let (_, rest) = description.split_once('\n')?;
    let marker_index = monster_description_marker_index(rest)?;
    let pendulum_description = rest[..marker_index]
        .strip_suffix('\n')
        .unwrap_or(&rest[..marker_index])
        .to_string();

    Some(Some(pendulum_description))
}

fn monster_description_marker_index(description: &str) -> Option<usize> {
    ["【怪兽效果】", "【怪兽描述】"]
        .iter()
        .filter_map(|marker| description.find(marker))
        .min()
}

fn normalize_atk(raw_atk: i64, card_type: &[&str]) -> Option<Option<i64>> {
    if !card_type.contains(&"怪兽") {
        return Some(None);
    }

    let atk = if raw_atk == -2 { -1 } else { raw_atk };
    if atk >= -1 { Some(Some(atk)) } else { None }
}

#[derive(Debug)]
struct LfLists {
    ocg: HashMap<i64, i64>,
    tcg: HashMap<i64, i64>,
}

impl LfLists {
    fn for_card(&self, id: i64, alias: i64) -> Vec<i64> {
        vec![
            self.limit_for(&self.ocg, id, alias),
            self.limit_for(&self.tcg, id, alias),
        ]
    }

    fn limit_for(&self, list: &HashMap<i64, i64>, id: i64, alias: i64) -> i64 {
        list.get(&id)
            .or_else(|| (alias > 0).then(|| list.get(&alias)).flatten())
            .copied()
            .unwrap_or(3)
    }
}

fn read_lf_lists(path: &Path) -> Result<LfLists> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read forbidden list {}", path.display()))?;
    parse_lf_lists(&text)
}

fn parse_lf_lists(text: &str) -> Result<LfLists> {
    let mut ocg = None;
    let mut tcg = None;
    let mut current_region = None;
    let mut current_entries = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(name) = line.strip_prefix('!') {
            finish_lf_list(current_region, &mut current_entries, &mut ocg, &mut tcg);
            if ocg.is_some() && tcg.is_some() {
                break;
            }

            let region = if name.contains("TCG") {
                LfRegion::Tcg
            } else {
                LfRegion::Ocg
            };
            current_region = match region {
                LfRegion::Ocg if ocg.is_none() => Some(region),
                LfRegion::Tcg if tcg.is_none() => Some(region),
                _ => None,
            };
            continue;
        }

        if current_region.is_none() || line.starts_with('#') {
            continue;
        }

        if let Some((id, count)) = parse_lf_entry(line) {
            current_entries.insert(id, count);
        }
    }

    finish_lf_list(current_region, &mut current_entries, &mut ocg, &mut tcg);

    Ok(LfLists {
        ocg: ocg.context("missing latest OCG forbidden list")?,
        tcg: tcg.context("missing latest TCG forbidden list")?,
    })
}

fn finish_lf_list(
    region: Option<LfRegion>,
    entries: &mut HashMap<i64, i64>,
    ocg: &mut Option<HashMap<i64, i64>>,
    tcg: &mut Option<HashMap<i64, i64>>,
) {
    match region {
        Some(LfRegion::Ocg) if ocg.is_none() => {
            *ocg = Some(std::mem::take(entries));
        }
        Some(LfRegion::Tcg) if tcg.is_none() => {
            *tcg = Some(std::mem::take(entries));
        }
        _ => entries.clear(),
    }
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

#[derive(Debug, Clone, Copy)]
enum LfRegion {
    Ocg,
    Tcg,
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
            image: 89631139,
            description: String::from("A legendary dragon."),
            pendulum_description: None,
            alias: 0,
            r#type: vec!["怪兽", "龙族", "通常"],
            lf: vec![3, 1],
            atk: Some(3000),
        };
        let json = serde_json::to_string(&card).unwrap();

        assert_eq!(
            json,
            r#"{"id":89631139,"name":"Blue-Eyes White Dragon","attribute":1,"image":89631139,"description":"A legendary dragon.","alias":0,"type":["怪兽","龙族","通常"],"lf":[3,1],"atk":3000}"#
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

    #[test]
    fn parses_latest_ocg_and_tcg_lists() {
        let lists = parse_lf_lists(
            "
            #[2026.4][2026.5 TCG]
            !2026.4
            #forbidden
            11111111 0 --A
            #limit
            22222222 1 --B
            !2026.5 TCG
            #semi limit
            22222222 2 --B
            33333333 0 --C
            !2026.1
            11111111 3 --old
            ",
        )
        .unwrap();

        assert_eq!(lists.for_card(11111111, 0), vec![0, 3]);
        assert_eq!(lists.for_card(22222222, 0), vec![1, 2]);
        assert_eq!(lists.for_card(33333333, 0), vec![3, 0]);
        assert_eq!(lists.for_card(44444444, 0), vec![3, 3]);
        assert_eq!(lists.for_card(55555555, 11111111), vec![0, 3]);
    }

    #[test]
    fn parses_lf_entry_lines() {
        assert_eq!(
            parse_lf_entry("08903700 0 --儀式魔人リリーサー"),
            Some((8903700, 0))
        );
        assert_eq!(parse_lf_entry("5318639 1 --旋风"), Some((5318639, 1)));
        assert_eq!(parse_lf_entry("#limit"), None);
        assert_eq!(parse_lf_entry("12345678 4 --invalid"), None);
    }

    #[test]
    fn builds_image_urls() {
        assert_eq!(
            image_url(89631139),
            "https://images.ygoprodeck.com/images/cards_cropped/89631139.jpg"
        );
    }

    #[test]
    fn resolves_image_id_with_alias_fallback() {
        assert_eq!(resolve_image(100, 200, |id| Ok(id == 100)).unwrap(), 100);
        assert_eq!(resolve_image(100, 200, |id| Ok(id == 200)).unwrap(), 200);
        assert_eq!(resolve_image(100, 0, |_| Ok(false)).unwrap(), 0);
        assert_eq!(resolve_image(100, 200, |_| Ok(false)).unwrap(), 0);
    }

    #[test]
    fn normalizes_descriptions() {
        assert_eq!(
            normalize_description("line 1\r\nline 2", &["魔法"]).unwrap(),
            "line 1\nline 2"
        );
        assert_eq!(
            normalize_description("【灵摆效果】P\r\n【怪兽效果】\r\nM\r\nE", &["怪兽", "灵摆"])
                .unwrap(),
            "M\nE"
        );
        assert_eq!(
            normalize_description("【灵摆效果】P\r\n【怪兽描述】\r\nM\r\nE", &["怪兽", "灵摆"])
                .unwrap(),
            "M\nE"
        );
        assert_eq!(
            normalize_description("missing marker", &["怪兽", "灵摆"]),
            None
        );
    }

    #[test]
    fn normalizes_pendulum_descriptions() {
        assert_eq!(
            normalize_pendulum_description("plain", &["魔法"]).unwrap(),
            None
        );
        assert_eq!(
            normalize_pendulum_description(
                "首行\r\nP1\r\nP2\r\n【怪兽效果】\r\nM",
                &["怪兽", "灵摆"]
            )
            .unwrap(),
            Some(String::from("P1\nP2"))
        );
        assert_eq!(
            normalize_pendulum_description("首行\r\nP1\r\n【怪兽描述】\r\nM", &["怪兽", "灵摆"])
                .unwrap(),
            Some(String::from("P1"))
        );
        assert_eq!(
            normalize_pendulum_description("首行\r\nmissing marker", &["怪兽", "灵摆"]),
            None
        );
    }

    #[test]
    fn normalizes_monster_atk() {
        assert_eq!(normalize_atk(3000, &["怪兽"]), Some(Some(3000)));
        assert_eq!(normalize_atk(-2, &["怪兽"]), Some(Some(-1)));
        assert_eq!(normalize_atk(-1, &["怪兽"]), Some(Some(-1)));
        assert_eq!(normalize_atk(-3, &["怪兽"]), None);
        assert_eq!(normalize_atk(0, &["魔法"]), Some(None));
    }
}
