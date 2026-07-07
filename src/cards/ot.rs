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
    masks::{has_label, mapped_label, mapped_value, ot_masks},
    text::{normalize_newlines, strip_effect_text_note_lines},
};
use crate::json::write_pretty_sorted;

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
}

pub fn write_json(options: BuildOptions) -> Result<WriteReport> {
    let lf_lists = read_lf_lists(Path::new(LFLIST))?;
    let mut images = ImageResolver::new(options.check_images)?;
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
                if let Some(card) = build_card(row, lf_lists, images)? {
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
        eprintln!("skip OT card {} ({}): missing description", row.id, name);
        return Ok(None);
    };

    let Some(attribute) = normalize_attribute(row.attribute) else {
        eprintln!(
            "skip OT card {} ({}): invalid attribute {} ({:#x})",
            row.id, name, row.attribute, row.attribute
        );
        return Ok(None);
    };

    if row.alias < 0 {
        eprintln!(
            "skip OT card {} ({}): invalid alias {}",
            row.id, name, row.alias
        );
        return Ok(None);
    }

    let Some(card_type) = parse_card_type(row.card_type, row.race) else {
        eprintln!(
            "skip OT card {} ({}): invalid type {} ({:#x}) or race {} ({:#x})",
            row.id, name, row.card_type, row.card_type, row.race, row.race
        );
        return Ok(None);
    };

    let Some(description) = normalize_description(&raw_description, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid description for type {:?}",
            row.id, name, card_type
        );
        return Ok(None);
    };
    let Some(pendulum_description) = normalize_pendulum_description(&raw_description, &card_type)
    else {
        eprintln!(
            "skip OT card {} ({}): invalid pendulum description",
            row.id, name
        );
        return Ok(None);
    };
    let Some(atk) = normalize_atk(row.atk, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid atk {}",
            row.id, name, row.atk
        );
        return Ok(None);
    };
    let Some(def) = normalize_def(row.defense, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid def {}",
            row.id, name, row.defense
        );
        return Ok(None);
    };
    let Some(level) = normalize_level(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid level {}",
            row.id, name, row.level
        );
        return Ok(None);
    };
    let Some(rank) = normalize_rank(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid rank {}",
            row.id, name, row.level
        );
        return Ok(None);
    };
    let Some(pendulum_scale) = normalize_pendulum_scale(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid pendulum scale {}",
            row.id, name, row.level
        );
        return Ok(None);
    };
    let Some(link_value) = normalize_link_value(row.level, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid link value {}",
            row.id, name, row.level
        );
        return Ok(None);
    };
    let Some(link_marker) = normalize_link_marker(row.defense, &card_type) else {
        eprintln!(
            "skip OT card {} ({}): invalid link marker {}",
            row.id, name, row.defense
        );
        return Ok(None);
    };

    let image = images.resolve("OT", row.id, &name, row.alias)?;

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
        def,
        level,
        rank,
        pendulum_scale,
        link_value,
        link_marker,
    }))
}

fn summarize_lf(cards: &[OtCard]) -> Vec<LfSummary> {
    let mut ocg = [0; 4];
    let mut tcg = [0; 4];

    for card in cards {
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

fn normalize_description(description: &str, card_type: &[String]) -> Option<String> {
    let description = normalize_newlines(description);

    if !has_label(card_type, "灵摆") {
        return Some(strip_effect_text_note_lines(&description));
    }

    description
        .split_once("【怪兽效果】")
        .or_else(|| description.split_once("【怪兽描述】"))
        .map(|(_, monster_description)| {
            let monster_description = monster_description
                .strip_prefix('\n')
                .unwrap_or(monster_description);
            strip_effect_text_note_lines(monster_description)
        })
}

fn normalize_pendulum_description(
    description: &str,
    card_type: &[String],
) -> Option<Option<String>> {
    if !has_label(card_type, "灵摆") {
        return Some(None);
    }

    let description = normalize_newlines(description);
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

fn normalize_atk(raw_atk: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") {
        return Some(None);
    }

    let atk = if raw_atk == -2 { -1 } else { raw_atk };
    if atk >= -1 { Some(Some(atk)) } else { None }
}

fn normalize_def(raw_def: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") || has_label(card_type, "连接") {
        return Some(None);
    }

    let defense = if raw_def == -2 { -1 } else { raw_def };
    if defense >= -1 {
        Some(Some(defense))
    } else {
        None
    }
}

fn normalize_level(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") || has_label(card_type, "超量") || has_label(card_type, "连接")
    {
        return Some(None);
    }

    let level = low_level_byte(raw_level)?;
    if (0..=13).contains(&level) {
        Some(Some(level))
    } else {
        None
    }
}

fn normalize_rank(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") || !has_label(card_type, "超量") {
        return Some(None);
    }

    let rank = low_level_byte(raw_level)?;
    if (0..=13).contains(&rank) {
        Some(Some(rank))
    } else {
        None
    }
}

