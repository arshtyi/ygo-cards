use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::Serialize;

use super::{
    LfSummary, WriteReport,
    images::ImageResolver,
    masks::{has_label, mapped_label, mapped_value, rd_masks},
    text::{normalize_newlines, strip_effect_text_note_lines},
};
use crate::json::write_pretty_sorted;

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
}

pub fn write_json(options: BuildOptions) -> Result<WriteReport> {
    let lf_list = read_lf_list(Path::new(LFLIST))?;
    let mut images = ImageResolver::new(options.check_images)?;
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
    for row in rows {
        match row {
            Ok(row) => {
                if let Some(card) = build_card(row, lf_list, images)? {
                    cards.push(card);
                } else {
                    cards_skipped += 1;
                }
            }
            Err(error) => {
                cards_skipped += 1;
                eprintln!("skip RD card row: failed to read database row: {error}");
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

    let Some(raw_description) = row.description.as_deref() else {
        eprintln!("skip RD card {} ({}): missing description", row.id, name);
        return Ok(None);
    };

    let description = normalize_description(raw_description);

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!(
            "skip RD card {} ({}): invalid attribute {} ({:#x})",
            row.id, name, row.attribute, row.attribute
        );
        return Ok(None);
    };

    if row.alias < 0 {
        eprintln!(
            "skip RD card {} ({}): invalid alias {}",
            row.id, name, row.alias
        );
        return Ok(None);
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!(
            "skip RD card {} ({}): invalid type {} ({:#x}) or race {} ({:#x})",
            row.id, name, row.card_type, row.card_type, row.race, row.race
        );
        return Ok(None);
    };

    let image = images.resolve("RD", row.id, &name, row.alias)?;
    let atk = monster_value(row.atk, &card_type);
    let defense = monster_value(row.defense, &card_type);
    let level = monster_value(row.level, &card_type);
    let Some(maximum) = normalize_maximum(&name, &card_type, raw_description) else {
        eprintln!(
            "skip RD card {} ({}): invalid maximum position for type {:?}",
            row.id, name, card_type
        );
        return Ok(None);
    };
    let Some(maximum_atk) = normalize_maximum_atk(maximum, raw_description) else {
        eprintln!(
            "skip RD card {} ({}): invalid maximum atk for maximum {:?}",
            row.id, name, maximum
        );
        return Ok(None);
    };

    Ok(Some(RdCard {
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
    }))
}

fn monster_value(value: i64, card_type: &[String]) -> Option<i64> {
    has_label(card_type, "怪兽").then_some(value)
}

fn summarize_lf(cards: &[RdCard]) -> Vec<LfSummary> {
    let mut counts = [0; 4];

    for card in cards {
        if let Some(limit) = usize::try_from(card.lf).ok() {
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

fn normalize_maximum(
    name: &str,
    card_type: &[String],
    raw_description: &str,
) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") {
        return Some(None);
    }

    let mut matched_positions = rd_masks().maximum_name_markers.iter().filter_map(|entry| {
        entry
            .markers
            .iter()
            .any(|marker| name.contains(marker))
            .then_some(entry.value)
    });
    let first_position = matched_positions.next();
    if matched_positions.next().is_some() {
        return None;
    }

    if first_position.is_some() {
        return Some(first_position);
    }

    if has_label(card_type, "极大") && parse_maximum_atk(raw_description).is_some() {
        Some(Some(1))
    } else {
        Some(None)
    }
}

fn normalize_maximum_atk(maximum: Option<i64>, raw_description: &str) -> Option<Option<i64>> {
    if maximum != Some(1) {
        return Some(None);
    }

    parse_maximum_atk(raw_description).map(Some)
}

fn normalize_description(raw_description: &str) -> String {
    let description = normalize_newlines(raw_description);
    let body = if description.starts_with("RD/") {
        description.split_once('\n').map_or("", |(_, body)| body)
    } else {
        description.as_str()
    };
    let body = strip_effect_text_note_lines(body);
    let body = strip_special_adjustment_lines(&body);
    let body = strip_leading_description_noise(&body);

    if body.trim().is_empty() {
        String::new()
    } else {
        body.to_string()
    }
}

fn strip_special_adjustment_lines(description: &str) -> String {
    let mut removed = false;
    let mut lines = Vec::new();

    for line in description.lines() {
        let trimmed = line.trim();
        let is_special_adjustment = trimmed.starts_with("（特殊调整：")
            && (trimmed.ends_with('）') || trimmed.ends_with(')'));
        if is_special_adjustment {
            removed = true;
        } else {
            lines.push(line);
        }
    }

    if removed {
        lines.join("\n")
    } else {
        description.to_string()
    }
}

fn strip_leading_description_noise(mut description: &str) -> &str {
    loop {
        let (line, rest) = description.split_once('\n').unwrap_or((description, ""));
        if parse_maximum_attack_line(line).is_none() {
            return description;
        }

        description = rest;
    }
}

fn parse_maximum_atk(raw_description: &str) -> Option<i64> {
    normalize_newlines(raw_description)
        .lines()
        .find_map(parse_maximum_attack_line)
}

fn parse_maximum_attack_line(line: &str) -> Option<i64> {
    let mut parts = line.split_whitespace();
    match parts.next()? {
        "极大攻击" | "极大攻击力" => {}
        _ => return None,
    }

    let attack = parts.next()?.parse().ok()?;
    if attack >= 0 && parts.next().is_none() {
        Some(attack)
    } else {
        None
    }
}

fn normalize_attribute(raw_attribute: i64) -> Option<i64> {
    mapped_value(&rd_masks().attributes, raw_attribute)
}

fn is_legend(raw_type: i64) -> bool {
    raw_type & rd_masks().legend_type != 0
}

fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<String>> {
    if raw_type < 0 {
        return None;
    }

    if raw_type & !known_type_mask() != 0 {
        return None;
    }

    let (primary, primary_label) = primary_type(raw_type)?;
    let mut card_type = match primary {
        PrimaryType::Monster => vec![primary_label, normalize_race(raw_race)?],
        PrimaryType::Spell | PrimaryType::Trap => vec![primary_label],
    };
    card_type.extend(matched_subtype_flags(raw_type, primary));

    Some(card_type)
}

fn known_type_mask() -> i64 {
    rd_masks()
        .primary_types
        .iter()
        .chain(rd_masks().subtypes.iter())
        .fold(rd_masks().legend_type, |mask, flag| mask | flag.bit)
}

fn primary_type(raw_type: i64) -> Option<(PrimaryType, String)> {
    let mut primary = None;

    for flag in &rd_masks().primary_types {
        if raw_type & flag.bit == 0 {
            continue;
        }
        let card_type = primary_kind(&flag.label)?;
        if primary.replace((card_type, flag.label.clone())).is_some() {
            return None;
        }
    }

    primary
}

fn primary_kind(label: &str) -> Option<PrimaryType> {
    match label {
        "怪兽" => Some(PrimaryType::Monster),
        "魔法" => Some(PrimaryType::Spell),
        "陷阱" => Some(PrimaryType::Trap),
        _ => None,
    }
}

fn matched_subtype_flags(raw_type: i64, primary: PrimaryType) -> Vec<String> {
    rd_masks()
        .subtypes
        .iter()
        .filter_map(|flag| {
            let masks = rd_masks();
            if primary == PrimaryType::Monster
                && raw_type & masks.ritual_type != 0
                && flag.bit == masks.fusion_type
            {
                return None;
            }

            if raw_type & flag.bit != 0 {
                Some(flag.label.clone())
            } else {
                None
            }
        })
        .collect()
}

fn normalize_race(raw_race: i64) -> Option<String> {
    mapped_label(&rd_masks().races, raw_race)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryType {
    Monster,
    Spell,
    Trap,
}

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

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
    }

    fn some_labels(values: &[&str]) -> Option<Vec<String>> {
        Some(labels(values))
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
    fn normalizes_descriptions() {
        assert_eq!(
            normalize_description("RD/ECG1-JP008 <传说卡> 【水族】\r\n正文\r\n第二行"),
            String::from("正文\n第二行")
        );
        assert_eq!(
            normalize_description("RD/RLP2-JP001 【银河族】\r\n【条件】\r\n\r\n内容"),
            String::from("【条件】\n内容")
        );
        assert_eq!(
            normalize_description(
                "RD/MAX1-JP002\r\n极大攻击 3500\r\n可以和其他卡集齐作极大召唤。\r\n\r\n【条件】"
            ),
            String::from("可以和其他卡集齐作极大召唤。\n【条件】")
        );
        assert_eq!(
            normalize_description(
                "RD/TEST-JP001\r\n【条件】\r\n无\r\n（状态类效果可在基本分处查看）\r\n"
            ),
            String::from("【条件】\n无")
        );
        assert_eq!(
            normalize_description(
                "RD/TEST-JP004\r\n【效果】\r\n正文\r\n（限制类和状态类效果可在基本分处查看）"
            ),
            String::from("【效果】\n正文")
        );
        assert_eq!(
            normalize_description(
                "RD/TEST-JP002\r\n【条件】\r\n无\r\n（注：有bug，可以当2只上级盖放)"
            ),
            String::from("【条件】\n无")
        );
        assert_eq!(
            normalize_description("第一行\r\n第二行"),
            String::from("第一行\n第二行")
        );
        assert_eq!(
            normalize_description("RD/ST01-JP002\r\r\n\r\n【效果】"),
            String::from("【效果】")
        );
        assert_eq!(normalize_description("RD/ONLY"), String::new());
        assert_eq!(normalize_description("RD/EMPTY\r\n  "), String::new());
        assert_eq!(
            normalize_description(
                "RD/TEST-JP003\r\n【效果】\r\n正文\r\n（特殊调整：特殊召唤的怪兽不用给对方确认）"
            ),
            String::from("【效果】\n正文")
        );
        assert_eq!(
            normalize_description("正文（特殊调整：这是正文的一部分）"),
            String::from("正文（特殊调整：这是正文的一部分）")
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
        assert_eq!(parse_card_type(0x2, 0), some_labels(&["魔法"]));
        assert_eq!(parse_card_type(0x80002, 0), some_labels(&["魔法", "场地"]));
        assert_eq!(parse_card_type(0x40002, 0), some_labels(&["魔法", "装备"]));
        assert_eq!(parse_card_type(0x82, 0), some_labels(&["魔法", "仪式"]));
        assert_eq!(parse_card_type(0xc, 0), some_labels(&["陷阱"]));
        assert_eq!(
            parse_card_type(0x29, 0x2),
            some_labels(&["怪兽", "魔法师族", "效果"])
        );
        assert_eq!(
            parse_card_type(0x61, 0x2000),
            some_labels(&["怪兽", "龙族", "融合", "效果"])
        );
        assert_eq!(
            parse_card_type(0xc1, 0x1),
            some_labels(&["怪兽", "战士族", "仪式"])
        );
        assert_eq!(
            parse_card_type(0xe1, 0x20000000),
            some_labels(&["怪兽", "天界战士族", "仪式", "效果"])
        );
        assert_eq!(
            parse_card_type(0x8021, 0x40000000),
            some_labels(&["怪兽", "银河族", "极大", "效果"])
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
        assert_eq!(normalize_race(0x1), Some(String::from("战士族")));
        assert_eq!(normalize_race(0x2000), Some(String::from("龙族")));
        assert_eq!(normalize_race(0x40000000), Some(String::from("银河族")));
        assert_eq!(normalize_race(0x80000000), Some(String::from("电子人族")));
        assert_eq!(normalize_race(-0x80000000), Some(String::from("电子人族")));
        assert_eq!(normalize_race(0), None);
    }

    #[test]
    fn keeps_stats_for_monsters_only() {
        assert_eq!(monster_value(2100, &labels(&["怪兽", "效果"])), Some(2100));
        assert_eq!(monster_value(0, &labels(&["魔法"])), None);
        assert_eq!(monster_value(0, &labels(&["陷阱"])), None);
    }

    #[test]
    fn normalizes_maximum_positions() {
        assert_eq!(
            normalize_maximum("超魔机神 大霸道王［L］", &labels(&["怪兽", "极大"]), "desc"),
            Some(Some(0))
        );
        assert_eq!(
            normalize_maximum("超魔机神 大霸道王[M]", &labels(&["怪兽", "极大"]), "desc"),
            Some(Some(1))
        );
        assert_eq!(
            normalize_maximum("超魔机神 大霸道王［R］", &labels(&["怪兽", "极大"]), "desc"),
            Some(Some(2))
        );
        assert_eq!(
            normalize_maximum(
                "超魔机神 大霸道王",
                &labels(&["怪兽", "极大"]),
                "RD/MAX1-JP002\r\n极大攻击 3500\r\n正文"
            ),
            Some(Some(1))
        );
        assert_eq!(
            normalize_maximum(
                "外宇宙 安琪利瓦天愿",
                &labels(&["怪兽", "极大"]),
                "RD/ORP3-JP061\r\n手卡的这张卡的卡名变成「破界王帝 外宇宙界愿［L］」。"
            ),
            Some(None)
        );
        assert_eq!(
            normalize_maximum("普通魔法[L]", &labels(&["魔法"]), "RD/X\r\n极大攻击 3500"),
            Some(None)
        );
        assert_eq!(
            normalize_maximum("异常［L］［R］", &labels(&["怪兽", "极大"]), "desc"),
            None
        );
    }

    #[test]
    fn normalizes_maximum_atk() {
        assert_eq!(
            normalize_maximum_atk(Some(1), "RD/MAX1-JP002\r\n极大攻击 3500\r\n正文"),
            Some(Some(3500))
        );
        assert_eq!(
            normalize_maximum_atk(Some(1), "RD/MAX1-JP002\r\n极大攻击力 4000\r\n正文"),
            Some(Some(4000))
        );
        assert_eq!(
            normalize_maximum_atk(Some(0), "RD/MAX1-JP001\r\n极大攻击 3500"),
            Some(None)
        );
        assert_eq!(
            normalize_maximum_atk(Some(1), "RD/MAX1-JP002\r\n正文"),
            None
        );
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn keeps_empty_descriptions() {
        let lf_list = LfList {
            entries: HashMap::new(),
        };
        let mut images = ImageResolver::new(false).unwrap();
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
        .unwrap()
        .unwrap();

        assert_eq!(card.description, "");
    }
}
