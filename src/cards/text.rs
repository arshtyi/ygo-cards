pub(super) fn normalize_newlines(text: &str) -> String {
    collapse_consecutive_newlines(&text.replace("\r\n", "\n").replace('\r', "\n"))
}

pub(super) fn strip_effect_text_note_lines(text: &str) -> String {
    let mut removed = false;
    let mut lines = Vec::new();

    for line in text.lines() {
        if is_effect_text_note_line(line) {
            removed = true;
        } else {
            lines.push(line);
        }
    }

    if removed {
        lines.join("\n")
    } else {
        text.to_string()
    }
}

fn collapse_consecutive_newlines(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut previous_was_newline = false;

    for character in text.chars() {
        if character == '\n' {
            if !previous_was_newline {
                normalized.push(character);
            }
            previous_was_newline = true;
        } else {
            normalized.push(character);
            previous_was_newline = false;
        }
    }

    normalized
}

fn is_effect_text_note_line(line: &str) -> bool {
    let line = line.trim();
    matches!(
        line,
        "（限制类效果可在基本分处查看）"
            | "（限制类效果可在基本分处查看)"
            | "（状态类效果可在基本分处查看）"
            | "（状态类效果可在基本分处查看)"
            | "（限制类和状态类效果可在基本分处查看）"
            | "（限制类和状态类效果可在基本分处查看)"
    )
        || (line.starts_with("（注：") && (line.ends_with('）') || line.ends_with(')')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_newlines() {
        assert_eq!(normalize_newlines("a\r\nb"), "a\nb");
        assert_eq!(normalize_newlines("a\r\r\n\r\nb"), "a\nb");
        assert_eq!(normalize_newlines("a\n\n\nb"), "a\nb");
    }

    #[test]
    fn strips_standalone_effect_note_lines() {
        assert_eq!(
            strip_effect_text_note_lines("（注：暂时无法正常使用）\r\n正文"),
            "正文"
        );
        assert_eq!(
            strip_effect_text_note_lines("正文\r\n（限制类效果可在基本分处查看）"),
            "正文"
        );
        assert_eq!(
            strip_effect_text_note_lines("正文\r\n（状态类效果可在基本分处查看）\r\n"),
            "正文"
        );
        assert_eq!(
            strip_effect_text_note_lines("正文\r\n（限制类和状态类效果可在基本分处查看）"),
            "正文"
        );
        assert_eq!(
            strip_effect_text_note_lines("上\r\n（注：有bug)\r\n下"),
            "上\n下"
        );
        assert_eq!(
            strip_effect_text_note_lines("正文（注：这里是正文的一部分）"),
            "正文（注：这里是正文的一部分）"
        );
    }
}
