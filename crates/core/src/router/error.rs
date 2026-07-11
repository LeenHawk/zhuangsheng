use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterCompileError {
    pub code: &'static str,
    pub message: String,
}

impl RouterCompileError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for RouterCompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RouterCompileError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterEvalError {
    pub code: &'static str,
    pub message: String,
}

impl RouterEvalError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub(crate) fn missing() -> Self {
        Self::new(
            "router_missing_value",
            "missing value cannot be used by this operation",
        )
    }

    pub(crate) fn type_error(message: impl Into<String>) -> Self {
        Self::new("router_type_error", message)
    }

    pub(crate) fn complexity() -> Self {
        Self::new(
            "router_complexity_exceeded",
            "router evaluation fuel exhausted",
        )
    }
}

impl fmt::Display for RouterEvalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for RouterEvalError {}
