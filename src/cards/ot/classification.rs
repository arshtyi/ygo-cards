use crate::cards::{
    CardKind, has_type,
    mappings::{map_attribute_value, map_race_name, ot_mappings},
};

pub(super) fn map_attribute(attribute_code: i64) -> Option<i64> {
    map_attribute_value(&ot_mappings().attribute_codes, attribute_code)
}

pub(super) fn map_card_types(type_flags: i64, race_code: i64) -> Option<Vec<String>> {
    if type_flags < 0 {
        return None;
    }

    if type_flags & !known_type_mask() != 0 {
        return None;
    }

    let (primary, primary_label) = primary_type(type_flags)?;
    let subtype_flags = matched_subtype_flags(type_flags);
    if primary == CardKind::Monster
        && subtype_flags.len() == 1
        && has_type(&subtype_flags, "衍生物")
    {
        return None;
    }

    let mut card_type = match primary {
        CardKind::Monster => {
            let mut card_type = vec![primary_label];
            if let Some(race) = map_race(race_code) {
                card_type.push(race);
            }
            card_type
        }
        CardKind::Spell | CardKind::Trap => vec![primary_label],
    };
    card_type.extend(subtype_flags);

    Some(card_type)
}

fn known_type_mask() -> i64 {
    ot_mappings()
        .primary_type_flags
        .iter()
        .chain(ot_mappings().subtype_flags.iter())
        .fold(0, |known_mask, flag| known_mask | flag.mask)
}

fn primary_type(type_flags: i64) -> Option<(CardKind, String)> {
    let mut primary = None;

    for flag in &ot_mappings().primary_type_flags {
        if type_flags & flag.mask == 0 {
            continue;
        }
        let card_type = CardKind::from_output_name(&flag.output_name)?;
        if primary
            .replace((card_type, flag.output_name.clone()))
            .is_some()
        {
            return None;
        }
    }

    let mappings = ot_mappings();
    if primary.is_none()
        && type_flags & mappings.inferred_monster_type_mask != 0
        && matched_subtype_flags(type_flags).len() > 1
    {
        primary = Some((CardKind::Monster, primary_label(CardKind::Monster)?));
    }

    primary
}

fn primary_label(primary: CardKind) -> Option<String> {
    ot_mappings()
        .primary_type_flags
        .iter()
        .find(|flag| CardKind::from_output_name(&flag.output_name) == Some(primary))
        .map(|flag| flag.output_name.clone())
}

fn matched_subtype_flags(type_flags: i64) -> Vec<String> {
    ot_mappings()
        .subtype_flags
        .iter()
        .filter_map(|flag| {
            if type_flags & flag.mask != 0 {
                Some(flag.output_name.clone())
            } else {
                None
            }
        })
        .collect()
}

fn map_race(race_code: i64) -> Option<String> {
    map_race_name(&ot_mappings().race_codes, race_code)
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
        assert_eq!(map_attribute(0x00), Some(0));
        assert_eq!(map_attribute(0x40), Some(0));
        assert_eq!(map_attribute(0x10), Some(1));
        assert_eq!(map_attribute(0x20), Some(2));
        assert_eq!(map_attribute(0x08), Some(3));
        assert_eq!(map_attribute(0x01), Some(4));
        assert_eq!(map_attribute(0x04), Some(5));
        assert_eq!(map_attribute(0x02), Some(6));
        assert_eq!(map_attribute(0x30), None);
    }

    #[test]
    fn parses_type_bits_after_primary_from_high_to_low() {
        assert_eq!(map_card_types(0x2, 0), some_labels(&["魔法"]));
        assert_eq!(map_card_types(0x10002, 0), some_labels(&["魔法", "速攻"]));
        assert_eq!(map_card_types(0x100004, 0), some_labels(&["陷阱", "反击"]));
        assert_eq!(
            map_card_types(0x2101, 0x20),
            some_labels(&["怪兽", "机械族", "同调", "陷阱怪兽"])
        );
        assert_eq!(
            map_card_types(0x4011, 0x8),
            some_labels(&["怪兽", "恶魔族", "衍生物", "通常"])
        );
    }

    #[test]
    fn rejects_invalid_type_bits() {
        assert_eq!(map_card_types(0, 0), None);
        assert_eq!(map_card_types(0x3, 0), None);
        assert_eq!(map_card_types(0x8000001, 0x1), None);
        assert_eq!(map_card_types(0x4000, 0x8), None);
        assert_eq!(
            map_card_types(0x4011, 0),
            some_labels(&["怪兽", "衍生物", "通常"])
        );
    }

    #[test]
    fn normalizes_monster_races() {
        assert_eq!(map_race(0x1), Some(String::from("战士族")));
        assert_eq!(map_race(0x2000), Some(String::from("龙族")));
        assert_eq!(map_race(0x2000000), Some(String::from("幻想魔族")));
        assert_eq!(map_race(0), None);
    }
}
