use super::{
    RouterCompileError,
    ast::{AccessKey, BinaryOp, Expr, Function, Literal, Root},
    lexer::{Token, lex},
    value::parse_number,
};

const MAX_SOURCE_BYTES: usize = 4 * 1024;
const MAX_AST_NODES: usize = 256;
const MAX_AST_DEPTH: usize = 32;
const MAX_LITERAL_LIST: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledExpression {
    source: String,
    pub(crate) ast: Expr,
}

impl CompiledExpression {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn is_statically_non_boolean(&self) -> bool {
        matches!(static_type(&self.ast), StaticType::NonBoolean)
    }
}

pub fn compile_expression(source: &str) -> Result<CompiledExpression, RouterCompileError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(complexity("expression source exceeds 4 KiB"));
    }
    let tokens = lex(source).map_err(syntax)?;
    let mut parser = Parser {
        tokens,
        position: 0,
        nodes: 0,
        nesting: 0,
    };
    let ast = parser.parse_or()?;
    parser.expect(Token::End)?;
    if depth(&ast) > MAX_AST_DEPTH {
        return Err(complexity("expression nesting depth exceeds 32"));
    }
    Ok(CompiledExpression {
        source: source.into(),
        ast,
    })
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    nodes: usize,
    nesting: usize,
}

