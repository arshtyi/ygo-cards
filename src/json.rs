use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;

pub(crate) fn write_pretty_sorted(path: &Path, value: &impl Serialize) -> Result<()> {
    // serde_json's default map keeps object keys sorted, including nested objects.
    let value = serde_json::to_value(value).context("failed to convert value to JSON")?;

    let file = File::create(path)
        .with_context(|| format!("failed to create output file {}", path.display()))?;
    write_pretty_json(BufWriter::new(file), &value)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn write_pretty_json(mut writer: impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn writes_sorted_objects_without_reordering_arrays() {
        let value = json!([{"z": 1, "a": [{"d": 4, "c": 3}]}, {"id": 0}]);
        let mut output = Vec::new();

        write_pretty_json(&mut output, &value).unwrap();

        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[\n  {\n    \"a\": [\n      {\n        \"c\": 3,\n        \"d\": 4\n      }\n    ],\n    \"z\": 1\n  },\n  {\n    \"id\": 0\n  }\n]\n"
        );
    }

    #[test]
    fn reports_buffered_write_failures() {
        struct FullDisk;

        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::StorageFull.into())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let error = write_pretty_json(BufWriter::new(FullDisk), &json!([])).unwrap_err();

        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::StorageFull
        );
    }
}
