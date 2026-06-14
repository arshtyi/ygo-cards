use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

pub(crate) fn write_pretty_sorted(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut value = serde_json::to_value(value).context("failed to convert value to JSON")?;
    sort_value(&mut value);

    let file = File::create(path)
        .with_context(|| format!("failed to create output file {}", path.display()))?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &value)
        .with_context(|| format!("failed to write {}", path.display()))?;
    writer
        .write_all(b"\n")
        .with_context(|| format!("failed to finish {}", path.display()))?;

    Ok(())
}

fn sort_value(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                sort_value(value);
            }
        }
        Value::Object(object) => {
            for value in object.values_mut() {
                sort_value(value);
            }

            let mut entries = std::mem::take(object).into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            object.extend(entries);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn sorts_nested_object_keys() {
        let mut value = json!({
            "b": 1,
            "a": {
                "d": 4,
                "c": 3
            }
        });
        sort_value(&mut value);
        let keys = value.as_object().unwrap().keys().collect::<Vec<_>>();
        let nested_keys = value["a"].as_object().unwrap().keys().collect::<Vec<_>>();

        assert_eq!(keys, vec!["a", "b"]);
        assert_eq!(nested_keys, vec!["c", "d"]);
    }
}
