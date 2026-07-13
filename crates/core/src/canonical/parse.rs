use std::{collections::HashSet, fmt};

use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};

use crate::{DomainError, DomainResult};

pub(super) fn reject_duplicate_keys(input: &str) -> DomainResult<()> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    NoDuplicateValue::deserialize(&mut deserializer)
        .map_err(|error| DomainError::InvalidJson(error.to_string()))?;
    deserializer
        .end()
        .map_err(|error| DomainError::InvalidJson(error.to_string()))
}

struct NoDuplicateValue;

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicateVisitor)
    }
}

struct NoDuplicateVisitor;

impl<'de> Visitor<'de> for NoDuplicateVisitor {
    type Value = NoDuplicateValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while sequence.next_element::<NoDuplicateValue>()?.is_some() {}
        Ok(NoDuplicateValue)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key: {key}"
                )));
            }
            map.next_value::<NoDuplicateValue>()?;
        }
        Ok(NoDuplicateValue)
    }
}
