use crate::cards::masks::{has_label, mapped_label, mapped_value, ot_masks};

pub(super) fn normalize_attribute(raw_attribute: i64) -> Option<i64> {
    mapped_value(&ot_masks().attributes, raw_attribute)
}

pub(super) fn parse_card_type(raw_type: i64, raw_race: i64) -> Option<Vec<String>> {
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
}
