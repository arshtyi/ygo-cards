use crate::cards::masks::{mapped_label, mapped_value, rd_masks};

pub(super) fn normalize_attribute(raw_attribute: i64) -> Option<i64> {
    mapped_value(&rd_masks().attributes, raw_attribute)
}

pub(super) fn is_legend(raw_type: i64) -> bool {
    raw_type & rd_masks().legend_type != 0
}

pub(super) fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<String>> {
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
}