impl Parser {
    fn parse_or(&mut self) -> Result<Expr, RouterCompileError> {
        let mut expression = self.parse_and()?;
        while self.take(&Token::Or) {
            let right = self.parse_and()?;
            expression = self.node(Expr::Binary(
                BinaryOp::Or,
                Box::new(expression),
                Box::new(right),
            ))?;
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<Expr, RouterCompileError> {
        let mut expression = self.parse_comparison()?;
        while self.take(&Token::And) {
            let right = self.parse_comparison()?;
            expression = self.node(Expr::Binary(
                BinaryOp::And,
                Box::new(expression),
                Box::new(right),
            ))?;
        }
        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<Expr, RouterCompileError> {
        let left = self.parse_not()?;
        let operator = match self.current() {
            Token::Equal => BinaryOp::Equal,
            Token::NotEqual => BinaryOp::NotEqual,
            Token::Less => BinaryOp::Less,
            Token::LessEqual => BinaryOp::LessEqual,
            Token::Greater => BinaryOp::Greater,
            Token::GreaterEqual => BinaryOp::GreaterEqual,
            Token::In => BinaryOp::In,
            _ => return Ok(left),
        };
        self.position += 1;
        let right = self.parse_not()?;
        self.node(Expr::Binary(operator, Box::new(left), Box::new(right)))
    }

    fn parse_not(&mut self) -> Result<Expr, RouterCompileError> {
        let mut count = 0;
        while self.take(&Token::Bang) {
            count += 1;
            if count > MAX_AST_DEPTH {
                return Err(complexity("expression nesting depth exceeds 32"));
            }
        }
        let mut expression = self.parse_postfix()?;
        for _ in 0..count {
            expression = self.node(Expr::Not(Box::new(expression)))?;
        }
        Ok(expression)
    }

    fn parse_postfix(&mut self) -> Result<Expr, RouterCompileError> {
        let mut expression = self.parse_primary()?;
        loop {
            if self.take(&Token::Dot) {
                let Token::Identifier(field) = self.advance().clone() else {
                    return Err(syntax("field name expected after '.'"));
                };
                expression = self.node(Expr::Field(Box::new(expression), field))?;
            } else if self.take(&Token::LeftBracket) {
                let key = match self.advance().clone() {
                    Token::String(value) => AccessKey::Field(value),
                    Token::Number(value)
                        if !value.contains(['.', 'e', 'E']) && value.parse::<i64>().is_ok() =>
                    {
                        AccessKey::Index(value.parse().expect("checked integer"))
                    }
                    _ => return Err(syntax("index must be a string or signed 64-bit integer")),
                };
                self.expect(Token::RightBracket)?;
                expression = self.node(Expr::Index(Box::new(expression), key))?;
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<Expr, RouterCompileError> {
        match self.advance().clone() {
            Token::Null => self.node(Expr::Literal(Literal::Null)),
            Token::Bool(value) => self.node(Expr::Literal(Literal::Bool(value))),
            Token::String(value) => self.node(Expr::Literal(Literal::String(value))),
            Token::Number(value) => {
                parse_number(&value)
                    .map_err(|error| RouterCompileError::new(error.code, error.message))?;
                self.node(Expr::Literal(Literal::Number(value)))
            }
            Token::Identifier(value) => self.parse_identifier(value),
            Token::LeftParen => {
                let expression = self.parse_nested_or()?;
                self.expect(Token::RightParen)?;
                Ok(expression)
            }
            Token::LeftBracket => self.parse_list(),
            token => Err(syntax(format!("unexpected token: {token:?}"))),
        }
    }

    fn parse_identifier(&mut self, name: String) -> Result<Expr, RouterCompileError> {
        let root = match name.as_str() {
            "inputs" => Some(Root::Inputs),
            "memory" => Some(Root::Memory),
            "control" => Some(Root::Control),
            _ => None,
        };
        if let Some(root) = root {
            return self.node(Expr::Root(root));
        }
        let function = Function::parse(&name)
            .ok_or_else(|| syntax(format!("unknown identifier or function: {name}")))?;
        self.expect(Token::LeftParen)?;
        let mut arguments = Vec::new();
        if !self.take(&Token::RightParen) {
            loop {
                arguments.push(self.parse_nested_or()?);
                if self.take(&Token::RightParen) {
                    break;
                }
                self.expect(Token::Comma)?;
            }
        }
        if arguments.len() != function.arity() {
            return Err(syntax(format!(
                "function {name} expects {} arguments",
                function.arity()
            )));
        }
        self.node(Expr::Call(function, arguments))
    }

    fn parse_list(&mut self) -> Result<Expr, RouterCompileError> {
        let mut values = Vec::new();
        if !self.take(&Token::RightBracket) {
            loop {
                let value = self.parse_nested_or()?;
                if !is_literal(&value) {
                    return Err(syntax("list literals may only contain literal values"));
                }
                values.push(value);
                if values.len() > MAX_LITERAL_LIST {
                    return Err(complexity("literal list exceeds 128 values"));
                }
                if self.take(&Token::RightBracket) {
                    break;
                }
                self.expect(Token::Comma)?;
            }
        }
        self.node(Expr::List(values))
    }

    fn node(&mut self, expression: Expr) -> Result<Expr, RouterCompileError> {
        self.nodes += 1;
        if self.nodes > MAX_AST_NODES {
            return Err(complexity("expression AST exceeds 256 nodes"));
        }
        Ok(expression)
    }

    fn parse_nested_or(&mut self) -> Result<Expr, RouterCompileError> {
        self.nesting += 1;
        if self.nesting > MAX_AST_DEPTH {
            self.nesting -= 1;
            return Err(complexity("expression nesting depth exceeds 32"));
        }
        let result = self.parse_or();
        self.nesting -= 1;
        result
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn advance(&mut self) -> &Token {
        let position = self.position;
        self.position += 1;
        &self.tokens[position]
    }

    fn take(&mut self, expected: &Token) -> bool {
        if self.current() == expected {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), RouterCompileError> {
        if self.take(&expected) {
            Ok(())
        } else {
            Err(syntax(format!(
                "expected {expected:?}, found {:?}",
                self.current()
            )))
        }
    }
}

fn is_literal(expression: &Expr) -> bool {
    matches!(expression, Expr::Literal(_))
        || matches!(expression, Expr::List(values) if values.iter().all(is_literal))
}

fn depth(expression: &Expr) -> usize {
    let child = match expression {
        Expr::Literal(_) | Expr::Root(_) => 0,
        Expr::List(values) | Expr::Call(_, values) => values.iter().map(depth).max().unwrap_or(0),
        Expr::Field(value, _) | Expr::Index(value, _) | Expr::Not(value) => depth(value),
        Expr::Binary(_, left, right) => depth(left).max(depth(right)),
    };
    child + 1
}

#[derive(Clone, Copy)]
enum StaticType {
    Boolean,
    NonBoolean,
    Unknown,
}

fn static_type(expression: &Expr) -> StaticType {
    match expression {
        Expr::Literal(Literal::Bool(_)) => StaticType::Boolean,
        Expr::Literal(_) | Expr::List(_) => StaticType::NonBoolean,
        Expr::Root(_) | Expr::Field(_, _) | Expr::Index(_, _) => StaticType::Unknown,
        Expr::Not(_) | Expr::Binary(_, _, _) => StaticType::Boolean,
        Expr::Call(function, _) => match function {
            Function::Has | Function::Contains | Function::StartsWith | Function::EndsWith => {
                StaticType::Boolean
            }
            Function::Size | Function::LowerAscii | Function::UpperAscii => StaticType::NonBoolean,
        },
    }
}

fn syntax(message: impl Into<String>) -> RouterCompileError {
    RouterCompileError::new("router_invalid_expression", message)
}

fn complexity(message: impl Into<String>) -> RouterCompileError {
    RouterCompileError::new("router_complexity_exceeded", message)
}