fn normalize_pendulum_scale(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") || !has_label(card_type, "灵摆") {
        return Some(None);
    }

    if raw_level < 0 {
        return None;
    }

    let rscale = (raw_level >> 16) & 0xff;
    let lscale = (raw_level >> 24) & 0xff;
    if rscale != lscale || !(0..=13).contains(&rscale) {
        return None;
    }

    Some(Some(rscale))
}

fn normalize_link_value(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") || !has_label(card_type, "连接") {
        return Some(None);
    }

    let link_value = low_level_byte(raw_level)?;
    if (1..=8).contains(&link_value) {
        Some(Some(link_value))
    } else {
        None
    }
}

fn normalize_link_marker(raw_def: i64, card_type: &[String]) -> Option<Option<Vec<i64>>> {
    if !has_label(card_type, "怪兽") || !has_label(card_type, "连接") {
        return Some(None);
    }

    if raw_def < 0 || raw_def & !known_link_marker_mask() != 0 {
        return None;
    }

    let markers = ot_masks()
        .link_markers
        .iter()
        .filter_map(|entry| {
            if raw_def & entry.bit != 0 {
                Some(entry.value)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if markers.is_empty() {
        None
    } else {
        Some(Some(markers))
    }
}

fn low_level_byte(raw_level: i64) -> Option<i64> {
    if raw_level >= 0 {
        Some(raw_level & 0xff)
    } else {
        None
    }
}

fn known_link_marker_mask() -> i64 {
    ot_masks()
        .link_markers
        .iter()
        .fold(0, |mask, entry| mask | entry.bit)
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
    mapped_value(&ot_masks().attributes, raw_attribute)
}

fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<String>> {
    if raw_type < 0 {
        return None;
    }

    if raw_type & !known_type_mask() != 0 {
        return None;
    }

    let (primary, primary_label) = primary_type(raw_type)?;
    let subtype_flags = matched_subtype_flags(raw_type);
    if primary == PrimaryType::Monster
        && subtype_flags.len() == 1
        && has_label(&subtype_flags, "衍生物")
    {
        return None;
    }

    let mut card_type = match primary {
        PrimaryType::Monster => {
            let mut card_type = vec![primary_label];
            if let Some(race) = normalize_race(raw_race) {
                card_type.push(race);
            }
            card_type
        }
        PrimaryType::Spell | PrimaryType::Trap => vec![primary_label],
    };
    card_type.extend(subtype_flags);

    Some(card_type)
}

fn known_type_mask() -> i64 {
    ot_masks()
        .primary_types
        .iter()
        .chain(ot_masks().subtypes.iter())
        .fold(0, |mask, flag| mask | flag.bit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimaryType {
    Monster,
    Spell,
    Trap,
}

fn primary_type(raw_type: i64) -> Option<(PrimaryType, String)> {
    let mut primary = None;

    for flag in &ot_masks().primary_types {
        if raw_type & flag.bit == 0 {
            continue;
        }
        let card_type = primary_kind(&flag.label)?;
        if primary.replace((card_type, flag.label.clone())).is_some() {
            return None;
        }
    }

    let masks = ot_masks();
    if primary.is_none()
        && raw_type & masks.inferred_monster_type_bit != 0
        && matched_subtype_flags(raw_type).len() > 1
    {
        primary = Some((PrimaryType::Monster, primary_label(PrimaryType::Monster)?));
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

fn primary_label(primary: PrimaryType) -> Option<String> {
    ot_masks()
        .primary_types
        .iter()
        .find(|flag| primary_kind(&flag.label) == Some(primary))
        .map(|flag| flag.label.clone())
}

fn matched_subtype_flags(raw_type: i64) -> Vec<String> {
    ot_masks()
        .subtypes
        .iter()
        .filter_map(|flag| {
            if raw_type & flag.bit != 0 {
                Some(flag.label.clone())
            } else {
                None
            }
        })
        .collect()
}

fn normalize_race(raw_race: i64) -> Option<String> {
    mapped_label(&ot_masks().races, raw_race)
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
        assert_eq!(parse_card_type(0x2, 0), some_labels(&["魔法"]));
        assert_eq!(parse_card_type(0x10002, 0), some_labels(&["魔法", "速攻"]));
        assert_eq!(parse_card_type(0x100004, 0), some_labels(&["陷阱", "反击"]));
        assert_eq!(
            parse_card_type(0x2101, 0x20),
            some_labels(&["怪兽", "机械族", "同调", "陷阱怪兽"])
        );
        assert_eq!(
            parse_card_type(0x4011, 0x8),
            some_labels(&["怪兽", "恶魔族", "衍生物", "通常"])
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
            some_labels(&["怪兽", "衍生物", "通常"])
        );
    }

    #[test]
    fn normalizes_monster_races() {
        assert_eq!(normalize_race(0x1), Some(String::from("战士族")));
        assert_eq!(normalize_race(0x2000), Some(String::from("龙族")));
        assert_eq!(normalize_race(0x2000000), Some(String::from("幻想魔族")));
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
    fn normalizes_descriptions() {
        assert_eq!(
            normalize_description("line 1\r\n\r\nline 2", &labels(&["魔法"])).unwrap(),
            "line 1\nline 2"
        );
        assert_eq!(
            normalize_description(
                "（注：暂时无法正常使用）\r\n这个卡名的卡在1回合只能发动1张。",
                &labels(&["魔法"])
            )
            .unwrap(),
            "这个卡名的卡在1回合只能发动1张。"
        );
        assert_eq!(
            normalize_description(
                "【灵摆效果】P\r\n【怪兽效果】\r\nM\r\n\r\nE",
                &labels(&["怪兽", "灵摆"])
            )
            .unwrap(),
            "M\nE"
        );
        assert_eq!(
            normalize_description(
                "【灵摆效果】P\r\n【怪兽描述】\r\nM\r\nE",
                &labels(&["怪兽", "灵摆"])
            )
            .unwrap(),
            "M\nE"
        );
        assert_eq!(
            normalize_description("missing marker", &labels(&["怪兽", "灵摆"])),
            None
        );
    }

    #[test]
    fn normalizes_pendulum_descriptions() {
        assert_eq!(
            normalize_pendulum_description("plain", &labels(&["魔法"])).unwrap(),
            None
        );
        assert_eq!(
            normalize_pendulum_description(
                "首行\r\nP1\r\n\r\nP2\r\n【怪兽效果】\r\nM",
                &labels(&["怪兽", "灵摆"])
            )
            .unwrap(),
            Some(String::from("P1\nP2"))
        );
        assert_eq!(
            normalize_pendulum_description(
                "首行\r\nP1\r\n【怪兽描述】\r\nM",
                &labels(&["怪兽", "灵摆"])
            )
            .unwrap(),
            Some(String::from("P1"))
        );
        assert_eq!(
            normalize_pendulum_description("首行\r\nmissing marker", &labels(&["怪兽", "灵摆"])),
            None
        );
    }

    #[test]
    fn normalizes_monster_atk() {
        assert_eq!(normalize_atk(3000, &labels(&["怪兽"])), Some(Some(3000)));
        assert_eq!(normalize_atk(-2, &labels(&["怪兽"])), Some(Some(-1)));
        assert_eq!(normalize_atk(-1, &labels(&["怪兽"])), Some(Some(-1)));
        assert_eq!(normalize_atk(-3, &labels(&["怪兽"])), None);
        assert_eq!(normalize_atk(0, &labels(&["魔法"])), Some(None));
    }

    #[test]
    fn normalizes_monster_def_and_link_value() {
        assert_eq!(normalize_def(2500, &labels(&["怪兽"])), Some(Some(2500)));
        assert_eq!(normalize_def(-2, &labels(&["怪兽"])), Some(Some(-1)));
        assert_eq!(normalize_def(-1, &labels(&["怪兽"])), Some(Some(-1)));
        assert_eq!(normalize_def(-3, &labels(&["怪兽"])), None);
        assert_eq!(normalize_def(0, &labels(&["魔法"])), Some(None));
        assert_eq!(normalize_def(-2, &labels(&["怪兽", "连接"])), Some(None));

        assert_eq!(
            normalize_link_value(0x04000004, &labels(&["怪兽", "连接"])),
            Some(Some(4))
        );
        assert_eq!(normalize_link_value(0, &labels(&["怪兽", "连接"])), None);
        assert_eq!(normalize_link_value(9, &labels(&["怪兽", "连接"])), None);
        assert_eq!(normalize_link_value(4, &labels(&["怪兽"])), Some(None));
        assert_eq!(normalize_link_value(4, &labels(&["魔法"])), Some(None));
    }

    #[test]
    fn normalizes_monster_level_rank_and_pendulum_scale() {
        assert_eq!(normalize_level(8, &labels(&["怪兽"])), Some(Some(8)));
        assert_eq!(
            normalize_level(0x0d000008, &labels(&["怪兽"])),
            Some(Some(8))
        );
        assert_eq!(normalize_level(14, &labels(&["怪兽"])), None);
        assert_eq!(normalize_level(-1, &labels(&["怪兽"])), None);
        assert_eq!(normalize_level(4, &labels(&["怪兽", "超量"])), Some(None));
        assert_eq!(normalize_level(4, &labels(&["怪兽", "连接"])), Some(None));
        assert_eq!(normalize_level(4, &labels(&["魔法"])), Some(None));

        assert_eq!(normalize_rank(4, &labels(&["怪兽", "超量"])), Some(Some(4)));
        assert_eq!(normalize_rank(14, &labels(&["怪兽", "超量"])), None);
        assert_eq!(normalize_rank(4, &labels(&["怪兽"])), Some(None));

        let pendulum_level = (8 << 24) | (8 << 16) | 4;
        assert_eq!(
            normalize_pendulum_scale(pendulum_level, &labels(&["怪兽", "灵摆"])),
            Some(Some(8))
        );
        assert_eq!(
            normalize_pendulum_scale((7 << 24) | (8 << 16) | 4, &labels(&["怪兽", "灵摆"])),
            None
        );
        assert_eq!(
            normalize_pendulum_scale((14 << 24) | (14 << 16) | 4, &labels(&["怪兽", "灵摆"])),
            None
        );
        assert_eq!(normalize_pendulum_scale(4, &labels(&["怪兽"])), Some(None));
    }

    #[test]
    fn normalizes_link_markers() {
        assert_eq!(
            normalize_link_marker(0xaa, &labels(&["怪兽", "连接"])),
            Some(Some(vec![1, 3, 5, 7]))
        );
        assert_eq!(
            normalize_link_marker(0x141, &labels(&["怪兽", "连接"])),
            Some(Some(vec![0, 2, 6]))
        );
        assert_eq!(normalize_link_marker(0, &labels(&["怪兽", "连接"])), None);
        assert_eq!(
            normalize_link_marker(0x200, &labels(&["怪兽", "连接"])),
            None
        );
        assert_eq!(normalize_link_marker(0xaa, &labels(&["怪兽"])), Some(None));
        assert_eq!(normalize_link_marker(0xaa, &labels(&["魔法"])), Some(None));
    }
}
