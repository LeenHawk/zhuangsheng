use std::cmp::Ordering;

use serde_json::Value;

use super::{
    CompiledExpression, RouterEvalError, RouterNumber, RouterValue,
    ast::{AccessKey, BinaryOp, Expr, Function, Literal, Root},
    value::{MAX_STRING_BYTES, parse_number},
};

const EXPRESSION_FUEL: u64 = 10_000;
const ACTIVATION_FUEL: u64 = 50_000;
const MAX_RESOLVED_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationEnvironment {
    pub inputs: RouterValue,
    pub memory: RouterValue,
    pub control: RouterValue,
}

impl EvaluationEnvironment {
    pub fn from_json(
        inputs: &Value,
        memory: &Value,
        control: &Value,
    ) -> Result<Self, RouterEvalError> {
        let resolved_bytes = serde_json::to_vec(inputs)
            .ok()
            .and_then(|left| {
                serde_json::to_vec(memory)
                    .ok()
                    .and_then(|right| left.len().checked_add(right.len()))
            })
            .ok_or_else(|| value_too_large("resolved Router values cannot be measured"))?;
        if resolved_bytes > MAX_RESOLVED_BYTES {
            return Err(value_too_large(
                "resolved Router inputs and memory exceed 1 MiB",
            ));
        }
        Ok(Self {
            inputs: RouterValue::from_json(inputs)?,
            memory: RouterValue::from_json(memory)?,
            control: RouterValue::from_json(control)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationFuel {
    remaining: u64,
}

impl Default for ActivationFuel {
    fn default() -> Self {
        Self::new(ACTIVATION_FUEL)
    }
}

impl ActivationFuel {
    pub fn new(limit: u64) -> Self {
        Self {
            remaining: limit.min(ACTIVATION_FUEL),
        }
    }

    pub fn remaining(&self) -> u64 {
        self.remaining
    }
}

pub fn evaluate_expression(
    expression: &CompiledExpression,
    environment: &EvaluationEnvironment,
    activation_fuel: &mut ActivationFuel,
) -> Result<bool, RouterEvalError> {
    let mut evaluator = Evaluator {
        environment,
        expression_fuel: EXPRESSION_FUEL,
        activation_fuel,
    };
    match evaluator.evaluate(&expression.ast)? {
        RouterValue::Bool(value) => Ok(value),
        RouterValue::Missing => Err(RouterEvalError::missing()),
        _ => Err(RouterEvalError::type_error(
            "Router rule result must be boolean",
        )),
    }
}

struct Evaluator<'a> {
    environment: &'a EvaluationEnvironment,
    expression_fuel: u64,
    activation_fuel: &'a mut ActivationFuel,
}

impl Evaluator<'_> {
    fn evaluate(&mut self, expression: &Expr) -> Result<RouterValue, RouterEvalError> {
        self.charge(1)?;
        match expression {
            Expr::Literal(value) => self.literal(value),
            Expr::Root(root) => Ok(match root {
                Root::Inputs => self.environment.inputs.clone(),
                Root::Memory => self.environment.memory.clone(),
                Root::Control => self.environment.control.clone(),
            }),
            Expr::List(values) => values
                .iter()
                .map(|value| self.evaluate(value))
                .collect::<Result<_, _>>()
                .map(RouterValue::List),
            Expr::Field(value, field) => {
                let value = self.evaluate(value)?;
                self.access(value, &AccessKey::Field(field.clone()))
            }
            Expr::Index(value, key) => {
                let value = self.evaluate(value)?;
                self.access(value, key)
            }
            Expr::Not(value) => match self.evaluate(value)? {
                RouterValue::Bool(value) => Ok(RouterValue::Bool(!value)),
                RouterValue::Missing => Err(RouterEvalError::missing()),
                _ => Err(RouterEvalError::type_error("'!' requires boolean")),
            },
            Expr::Binary(operator, left, right) => self.binary(*operator, left, right),
            Expr::Call(function, arguments) => self.call(*function, arguments),
        }
    }

    fn literal(&self, literal: &Literal) -> Result<RouterValue, RouterEvalError> {
        match literal {
            Literal::Null => Ok(RouterValue::Null),
            Literal::Bool(value) => Ok(RouterValue::Bool(*value)),
            Literal::String(value) if value.len() <= MAX_STRING_BYTES => {
                Ok(RouterValue::String(value.clone()))
            }
            Literal::String(_) => Err(value_too_large("string literal exceeds 64 KiB")),
            Literal::Number(value) => Ok(RouterValue::Number(parse_number(value)?)),
        }
    }

    fn access(&self, value: RouterValue, key: &AccessKey) -> Result<RouterValue, RouterEvalError> {
        match (value, key) {
            (RouterValue::Object(values), AccessKey::Field(field)) => {
                Ok(values.get(field).cloned().unwrap_or(RouterValue::Missing))
            }
            (RouterValue::List(values), AccessKey::Index(index)) => usize::try_from(*index)
                .ok()
                .and_then(|index| values.get(index).cloned())
                .ok_or_else(|| {
                    RouterEvalError::new(
                        "router_index_out_of_range",
                        "Router list index is out of range",
                    )
                }),
            (RouterValue::Missing, _) => Err(RouterEvalError::missing()),
            (RouterValue::Null, _) => Err(RouterEvalError::type_error(
                "cannot access field or index on null",
            )),
            (_, AccessKey::Field(_)) => Err(RouterEvalError::type_error(
                "field access requires an object",
            )),
            (_, AccessKey::Index(_)) => Err(RouterEvalError::type_error(
                "integer index access requires a list",
            )),
        }
    }

    fn binary(
        &mut self,
        operator: BinaryOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<RouterValue, RouterEvalError> {
        if matches!(operator, BinaryOp::And | BinaryOp::Or) {
            return self.logical(operator, left, right);
        }
        let left = self.evaluate(left)?;
        let right = self.evaluate(right)?;
        let result = match operator {
            BinaryOp::Equal => self.equal(&left, &right)?,
            BinaryOp::NotEqual => !self.equal(&left, &right)?,
            BinaryOp::Less => self.order(&left, &right)? == Ordering::Less,
            BinaryOp::LessEqual => self.order(&left, &right)? != Ordering::Greater,
            BinaryOp::Greater => self.order(&left, &right)? == Ordering::Greater,
            BinaryOp::GreaterEqual => self.order(&left, &right)? != Ordering::Less,
            BinaryOp::In => self.contains_value(&left, &right)?,
            BinaryOp::And | BinaryOp::Or => unreachable!(),
        };
        Ok(RouterValue::Bool(result))
    }

    fn logical(
        &mut self,
        operator: BinaryOp,
        left: &Expr,
        right: &Expr,
    ) -> Result<RouterValue, RouterEvalError> {
        let left = match self.evaluate(left)? {
            RouterValue::Bool(value) => value,
            RouterValue::Missing => return Err(RouterEvalError::missing()),
            _ => {
                return Err(RouterEvalError::type_error(
                    "logical operators require booleans",
                ));
            }
        };
        if (operator == BinaryOp::And && !left) || (operator == BinaryOp::Or && left) {
            return Ok(RouterValue::Bool(left));
        }
        match self.evaluate(right)? {
            RouterValue::Bool(value) => Ok(RouterValue::Bool(value)),
            RouterValue::Missing => Err(RouterEvalError::missing()),
            _ => Err(RouterEvalError::type_error(
                "logical operators require booleans",
            )),
        }
    }

    fn equal(&mut self, left: &RouterValue, right: &RouterValue) -> Result<bool, RouterEvalError> {
        if matches!(left, RouterValue::Missing) || matches!(right, RouterValue::Missing) {
            return Err(RouterEvalError::missing());
        }
        match (left, right) {
            (RouterValue::String(left), RouterValue::String(right)) => {
                self.charge(sum_lengths(left, right)?)?;
                Ok(left == right)
            }
            (RouterValue::List(_), RouterValue::List(_))
            | (RouterValue::Object(_), RouterValue::Object(_)) => {
                let cost = left
                    .deep_cost()
                    .and_then(|cost| cost.checked_add(right.deep_cost()?))
                    .ok_or_else(RouterEvalError::complexity)?;
                self.charge(cost)?;
                Ok(structural_equal(left, right))
            }
            (RouterValue::Number(left), RouterValue::Number(right)) => {
                Ok(left.compare(right) == Ordering::Equal)
            }
            _ => Ok(left == right),
        }
    }

    fn order(
        &mut self,
        left: &RouterValue,
        right: &RouterValue,
    ) -> Result<Ordering, RouterEvalError> {
        if matches!(left, RouterValue::Missing) || matches!(right, RouterValue::Missing) {
            return Err(RouterEvalError::missing());
        }
        match (left, right) {
            (RouterValue::Number(left), RouterValue::Number(right)) => Ok(left.compare(right)),
            (RouterValue::String(left), RouterValue::String(right)) => {
                self.charge(sum_lengths(left, right)?)?;
                Ok(left.cmp(right))
            }
            _ => Err(RouterEvalError::type_error(
                "ordering requires two numbers or two strings",
            )),
        }
    }

    fn contains_value(
        &mut self,
        needle: &RouterValue,
        haystack: &RouterValue,
    ) -> Result<bool, RouterEvalError> {
        if matches!(needle, RouterValue::Missing) {
            return Err(RouterEvalError::missing());
        }
        let RouterValue::List(values) = haystack else {
            return if matches!(haystack, RouterValue::Missing) {
                Err(RouterEvalError::missing())
            } else {
                Err(RouterEvalError::type_error(
                    "right side of 'in' must be a list",
                ))
            };
        };
        for value in values {
            self.charge(1)?;
            if self.equal(needle, value)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn call(
        &mut self,
        function: Function,
        arguments: &[Expr],
    ) -> Result<RouterValue, RouterEvalError> {
        let arguments = arguments
            .iter()
            .map(|value| self.evaluate(value))
            .collect::<Result<Vec<_>, _>>()?;
        if arguments.contains(&RouterValue::Missing) {
            return Err(RouterEvalError::missing());
        }
        match function {
            Function::Has => self.has(&arguments),
            Function::Size => self.size(&arguments),
            Function::Contains | Function::StartsWith | Function::EndsWith => {
                self.string_predicate(function, &arguments)
            }
            Function::LowerAscii | Function::UpperAscii => self.ascii_case(function, &arguments),
        }
    }

    fn has(&self, arguments: &[RouterValue]) -> Result<RouterValue, RouterEvalError> {
        match arguments {
            [RouterValue::Object(values), RouterValue::String(key)] => {
                Ok(RouterValue::Bool(values.contains_key(key)))
            }
            _ => Err(RouterEvalError::type_error(
                "has requires an object and a string",
            )),
        }
    }

    fn size(&self, arguments: &[RouterValue]) -> Result<RouterValue, RouterEvalError> {
        let size = match arguments {
            [RouterValue::String(value)] => value.chars().count(),
            [RouterValue::List(value)] => value.len(),
            [RouterValue::Object(value)] => value.len(),
            _ => {
                return Err(RouterEvalError::type_error(
                    "size requires a string, list, or object",
                ));
            }
        };
        Ok(RouterValue::Number(RouterNumber::Int(size as i64)))
    }

    fn string_predicate(
        &mut self,
        function: Function,
        arguments: &[RouterValue],
    ) -> Result<RouterValue, RouterEvalError> {
        let [RouterValue::String(left), RouterValue::String(right)] = arguments else {
            return Err(RouterEvalError::type_error(
                "string predicate requires two strings",
            ));
        };
        self.charge(sum_lengths(left, right)?)?;
        Ok(RouterValue::Bool(match function {
            Function::Contains => left.contains(right),
            Function::StartsWith => left.starts_with(right),
            Function::EndsWith => left.ends_with(right),
            _ => unreachable!(),
        }))
    }

    fn ascii_case(
        &mut self,
        function: Function,
        arguments: &[RouterValue],
    ) -> Result<RouterValue, RouterEvalError> {
        let [RouterValue::String(value)] = arguments else {
            return Err(RouterEvalError::type_error(
                "ASCII case conversion requires a string",
            ));
        };
        self.charge(value.len() as u64)?;
        Ok(RouterValue::String(match function {
            Function::LowerAscii => value.to_ascii_lowercase(),
            Function::UpperAscii => value.to_ascii_uppercase(),
            _ => unreachable!(),
        }))
    }

    fn charge(&mut self, cost: u64) -> Result<(), RouterEvalError> {
        if self.expression_fuel < cost || self.activation_fuel.remaining < cost {
            return Err(RouterEvalError::complexity());
        }
        self.expression_fuel -= cost;
        self.activation_fuel.remaining -= cost;
        Ok(())
    }
}

fn structural_equal(left: &RouterValue, right: &RouterValue) -> bool {
    match (left, right) {
        (RouterValue::Number(left), RouterValue::Number(right)) => {
            left.compare(right) == Ordering::Equal
        }
        (RouterValue::List(left), RouterValue::List(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| structural_equal(left, right))
        }
        (RouterValue::Object(left), RouterValue::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| structural_equal(left, right))
                })
        }
        _ => left == right,
    }
}

fn sum_lengths(left: &str, right: &str) -> Result<u64, RouterEvalError> {
    (left.len() as u64)
        .checked_add(right.len() as u64)
        .ok_or_else(RouterEvalError::complexity)
}

fn value_too_large(message: impl Into<String>) -> RouterEvalError {
    RouterEvalError::new("router_value_too_large", message)
}
