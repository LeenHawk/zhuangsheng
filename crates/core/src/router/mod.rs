pub mod ast;
mod decision;
mod error;
mod eval;
pub mod lexer;
mod parser;
mod value;

pub use error::{RouterCompileError, RouterEvalError};
pub use eval::{ActivationFuel, EvaluationEnvironment, evaluate_expression};
pub use parser::{CompiledExpression, compile_expression};
pub use value::{ExactDecimal, RouterNumber, RouterValue};

#[cfg(test)]
mod decision_tests;
#[cfg(test)]
mod tests;
pub use decision::{
    RouterControlSnapshot, RouterDecision, RouterDecisionError, RouterDecisionReason,
    RouterLimitReason, evaluate_router,
};
