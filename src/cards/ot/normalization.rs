use crate::cards::{
    masks::{has_label, ot_masks},
    text::{normalize_card_text, strip_effect_text_note_lines},
};

pub(super) fn normalize_description(
    description: &str,
    card_type: &[String],
) -> Option<String> {
    let description = normalize_card_text(description);

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

pub(super) fn normalize_pendulum_description(
    description: &str,
    card_type: &[String],
) -> Option<Option<String>> {
    if !has_label(card_type, "灵摆") {
        return Some(None);
    }

    let description = normalize_card_text(description);
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

pub(super) fn normalize_atk(raw_atk: i64, card_type: &[String]) -> Option<Option<i64>> {
    if !has_label(card_type, "怪兽") {
        return Some(None);
    }

    let atk = if raw_atk == -2 { -1 } else { raw_atk };
    if atk >= -1 { Some(Some(atk)) } else { None }
}

pub(super) fn normalize_def(raw_def: i64, card_type: &[String]) -> Option<Option<i64>> {
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

pub(super) fn normalize_level(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
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

pub(super) fn normalize_rank(raw_level: i64, card_type: &[String]) -> Option<Option<i64>> {
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

pub(super) fn normalize_pendulum_scale(
    raw_level: i64,
    card_type: &[String],
) -> Option<Option<i64>> {
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

pub(super) fn normalize_link_value(
    raw_level: i64,
    card_type: &[String],
) -> Option<Option<i64>> {
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

pub(super) fn normalize_link_marker(
    raw_def: i64,
    card_type: &[String],
) -> Option<Option<Vec<i64>>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
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
            normalize_pendulum_description(
                "首行\r\nmissing marker",
                &labels(&["怪兽", "灵摆"])
            ),
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
            normalize_pendulum_scale(
                (7 << 24) | (8 << 16) | 4,
                &labels(&["怪兽", "灵摆"])
            ),
            None
        );
        assert_eq!(
            normalize_pendulum_scale(
                (14 << 24) | (14 << 16) | 4,
                &labels(&["怪兽", "灵摆"])
            ),
            None
        );
        assert_eq!(
            normalize_pendulum_scale(4, &labels(&["怪兽"])),
            Some(None)
        );
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
        assert_eq!(
            normalize_link_marker(0xaa, &labels(&["怪兽"])),
            Some(None)
        );
        assert_eq!(
            normalize_link_marker(0xaa, &labels(&["魔法"])),
            Some(None)
        );
    }
}
