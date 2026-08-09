use std::sync::OnceLock;

use anyhow::Result;
use serde::Deserialize;

use crate::config::read_json_config;

const OT_MAPPINGS_PATH: &str = "config/ot-field-mappings.json";
const RD_MAPPINGS_PATH: &str = "config/rd-field-mappings.json";

static OT_MAPPINGS: OnceLock<std::result::Result<OtFieldMappings, String>> = OnceLock::new();
static RD_MAPPINGS: OnceLock<std::result::Result<RdFieldMappings, String>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AttributeCodeMapping {
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) raw_code: i64,
    pub(crate) output_value: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RaceCodeMapping {
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) raw_code: i64,
    pub(crate) output_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TypeFlag {
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) mask: i64,
    pub(crate) output_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LinkMarkerFlag {
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) mask: i64,
    pub(crate) output_position: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MaximumPositionMapping {
    pub(crate) position: i64,
    pub(crate) markers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OtFieldMappings {
    pub(crate) attribute_codes: Vec<AttributeCodeMapping>,
    pub(crate) primary_type_flags: Vec<TypeFlag>,
    pub(crate) subtype_flags: Vec<TypeFlag>,
    pub(crate) race_codes: Vec<RaceCodeMapping>,
    pub(crate) link_marker_flags: Vec<LinkMarkerFlag>,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) inferred_monster_type_mask: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RdFieldMappings {
    pub(crate) attribute_codes: Vec<AttributeCodeMapping>,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) legend_type_mask: i64,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) fusion_type_mask: i64,
    #[serde(deserialize_with = "deserialize_integer")]
    pub(crate) ritual_type_mask: i64,
    pub(crate) primary_type_flags: Vec<TypeFlag>,
    pub(crate) subtype_flags: Vec<TypeFlag>,
    pub(crate) race_codes: Vec<RaceCodeMapping>,
    pub(crate) maximum_position_markers: Vec<MaximumPositionMapping>,
}

pub(crate) fn ot_mappings() -> &'static OtFieldMappings {
    ensure_ot_mappings().expect("OT field mappings must be validated before card normalization")
}

pub(crate) fn rd_mappings() -> &'static RdFieldMappings {
    ensure_rd_mappings().expect("RD field mappings must be validated before card normalization")
}

pub(crate) fn ensure_ot_mappings() -> Result<&'static OtFieldMappings> {
    OT_MAPPINGS
        .get_or_init(|| load_ot_mappings().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("failed to load OT field mappings: {error}"))
}

pub(crate) fn ensure_rd_mappings() -> Result<&'static RdFieldMappings> {
    RD_MAPPINGS
        .get_or_init(|| load_rd_mappings().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("failed to load RD field mappings: {error}"))
}

pub(crate) fn map_attribute_value(entries: &[AttributeCodeMapping], raw_code: i64) -> Option<i64> {
    entries
        .iter()
        .find(|entry| entry.raw_code == raw_code)
        .map(|entry| entry.output_value)
}

pub(crate) fn map_race_name(entries: &[RaceCodeMapping], raw_code: i64) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.raw_code == raw_code)
        .map(|entry| entry.output_name.clone())
}

fn load_ot_mappings() -> Result<OtFieldMappings> {
    read_json_config(OT_MAPPINGS_PATH, "OT field mapping config")
}

fn load_rd_mappings() -> Result<RdFieldMappings> {
    read_json_config(RD_MAPPINGS_PATH, "RD field mapping config")
}

fn deserialize_integer<'de, D>(deserializer: D) -> std::result::Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match IntegerLiteral::deserialize(deserializer)? {
        IntegerLiteral::Number(value) => Ok(value),
        IntegerLiteral::Text(value) => parse_integer(&value).map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IntegerLiteral {
    Number(i64),
    Text(String),
}

fn parse_integer(value: &str) -> std::result::Result<i64, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(String::from("empty integer value"));
    }

    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or((false, value), |rest| (true, rest));
    let parsed = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16)
            .map_err(|error| format!("invalid hexadecimal integer {value:?}: {error}"))?
    } else {
        unsigned
            .parse::<i64>()
            .map_err(|error| format!("invalid decimal integer {value:?}: {error}"))?
    };

    if negative { Ok(-parsed) } else { Ok(parsed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hexadecimal_and_decimal_integers() {
        assert_eq!(parse_integer("0x40").unwrap(), 64);
        assert_eq!(parse_integer("-0x80000000").unwrap(), -2_147_483_648);
        assert_eq!(parse_integer("128").unwrap(), 128);
        assert!(parse_integer("").is_err());
        assert!(parse_integer("0xzz").is_err());
    }

    #[test]
    fn loads_field_mapping_configs() {
        let ot = ensure_ot_mappings().unwrap();
        assert_eq!(map_attribute_value(&ot.attribute_codes, 0x40), Some(0));
        assert_eq!(
            map_race_name(&ot.race_codes, 0x2000),
            Some(String::from("龙族"))
        );

        let rd = ensure_rd_mappings().unwrap();
        assert_eq!(rd.legend_type_mask, 0x8);
        assert_eq!(
            map_race_name(&rd.race_codes, -0x80000000),
            Some(String::from("电子人族"))
        );
    }
}
