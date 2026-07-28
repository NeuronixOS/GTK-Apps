//! Expression engine — lexer, recursive-descent parser, and evaluator.
//!
//! Mirrors the essential evaluation path of GNOME Calculator's `lib/`
//! (equation-lexer → equation-parser → Number), using `f64` instead of MPFR.

mod eval;
mod format;
mod lexer;
mod parser;

pub use eval::{evaluate, EvalContext};
pub use format::{format_bits, format_number};
pub use lexer::tokenize;
pub use parser::parse;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CalcMode {
    #[default]
    Basic,
    Advanced,
    Programming,
    Keyboard,
}

impl CalcMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::Advanced => "advanced",
            Self::Programming => "programming",
            Self::Keyboard => "keyboard",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Advanced => "Advanced",
            Self::Programming => "Programming",
            Self::Keyboard => "Keyboard",
        }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AngleUnit {
    #[default]
    Degrees,
    Radians,
    Gradians,
}

impl AngleUnit {
    pub fn label(self) -> &'static str {
        match self {
            Self::Degrees => "Degrees",
            Self::Radians => "Radians",
            Self::Gradians => "Gradians",
        }
    }

    pub fn to_radians(self, x: f64) -> f64 {
        match self {
            Self::Degrees => x.to_radians(),
            Self::Radians => x,
            Self::Gradians => x * std::f64::consts::PI / 200.0,
        }
    }

    pub fn from_radians(self, x: f64) -> f64 {
        match self {
            Self::Degrees => x.to_degrees(),
            Self::Radians => x,
            Self::Gradians => x * 200.0 / std::f64::consts::PI,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalcError {
    Empty,
    Syntax(String),
    Math(String),
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Empty expression"),
            Self::Syntax(m) | Self::Math(m) => write!(f, "{m}"),
        }
    }
}

/// Solve an expression string with the given context.
pub fn solve(expr: &str, ctx: &EvalContext) -> Result<f64, CalcError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(CalcError::Empty);
    }
    let tokens = tokenize(trimmed)?;
    let ast = parse(&tokens)?;
    evaluate(&ast, ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EvalContext {
        EvalContext::default()
    }

    #[test]
    fn basic_arithmetic() {
        assert!((solve("2+3×4", &ctx()).unwrap() - 14.0).abs() < 1e-9);
        assert!((solve("(2+3)×4", &ctx()).unwrap() - 20.0).abs() < 1e-9);
        assert!((solve("10÷4", &ctx()).unwrap() - 2.5).abs() < 1e-9);
        assert!((solve("2^10", &ctx()).unwrap() - 1024.0).abs() < 1e-9);
    }

    #[test]
    fn functions_and_constants() {
        let mut c = ctx();
        c.angle = AngleUnit::Degrees;
        assert!((solve("sin(90)", &c).unwrap() - 1.0).abs() < 1e-9);
        assert!((solve("π", &ctx()).unwrap() - std::f64::consts::PI).abs() < 1e-12);
        assert!((solve("sqrt(16)", &ctx()).unwrap() - 4.0).abs() < 1e-9);
        assert!((solve("√16", &ctx()).unwrap() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn factorial_mod_percent() {
        assert!((solve("5!", &ctx()).unwrap() - 120.0).abs() < 1e-9);
        assert!((solve("10 mod 3", &ctx()).unwrap() - 1.0).abs() < 1e-9);
        assert!((solve("50%", &ctx()).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bitwise() {
        assert!((solve("5 ∧ 3", &ctx()).unwrap() - 1.0).abs() < 1e-9);
        assert!((solve("5 ∨ 3", &ctx()).unwrap() - 7.0).abs() < 1e-9);
        assert!((solve("5 ≪ 2", &ctx()).unwrap() - 20.0).abs() < 1e-9);
    }
}
