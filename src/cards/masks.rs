use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result};
use serde::Deserialize;

const OT_MASKS: &str = "config/ot-masks.json";
const RD_MASKS: &str = "config/rd-masks.json";

#[derive(Debug)]
pub(crate) struct ValueEntry {
    pub(crate) bit: i64,
    pub(crate) value: i64,
}

#[derive(Debug)]
pub(crate) struct LabelEntry {
    pub(crate) bit: i64,
    pub(crate) label: String,
}

#[derive(Debug)]
pub(crate) struct MaximumNameMarker {
    pub(crate) value: i64,
    pub(crate) markers: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct OtMasks {
    pub(crate) attributes: Vec<ValueEntry>,
    pub(crate) primary_types: Vec<LabelEntry>,
    pub(crate) subtypes: Vec<LabelEntry>,
    pub(crate) races: Vec<LabelEntry>,
    pub(crate) link_markers: Vec<ValueEntry>,
    pub(crate) inferred_monster_type_bit: i64,
}

#[derive(Debug)]
pub(crate) struct RdMasks {
    pub(crate) attributes: Vec<ValueEntry>,
    pub(crate) legend_type: i64,
    pub(crate) fusion_type: i64,
    pub(crate) ritual_type: i64,
    pub(crate) primary_types: Vec<LabelEntry>,
    pub(crate) subtypes: Vec<LabelEntry>,
    pub(crate) races: Vec<LabelEntry>,
    pub(crate) maximum_name_markers: Vec<MaximumNameMarker>,
}

pub(crate) fn ot_masks() -> &'static OtMasks {
    static MASKS: OnceLock<OtMasks> = OnceLock::new();
    MASKS.get_or_init(|| {
        load_ot_masks().unwrap_or_else(|error| panic!("failed to load OT mask config: {error:#}"))
    })
}

pub(crate) fn rd_masks() -> &'static RdMasks {
    static MASKS: OnceLock<RdMasks> = OnceLock::new();
    MASKS.get_or_init(|| {
        load_rd_masks().unwrap_or_else(|error| panic!("failed to load RD mask config: {error:#}"))
    })
}

pub(crate) fn mapped_value(entries: &[ValueEntry], raw: i64) -> Option<i64> {
    entries
        .iter()
        .find(|entry| entry.bit == raw)
        .map(|entry| entry.value)
}

pub(crate) fn mapped_label(entries: &[LabelEntry], raw: i64) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.bit == raw)
        .map(|entry| entry.label.clone())
}

pub(crate) fn has_label(labels: &[String], label: &str) -> bool {
    labels.iter().any(|value| value == label)
}

fn load_ot_masks() -> Result<OtMasks> {
    let raw: RawOtMasks = read_config(OT_MASKS)?;
    Ok(raw.into_masks())
}

fn load_rd_masks() -> Result<RdMasks> {
    let raw: RawRdMasks = read_config(RD_MASKS)?;
    Ok(raw.into_masks())
}

fn read_config<T>(path: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let path = manifest_path(path);
    let text = fs::read_to_string(&path)
        .with_context(|| format!("failed to read mask config {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse mask config {}", path.display()))
}

fn manifest_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOtMasks {
    attributes: Vec<RawValueEntry>,
    primary_types: Vec<RawLabelEntry>,
    subtypes: Vec<RawLabelEntry>,
    races: Vec<RawLabelEntry>,
    link_markers: Vec<RawValueEntry>,
    inferred_monster_type_bit: MaskValue,
}

impl RawOtMasks {
    fn into_masks(self) -> OtMasks {
        OtMasks {
            attributes: self.attributes.into_iter().map(Into::into).collect(),
            primary_types: self.primary_types.into_iter().map(Into::into).collect(),
            subtypes: self.subtypes.into_iter().map(Into::into).collect(),
            races: self.races.into_iter().map(Into::into).collect(),
            link_markers: self.link_markers.into_iter().map(Into::into).collect(),
            inferred_monster_type_bit: self.inferred_monster_type_bit.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRdMasks {
    attributes: Vec<RawValueEntry>,
    legend_type: MaskValue,
    fusion_type: MaskValue,
    ritual_type: MaskValue,
    primary_types: Vec<RawLabelEntry>,
    subtypes: Vec<RawLabelEntry>,
    races: Vec<RawLabelEntry>,
    maximum_name_markers: Vec<RawMaximumNameMarker>,
}

impl RawRdMasks {
    fn into_masks(self) -> RdMasks {
        RdMasks {
            attributes: self.attributes.into_iter().map(Into::into).collect(),
            legend_type: self.legend_type.0,
            fusion_type: self.fusion_type.0,
            ritual_type: self.ritual_type.0,
            primary_types: self.primary_types.into_iter().map(Into::into).collect(),
            subtypes: self.subtypes.into_iter().map(Into::into).collect(),
            races: self.races.into_iter().map(Into::into).collect(),
            maximum_name_markers: self
                .maximum_name_markers
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawValueEntry {
    bit: MaskValue,
    value: i64,
}

impl From<RawValueEntry> for ValueEntry {
    fn from(value: RawValueEntry) -> Self {
        Self {
            bit: value.bit.0,
            value: value.value,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawLabelEntry {
    bit: MaskValue,
    label: String,
}

impl From<RawLabelEntry> for LabelEntry {
    fn from(value: RawLabelEntry) -> Self {
        Self {
            bit: value.bit.0,
            label: value.label,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawMaximumNameMarker {
    value: i64,
    markers: Vec<String>,
}

impl From<RawMaximumNameMarker> for MaximumNameMarker {
    fn from(value: RawMaximumNameMarker) -> Self {
        Self {
            value: value.value,
            markers: value.markers,
        }
    }
}

#[derive(Debug)]
struct MaskValue(i64);

impl<'de> Deserialize<'de> for MaskValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match RawMaskValue::deserialize(deserializer)? {
            RawMaskValue::Number(value) => Ok(Self(value)),
            RawMaskValue::Text(value) => parse_mask_text(&value)
                .map(Self)
                .map_err(serde::de::Error::custom),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawMaskValue {
    Number(i64),
    Text(String),
}

fn parse_mask_text(value: &str) -> std::result::Result<i64, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(String::from("empty mask value"));
    }

    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |rest| (true, rest));

    let parsed = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
            .map_err(|error| format!("invalid hex mask value {value:?}: {error}"))?
    } else {
        unsigned
            .parse::<i64>()
            .map_err(|error| format!("invalid decimal mask value {value:?}: {error}"))?
    };

    if negative { Ok(-parsed) } else { Ok(parsed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_decimal_mask_values() {
        assert_eq!(parse_mask_text("0x40").unwrap(), 64);
        assert_eq!(parse_mask_text("-0x80000000").unwrap(), -2147483648);
        assert_eq!(parse_mask_text("128").unwrap(), 128);
        assert!(parse_mask_text("").is_err());
        assert!(parse_mask_text("0xzz").is_err());
    }

    #[test]
    fn loads_mask_configs() {
        let ot = ot_masks();
        assert_eq!(mapped_value(&ot.attributes, 0x40), Some(0));
        assert_eq!(mapped_label(&ot.races, 0x2000), Some(String::from("龙族")));

        let rd = rd_masks();
        assert_eq!(rd.legend_type, 0x8);
        assert_eq!(
            mapped_label(&rd.races, -0x80000000),
            Some(String::from("电子人族"))
        );
    }
}
