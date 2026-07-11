use crate::cards::{
    masks::{has_label, rd_masks},
    text::{normalize_card_text, strip_effect_text_note_lines},
};

pub(super) fn monster_value(value: i64, card_type: &[String]) -> Option<i64> {
    has_label(card_type, "怪兽").then_some(value)
}

pub(super) fn normalize_maximum(
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

pub(super) fn normalize_maximum_atk(
    maximum: Option<i64>,
    raw_description: &str,
) -> Option<Option<i64>> {
    if maximum != Some(1) {
        return Some(None);
    }

    parse_maximum_atk(raw_description).map(Some)
}

pub(super) fn normalize_description(raw_description: &str) -> String {
    let description = normalize_card_text(raw_description);
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
    normalize_card_text(raw_description)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| String::from(*value)).collect()
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
}
