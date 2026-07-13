use crate::{DomainError, DomainResult};

use super::{MAX_EXPONENT_MAGNITUDE, MAX_NUMBER_DIGITS};

pub(super) fn normalize_number(raw: &str) -> DomainResult<String> {
    let number = analyze(raw, MAX_NUMBER_DIGITS, i64::from(MAX_EXPONENT_MAGNITUDE))?;
    if number.significant.is_empty() {
        return Ok("0".into());
    }

    let decimal_position = number.scientific_exponent + 1;
    let mut output = String::new();
    if number.negative {
        output.push('-');
    }
    if decimal_position <= 0 {
        output.push_str("0.");
        output.extend(std::iter::repeat_n('0', (-decimal_position) as usize));
        output.push_str(&number.significant);
    } else if decimal_position as usize >= number.significant.len() {
        output.push_str(&number.significant);
        output.extend(std::iter::repeat_n(
            '0',
            decimal_position as usize - number.significant.len(),
        ));
    } else {
        let split = decimal_position as usize;
        output.push_str(&number.significant[..split]);
        output.push('.');
        output.push_str(&number.significant[split..]);
    }
    Ok(output)
}

pub(super) fn validate_number(raw: &str, max_digits: usize, max_exponent: i64) -> DomainResult<()> {
    analyze(raw, max_digits, max_exponent).map(|_| ())
}

struct AnalyzedNumber {
    negative: bool,
    significant: String,
    scientific_exponent: i64,
}

fn analyze(raw: &str, max_digits: usize, max_exponent: i64) -> DomainResult<AnalyzedNumber> {
    if max_digits == 0 || max_exponent < 0 {
        return Err(limit("number limits are invalid"));
    }
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw), |value| (true, value));
    let (coefficient, explicit_exponent) = split_exponent(unsigned, max_exponent)?;
    let (integer, fraction) = coefficient.split_once('.').unwrap_or((coefficient, ""));
    let raw_digits = format!("{integer}{fraction}");
    if raw_digits.len() > max_digits {
        return Err(limit("number digits exceeded"));
    }

    let without_leading = raw_digits.trim_start_matches('0');
    if without_leading.is_empty() {
        return Ok(AnalyzedNumber {
            negative: false,
            significant: String::new(),
            scientific_exponent: 0,
        });
    }
    let significant = without_leading.trim_end_matches('0');
    let trailing_zeros = without_leading.len() - significant.len();
    let scientific_exponent = explicit_exponent
        .checked_sub(fraction.len() as i64)
        .and_then(|value| value.checked_add(trailing_zeros as i64))
        .and_then(|value| value.checked_add(significant.len() as i64 - 1))
        .ok_or_else(|| limit("number exponent exceeded"))?;
    if scientific_exponent.unsigned_abs() > max_exponent as u64 {
        return Err(limit("number exponent exceeded"));
    }
    Ok(AnalyzedNumber {
        negative,
        significant: significant.into(),
        scientific_exponent,
    })
}

fn split_exponent(value: &str, max_exponent: i64) -> DomainResult<(&str, i64)> {
    let Some(index) = value.find(['e', 'E']) else {
        return Ok((value, 0));
    };
    let exponent = value[index + 1..]
        .parse::<i64>()
        .map_err(|_| limit("number exponent exceeded"))?;
    if exponent.unsigned_abs() > max_exponent as u64 {
        return Err(limit("number exponent exceeded"));
    }
    Ok((&value[..index], exponent))
}

fn limit(message: &str) -> DomainError {
    DomainError::JsonLimit(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_minimal_plain_decimal_for_equivalent_numbers() {
        for raw in ["1", "1.0", "10e-1", "0.10e1"] {
            assert_eq!(normalize_number(raw).unwrap(), "1");
        }
        assert_eq!(normalize_number("-123.4500").unwrap(), "-123.45");
        assert_eq!(normalize_number("1e-3").unwrap(), "0.001");
        assert_eq!(normalize_number("-0.000e1024").unwrap(), "0");
    }
}
