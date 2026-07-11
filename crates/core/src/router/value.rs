use std::{cmp::Ordering, collections::BTreeMap};

use num_bigint::BigInt;
use num_traits::Zero;
use serde_json::Value;

use super::RouterEvalError;

pub const MAX_STRING_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactDecimal {
    coefficient: BigInt,
    scale: i32,
}

impl ExactDecimal {
    pub fn coefficient(&self) -> &BigInt {
        &self.coefficient
    }

    pub fn scale(&self) -> i32 {
        self.scale
    }

    fn compare(&self, other: &Self) -> Ordering {
        let common_scale = self.scale.max(other.scale);
        scaled(&self.coefficient, common_scale - self.scale)
            .cmp(&scaled(&other.coefficient, common_scale - other.scale))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterNumber {
    Int(i64),
    Decimal(ExactDecimal),
}

impl RouterNumber {
    pub(crate) fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => left.cmp(right),
            _ => as_decimal(self).compare(&as_decimal(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterValue {
    Missing,
    Null,
    Bool(bool),
    Number(RouterNumber),
    String(String),
    List(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl RouterValue {
    pub fn from_json(value: &Value) -> Result<Self, RouterEvalError> {
        match value {
            Value::Null => Ok(Self::Null),
            Value::Bool(value) => Ok(Self::Bool(*value)),
            Value::Number(value) => Ok(Self::Number(parse_number(&value.to_string())?)),
            Value::String(value) => {
                check_string(value)?;
                Ok(Self::String(value.clone()))
            }
            Value::Array(values) => values
                .iter()
                .map(Self::from_json)
                .collect::<Result<_, _>>()
                .map(Self::List),
            Value::Object(values) => values
                .iter()
                .map(|(key, value)| {
                    check_string(key)?;
                    Ok((key.clone(), Self::from_json(value)?))
                })
                .collect::<Result<_, _>>()
                .map(Self::Object),
        }
    }

    pub(crate) fn deep_cost(&self) -> Option<u64> {
        match self {
            Self::Missing => None,
            Self::Null | Self::Bool(_) | Self::Number(_) => Some(1),
            Self::String(value) => 1_u64.checked_add(value.len() as u64),
            Self::List(values) => values
                .iter()
                .try_fold(1_u64, |sum, value| sum.checked_add(value.deep_cost()?)),
            Self::Object(values) => values.iter().try_fold(1_u64, |sum, (key, value)| {
                sum.checked_add(key.len() as u64)?
                    .checked_add(value.deep_cost()?)
            }),
        }
    }
}

pub(crate) fn parse_number(source: &str) -> Result<RouterNumber, RouterEvalError> {
    if !source.contains(['.', 'e', 'E']) {
        return source.parse::<i64>().map(RouterNumber::Int).map_err(|_| {
            numeric_error(format!("integer is outside signed 64-bit range: {source}"))
        });
    }
    parse_decimal(source).map(RouterNumber::Decimal)
}

fn parse_decimal(source: &str) -> Result<ExactDecimal, RouterEvalError> {
    let (negative, unsigned) = source
        .strip_prefix('-')
        .map_or((false, source), |value| (true, value));
    let (mantissa, exponent) = unsigned
        .split_once(['e', 'E'])
        .map_or((unsigned, Ok(0_i32)), |(left, right)| {
            (left, right.parse::<i32>())
        });
    let exponent = exponent.map_err(|_| numeric_error("decimal exponent is out of range"))?;
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |parts| parts);
    let mut digits = format!("{whole}{fraction}");
    let mut scale = i32::try_from(fraction.len())
        .ok()
        .and_then(|length| length.checked_sub(exponent))
        .ok_or_else(|| numeric_error("decimal scale is out of range"))?;
    while digits.ends_with('0') {
        digits.pop();
        scale = scale
            .checked_sub(1)
            .ok_or_else(|| numeric_error("decimal scale is out of range"))?;
    }
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return Ok(ExactDecimal {
            coefficient: BigInt::zero(),
            scale: 0,
        });
    }
    if significant.len() > 38 || scale.unsigned_abs() > 18 {
        return Err(numeric_error(
            "decimal exceeds 38 significant digits or scale 18",
        ));
    }
    let mut coefficient = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| numeric_error("invalid decimal coefficient"))?;
    if negative {
        coefficient = -coefficient;
    }
    Ok(ExactDecimal { coefficient, scale })
}

fn as_decimal(value: &RouterNumber) -> ExactDecimal {
    match value {
        RouterNumber::Int(value) => ExactDecimal {
            coefficient: BigInt::from(*value),
            scale: 0,
        },
        RouterNumber::Decimal(value) => value.clone(),
    }
}

fn scaled(value: &BigInt, places: i32) -> BigInt {
    value * BigInt::from(10_u8).pow(places as u32)
}

fn check_string(value: &str) -> Result<(), RouterEvalError> {
    if value.len() > MAX_STRING_BYTES {
        return Err(RouterEvalError::new(
            "router_value_too_large",
            "router string exceeds 64 KiB",
        ));
    }
    Ok(())
}

fn numeric_error(message: impl Into<String>) -> RouterEvalError {
    RouterEvalError::new("router_numeric_out_of_range", message)
}
