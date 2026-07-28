//! Recursive-descent parser with GNOME Calculator-like precedence.

use super::lexer::Token;
use super::CalcError;

#[derive(Debug, Clone)]
pub enum Expr {
    Number(f64),
    Ident(String),
    Unary(UnaryOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Factorial(Box<Expr>),
    Percent(Box<Expr>),
}

#[derive(Debug, Clone, Copy)]
pub enum UnaryOp {
    Neg,
    Pos,
    BitNot,
    Sqrt,
}

#[derive(Debug, Clone, Copy)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    BitNand,
    BitNor,
    BitXnor,
    LShift,
    RShift,
    UrShift,
}

pub fn parse(tokens: &[Token]) -> Result<Expr, CalcError> {
    if tokens.is_empty() {
        return Err(CalcError::Empty);
    }
    let mut p = Parser { tokens, pos: 0 };
    let expr = p.parse_expr()?;
    if p.pos < p.tokens.len() {
        return Err(CalcError::Syntax("Unexpected trailing tokens".into()));
    }
    Ok(expr)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn parse_expr(&mut self) -> Result<Expr, CalcError> {
        self.parse_bit_or()
    }

    // Lowest precedence bitwise OR family
    fn parse_bit_or(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_bit_xor()?;
        loop {
            match self.peek() {
                Some(Token::BitOr) => {
                    self.bump();
                    let right = self.parse_bit_xor()?;
                    left = Expr::Binary(BinOp::BitOr, Box::new(left), Box::new(right));
                }
                Some(Token::BitNor) => {
                    self.bump();
                    let right = self.parse_bit_xor()?;
                    left = Expr::Binary(BinOp::BitNor, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_bit_and()?;
        loop {
            match self.peek() {
                Some(Token::BitXor) => {
                    self.bump();
                    let right = self.parse_bit_and()?;
                    left = Expr::Binary(BinOp::BitXor, Box::new(left), Box::new(right));
                }
                Some(Token::BitXnor) => {
                    self.bump();
                    let right = self.parse_bit_and()?;
                    left = Expr::Binary(BinOp::BitXnor, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_shift()?;
        loop {
            match self.peek() {
                Some(Token::BitAnd) => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Binary(BinOp::BitAnd, Box::new(left), Box::new(right));
                }
                Some(Token::BitNand) => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Binary(BinOp::BitNand, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_add()?;
        loop {
            match self.peek() {
                Some(Token::LShift) => {
                    self.bump();
                    let right = self.parse_add()?;
                    left = Expr::Binary(BinOp::LShift, Box::new(left), Box::new(right));
                }
                Some(Token::RShift) => {
                    self.bump();
                    let right = self.parse_add()?;
                    left = Expr::Binary(BinOp::RShift, Box::new(left), Box::new(right));
                }
                Some(Token::UrShift) => {
                    self.bump();
                    let right = self.parse_add()?;
                    left = Expr::Binary(BinOp::UrShift, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary(BinOp::Add, Box::new(left), Box::new(right));
                }
                Some(Token::Minus) => {
                    self.bump();
                    let right = self.parse_mul()?;
                    left = Expr::Binary(BinOp::Sub, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> Result<Expr, CalcError> {
        let mut left = self.parse_power()?;
        loop {
            match self.peek() {
                Some(Token::Multiply) => {
                    self.bump();
                    let right = self.parse_power()?;
                    left = Expr::Binary(BinOp::Mul, Box::new(left), Box::new(right));
                }
                Some(Token::Divide) => {
                    self.bump();
                    let right = self.parse_power()?;
                    left = Expr::Binary(BinOp::Div, Box::new(left), Box::new(right));
                }
                Some(Token::Mod) => {
                    self.bump();
                    let right = self.parse_power()?;
                    left = Expr::Binary(BinOp::Mod, Box::new(left), Box::new(right));
                }
                // Implicit multiplication: 2π, 2(3+1), )(
                Some(Token::Number(_) | Token::Ident(_) | Token::LParen | Token::Sqrt) => {
                    let right = self.parse_power()?;
                    left = Expr::Binary(BinOp::Mul, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, CalcError> {
        let left = self.parse_postfix()?;
        if matches!(self.peek(), Some(Token::Power)) {
            self.bump();
            // Right-associative
            let right = self.parse_power()?;
            Ok(Expr::Binary(BinOp::Pow, Box::new(left), Box::new(right)))
        } else {
            Ok(left)
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, CalcError> {
        let mut expr = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Factorial) => {
                    self.bump();
                    expr = Expr::Factorial(Box::new(expr));
                }
                Some(Token::Percent) => {
                    self.bump();
                    expr = Expr::Percent(Box::new(expr));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, CalcError> {
        match self.peek() {
            Some(Token::Minus) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Neg, Box::new(e)))
            }
            Some(Token::Plus) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Pos, Box::new(e)))
            }
            Some(Token::BitNot) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::BitNot, Box::new(e)))
            }
            Some(Token::Sqrt) => {
                self.bump();
                let e = self.parse_unary()?;
                Ok(Expr::Unary(UnaryOp::Sqrt, Box::new(e)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, CalcError> {
        match self.bump() {
            Some(Token::Number(n)) => Ok(Expr::Number(*n)),
            Some(Token::Ident(name)) => {
                let name = name.clone();
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.bump();
                    let args = self.parse_args()?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Ident(name))
                }
            }
            Some(Token::LParen) => {
                let e = self.parse_expr()?;
                match self.bump() {
                    Some(Token::RParen) => Ok(e),
                    _ => Err(CalcError::Syntax("Expected ')'".into())),
                }
            }
            Some(other) => Err(CalcError::Syntax(format!("Unexpected token {other:?}"))),
            None => Err(CalcError::Syntax("Unexpected end of expression".into())),
        }
    }

    fn parse_args(&mut self) -> Result<Vec<Expr>, CalcError> {
        let mut args = Vec::new();
        if matches!(self.peek(), Some(Token::RParen)) {
            self.bump();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr()?);
            match self.bump() {
                Some(Token::Comma) => continue,
                Some(Token::RParen) => break,
                _ => return Err(CalcError::Syntax("Expected ',' or ')'".into())),
            }
        }
        Ok(args)
    }
}
