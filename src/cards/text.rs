pub(super) fn normalize_newlines(text: &str) -> String {
    collapse_consecutive_newlines(&text.replace("\r\n", "\n").replace('\r', "\n"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_newlines() {
        assert_eq!(normalize_newlines("a\r\nb"), "a\nb");
        assert_eq!(normalize_newlines("a\r\r\n\r\nb"), "a\nb");
        assert_eq!(normalize_newlines("a\n\n\nb"), "a\nb");
    }
}
