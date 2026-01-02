use crate::environment::models::types::Percentage;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, de};
use std::fmt;

impl<'de> Deserialize<'de> for Percentage {
    fn deserialize<D>(deserializer: D) -> Result<Percentage, D::Error>
    where
        D: Deserializer<'de>,
    {
        match deserializer.deserialize_f64(PercentageVisitor) {
            Ok(value) => Percentage::try_from(value).map_err(de::Error::custom),
            Err(e) => Err(e),
        }
    }
}

struct PercentageVisitor;

impl Visitor<'_> for PercentageVisitor {
    type Value = f64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a percentage value between 0.0 and 1.0")
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !(0.0..=1.0).contains(&value) {
            Err(E::custom("Percentage value is out of range"))
        } else {
            Ok(value as f64)
        }
    }
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if !(0.0..=1.0).contains(&value) {
            Err(E::custom("Percentage value is out of range"))
        } else {
            Ok(value)
        }
    }
}
