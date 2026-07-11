#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Root(Root),
    List(Vec<Expr>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, AccessKey),
    Not(Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Call(Function, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Null,
    Bool(bool),
    String(String),
    Number(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Inputs,
    Memory,
    Control,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessKey {
    Field(String),
    Index(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    Has,
    Size,
    Contains,
    StartsWith,
    EndsWith,
    LowerAscii,
    UpperAscii,
}

impl Function {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "has" => Some(Self::Has),
            "size" => Some(Self::Size),
            "contains" => Some(Self::Contains),
            "starts_with" => Some(Self::StartsWith),
            "ends_with" => Some(Self::EndsWith),
            "lower_ascii" => Some(Self::LowerAscii),
            "upper_ascii" => Some(Self::UpperAscii),
            _ => None,
        }
    }

    pub fn arity(self) -> usize {
        match self {
            Self::Has | Self::Contains | Self::StartsWith | Self::EndsWith => 2,
            Self::Size | Self::LowerAscii | Self::UpperAscii => 1,
        }
    }
}
