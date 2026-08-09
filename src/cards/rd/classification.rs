use crate::cards::{
    CardKind,
    mappings::{map_attribute_value, map_race_name, rd_mappings},
};

pub(super) fn map_attribute(attribute_code: i64) -> Option<i64> {
    map_attribute_value(&rd_mappings().attribute_codes, attribute_code)
}

pub(super) fn is_legend(type_flags: i64) -> bool {
    type_flags & rd_mappings().legend_type_mask != 0
}

pub(super) fn map_card_types(type_flags: i64, race_code: i64) -> Option<Vec<String>> {
    if type_flags < 0 {
        return None;
    }

    if type_flags & !known_type_mask() != 0 {
        return None;
    }

    let (primary, primary_label) = primary_type(type_flags)?;
    let mut card_type = match primary {
        CardKind::Monster => vec![primary_label, map_race(race_code)?],
        CardKind::Spell | CardKind::Trap => vec![primary_label],
    };
    card_type.extend(matched_subtype_flags(type_flags, primary));

    Some(card_type)
}

fn known_type_mask() -> i64 {
    rd_mappings()
        .primary_type_flags
        .iter()
        .chain(rd_mappings().subtype_flags.iter())
        .fold(rd_mappings().legend_type_mask, |known_mask, flag| {
            known_mask | flag.mask
        })
}

fn primary_type(type_flags: i64) -> Option<(CardKind, String)> {
    let mut primary = None;

    for flag in &rd_mappings().primary_type_flags {
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

    primary
}

fn matched_subtype_flags(type_flags: i64, primary: CardKind) -> Vec<String> {
    rd_mappings()
        .subtype_flags
        .iter()
        .filter_map(|flag| {
            let mappings = rd_mappings();
            if primary == CardKind::Monster
                && type_flags & mappings.ritual_type_mask != 0
                && flag.mask == mappings.fusion_type_mask
            {
                return None;
            }

            if type_flags & flag.mask != 0 {
                Some(flag.output_name.clone())
            } else {
                None
            }
        })
        .collect()
}

fn map_race(race_code: i64) -> Option<String> {
    map_race_name(&rd_mappings().race_codes, race_code)
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
        assert_eq!(map_attribute(0x00), Some(0));
        assert_eq!(map_attribute(0x10), Some(0));
        assert_eq!(map_attribute(0x20), Some(1));
        assert_eq!(map_attribute(0x08), Some(2));
        assert_eq!(map_attribute(0x01), Some(3));
        assert_eq!(map_attribute(0x04), Some(4));
        assert_eq!(map_attribute(0x02), Some(5));
        assert_eq!(map_attribute(0x40), None);
    }

    #[test]
    fn parses_type_bits_after_primary_from_high_to_low() {
        assert_eq!(map_card_types(0x2, 0), some_labels(&["魔法"]));
        assert_eq!(map_card_types(0x80002, 0), some_labels(&["魔法", "场地"]));
        assert_eq!(map_card_types(0x40002, 0), some_labels(&["魔法", "装备"]));
        assert_eq!(map_card_types(0x82, 0), some_labels(&["魔法", "仪式"]));
        assert_eq!(map_card_types(0xc, 0), some_labels(&["陷阱"]));
        assert_eq!(
            map_card_types(0x29, 0x2),
            some_labels(&["怪兽", "魔法师族", "效果"])
        );
        assert_eq!(
            map_card_types(0x61, 0x2000),
            some_labels(&["怪兽", "龙族", "融合", "效果"])
        );
        assert_eq!(
            map_card_types(0xc1, 0x1),
            some_labels(&["怪兽", "战士族", "仪式"])
        );
        assert_eq!(
            map_card_types(0xe1, 0x20000000),
            some_labels(&["怪兽", "天界战士族", "仪式", "效果"])
        );
        assert_eq!(
            map_card_types(0x8021, 0x40000000),
            some_labels(&["怪兽", "银河族", "极大", "效果"])
        );
        assert_eq!(
            map_card_types(0x421, 0x1),
            some_labels(&["怪兽", "战士族", "同盟", "效果"])
        );
    }

    #[test]
    fn rejects_invalid_type_bits() {
        assert_eq!(map_card_types(0, 0), None);
        assert_eq!(map_card_types(0x3, 0), None);
        assert_eq!(map_card_types(0x1000001, 0x1), None);
        assert_eq!(map_card_types(0x1, 0), None);
    }

    #[test]
    fn normalizes_monster_races() {
        assert_eq!(map_race(0x1), Some(String::from("战士族")));
        assert_eq!(map_race(0x2000), Some(String::from("龙族")));
        assert_eq!(map_race(0x40000000), Some(String::from("银河族")));
        assert_eq!(map_race(0x80000000), Some(String::from("电子人族")));
        assert_eq!(map_race(-0x80000000), Some(String::from("电子人族")));
        assert_eq!(map_race(0), None);
    }
}
