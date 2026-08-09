pub(crate) fn format_count(value: impl ToString) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(character);
    }
    formatted
}

pub(crate) fn plural<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_counts_and_plurals() {
        assert_eq!(format_count(12_345_678_u64), "12,345,678");
        assert_eq!(plural(1, "card", "cards"), "card");
        assert_eq!(plural(2, "card", "cards"), "cards");
    }
}
