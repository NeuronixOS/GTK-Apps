//! AST evaluation with angle units, constants, and programming bit ops.

use super::parser::{BinOp, Expr, UnaryOp};
use super::{AngleUnit, CalcError};

#[derive(Debug, Clone)]
pub struct EvalContext {
    pub angle: AngleUnit,
    pub ans: f64,
    pub word_size: u32,
}

impl Default for EvalContext {
    fn default() -> Self {
        Self {
            angle: AngleUnit::Degrees,
            ans: 0.0,
            word_size: 64,
        }
    }
}

pub fn evaluate(expr: &Expr, ctx: &EvalContext) -> Result<f64, CalcError> {
    match expr {
        Expr::Number(n) => Ok(*n),
        Expr::Ident(name) => resolve_ident(name, ctx),
        Expr::Unary(op, e) => {
            let v = evaluate(e, ctx)?;
            match op {
                UnaryOp::Neg => Ok(-v),
                UnaryOp::Pos => Ok(v),
                UnaryOp::BitNot => Ok(mask_bits(!to_u64(v, ctx.word_size)?, ctx.word_size) as f64),
                UnaryOp::Sqrt => {
                    if v < 0.0 {
                        return Err(CalcError::Math("Square root of negative number".into()));
                    }
                    Ok(v.sqrt())
                }
            }
        }
        Expr::Binary(op, l, r) => {
            let a = evaluate(l, ctx)?;
            let b = evaluate(r, ctx)?;
            eval_binary(*op, a, b, ctx)
        }
        Expr::Call(name, args) => eval_call(name, args, ctx),
        Expr::Factorial(e) => {
            let v = evaluate(e, ctx)?;
            factorial(v)
        }
        Expr::Percent(e) => {
            let v = evaluate(e, ctx)?;
            Ok(v / 100.0)
        }
    }
}

fn resolve_ident(name: &str, ctx: &EvalContext) -> Result<f64, CalcError> {
    match name {
        "π" | "pi" => Ok(std::f64::consts::PI),
        "τ" | "tau" => Ok(std::f64::consts::TAU),
        "e" => Ok(std::f64::consts::E),
        "φ" | "phi" => Ok((1.0 + 5.0_f64.sqrt()) / 2.0),
        "ans" | "ANS" => Ok(ctx.ans),
        "i" => Err(CalcError::Math(
            "Complex numbers are not supported yet".into(),
        )),
        other => Err(CalcError::Syntax(format!("Unknown identifier '{other}'"))),
    }
}

fn eval_binary(op: BinOp, a: f64, b: f64, ctx: &EvalContext) -> Result<f64, CalcError> {
    match op {
        BinOp::Add => Ok(a + b),
        BinOp::Sub => Ok(a - b),
        BinOp::Mul => Ok(a * b),
        BinOp::Div => {
            if b == 0.0 {
                Err(CalcError::Math("Division by zero".into()))
            } else {
                Ok(a / b)
            }
        }
        BinOp::Pow => Ok(a.powf(b)),
        BinOp::Mod => {
            if b == 0.0 {
                Err(CalcError::Math("Modulo by zero".into()))
            } else {
                Ok(a % b)
            }
        }
        BinOp::BitAnd => Ok((to_u64(a, ctx.word_size)? & to_u64(b, ctx.word_size)?) as f64),
        BinOp::BitOr => Ok((to_u64(a, ctx.word_size)? | to_u64(b, ctx.word_size)?) as f64),
        BinOp::BitXor => Ok((to_u64(a, ctx.word_size)? ^ to_u64(b, ctx.word_size)?) as f64),
        BinOp::BitNand => {
            Ok(mask_bits(!(to_u64(a, ctx.word_size)? & to_u64(b, ctx.word_size)?), ctx.word_size)
                as f64)
        }
        BinOp::BitNor => {
            Ok(mask_bits(!(to_u64(a, ctx.word_size)? | to_u64(b, ctx.word_size)?), ctx.word_size)
                as f64)
        }
        BinOp::BitXnor => {
            Ok(mask_bits(!(to_u64(a, ctx.word_size)? ^ to_u64(b, ctx.word_size)?), ctx.word_size)
                as f64)
        }
        BinOp::LShift => {
            let shift = b as u32;
            if shift >= ctx.word_size {
                Ok(0.0)
            } else {
                Ok(mask_bits(to_u64(a, ctx.word_size)? << shift, ctx.word_size) as f64)
            }
        }
        BinOp::RShift => {
            // Arithmetic right shift (sign-extend)
            let bits = ctx.word_size;
            let shift = b as u32;
            let val = to_i64(a, bits)?;
            if shift >= bits {
                Ok(if val < 0 { -1.0 } else { 0.0 })
            } else {
                Ok(mask_bits((val >> shift) as u64, bits) as f64)
            }
        }
        BinOp::UrShift => {
            let shift = b as u32;
            if shift >= ctx.word_size {
                Ok(0.0)
            } else {
                Ok((to_u64(a, ctx.word_size)? >> shift) as f64)
            }
        }
    }
}

fn eval_call(name: &str, args: &[Expr], ctx: &EvalContext) -> Result<f64, CalcError> {
    let vals: Result<Vec<f64>, _> = args.iter().map(|a| evaluate(a, ctx)).collect();
    let vals = vals?;

    let one = |fname: &str| -> Result<f64, CalcError> {
        if vals.len() != 1 {
            return Err(CalcError::Syntax(format!(
                "{fname}() expects 1 argument"
            )));
        }
        Ok(vals[0])
    };
    let two = |fname: &str| -> Result<(f64, f64), CalcError> {
        if vals.len() != 2 {
            return Err(CalcError::Syntax(format!(
                "{fname}() expects 2 arguments"
            )));
        }
        Ok((vals[0], vals[1]))
    };

    match name {
        "sin" => Ok(ctx.angle.to_radians(one("sin")?).sin()),
        "cos" => Ok(ctx.angle.to_radians(one("cos")?).cos()),
        "tan" => Ok(ctx.angle.to_radians(one("tan")?).tan()),
        "asin" => Ok(ctx.angle.from_radians(one("asin")?.asin())),
        "acos" => Ok(ctx.angle.from_radians(one("acos")?.acos())),
        "atan" => Ok(ctx.angle.from_radians(one("atan")?.atan())),
        "sinh" => Ok(one("sinh")?.sinh()),
        "cosh" => Ok(one("cosh")?.cosh()),
        "tanh" => Ok(one("tanh")?.tanh()),
        "asinh" => Ok(one("asinh")?.asinh()),
        "acosh" => Ok(one("acosh")?.acosh()),
        "atanh" => Ok(one("atanh")?.atanh()),
        "log" | "log10" => {
            let v = one("log")?;
            if v <= 0.0 {
                return Err(CalcError::Math("Logarithm of non-positive number".into()));
            }
            Ok(v.log10())
        }
        "ln" | "logₑ" => {
            let v = one("ln")?;
            if v <= 0.0 {
                return Err(CalcError::Math("Logarithm of non-positive number".into()));
            }
            Ok(v.ln())
        }
        "log2" => {
            let v = one("log2")?;
            if v <= 0.0 {
                return Err(CalcError::Math("Logarithm of non-positive number".into()));
            }
            Ok(v.log2())
        }
        "sqrt" => {
            let v = one("sqrt")?;
            if v < 0.0 {
                return Err(CalcError::Math("Square root of negative number".into()));
            }
            Ok(v.sqrt())
        }
        "cbrt" => Ok(one("cbrt")?.cbrt()),
        "abs" => Ok(one("abs")?.abs()),
        "floor" => Ok(one("floor")?.floor()),
        "ceil" => Ok(one("ceil")?.ceil()),
        "round" => Ok(one("round")?.round()),
        "exp" => Ok(one("exp")?.exp()),
        "inv" | "recip" => {
            let v = one("inv")?;
            if v == 0.0 {
                return Err(CalcError::Math("Division by zero".into()));
            }
            Ok(1.0 / v)
        }
        "fact" | "factorial" => factorial(one("fact")?),
        "pow" => {
            let (a, b) = two("pow")?;
            Ok(a.powf(b))
        }
        "root" | "nroot" => {
            let (n, x) = two("root")?;
            if n == 0.0 {
                return Err(CalcError::Math("0th root is undefined".into()));
            }
            Ok(x.powf(1.0 / n))
        }
        "logn" | "logy" => {
            let (base, x) = two("logn")?;
            if base <= 0.0 || base == 1.0 || x <= 0.0 {
                return Err(CalcError::Math("Invalid logarithm".into()));
            }
            Ok(x.log(base))
        }
        "mod" => {
            let (a, b) = two("mod")?;
            if b == 0.0 {
                return Err(CalcError::Math("Modulo by zero".into()));
            }
            Ok(a % b)
        }
        "gcd" => {
            let (a, b) = two("gcd")?;
            Ok(gcd(a.round().abs() as u64, b.round().abs() as u64) as f64)
        }
        "lcm" => {
            let (a, b) = two("lcm")?;
            let aa = a.round().abs() as u64;
            let bb = b.round().abs() as u64;
            if aa == 0 || bb == 0 {
                return Ok(0.0);
            }
            Ok((aa / gcd(aa, bb) * bb) as f64)
        }
        "ncr" | "C" => {
            let (n, r) = two("ncr")?;
            combinations(n, r)
        }
        "npr" | "P" => {
            let (n, r) = two("npr")?;
            permutations(n, r)
        }
        "twos" => {
            let v = one("twos")?;
            let bits = ctx.word_size;
            Ok(mask_bits((!to_u64(v, bits)?).wrapping_add(1), bits) as f64)
        }
        "bswap" => {
            let v = one("bswap")?;
            Ok(byteswap(to_u64(v, ctx.word_size)?, ctx.word_size) as f64)
        }
        "absint" => Ok(one("abs")?.abs().trunc()),
        other => Err(CalcError::Syntax(format!("Unknown function '{other}'"))),
    }
}

fn factorial(v: f64) -> Result<f64, CalcError> {
    if v < 0.0 || v.fract() != 0.0 {
        return Err(CalcError::Math("Factorial requires a non-negative integer".into()));
    }
    if v > 170.0 {
        return Err(CalcError::Math("Factorial overflow".into()));
    }
    let mut acc = 1.0;
    let mut n = v as u32;
    while n > 1 {
        acc *= n as f64;
        n -= 1;
    }
    Ok(acc)
}

fn combinations(n: f64, r: f64) -> Result<f64, CalcError> {
    if n < 0.0 || r < 0.0 || n.fract() != 0.0 || r.fract() != 0.0 || r > n {
        return Err(CalcError::Math("Invalid combination arguments".into()));
    }
    let n = n as u64;
    let r = r as u64;
    let r = r.min(n - r);
    let mut result = 1u64;
    for i in 0..r {
        result = result * (n - i) / (i + 1);
    }
    Ok(result as f64)
}

fn permutations(n: f64, r: f64) -> Result<f64, CalcError> {
    if n < 0.0 || r < 0.0 || n.fract() != 0.0 || r.fract() != 0.0 || r > n {
        return Err(CalcError::Math("Invalid permutation arguments".into()));
    }
    let n = n as u64;
    let r = r as u64;
    let mut result = 1u64;
    for i in 0..r {
        result = result.saturating_mul(n - i);
    }
    Ok(result as f64)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn word_mask(bits: u32) -> u64 {
    if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    }
}

fn mask_bits(v: u64, bits: u32) -> u64 {
    v & word_mask(bits)
}

fn to_u64(v: f64, bits: u32) -> Result<u64, CalcError> {
    if !v.is_finite() || v.fract().abs() > 1e-9 {
        return Err(CalcError::Math(
            "Bitwise operations require an integer".into(),
        ));
    }
    Ok(mask_bits(v as i64 as u64, bits))
}

fn to_i64(v: f64, bits: u32) -> Result<i64, CalcError> {
    let u = to_u64(v, bits)?;
    if bits >= 64 {
        return Ok(u as i64);
    }
    let sign_bit = 1u64 << (bits - 1);
    if u & sign_bit != 0 {
        // Sign-extend
        Ok((u | !word_mask(bits)) as i64)
    } else {
        Ok(u as i64)
    }
}

fn byteswap(v: u64, bits: u32) -> u64 {
    match bits {
        8 => v,
        16 => ((v & 0xff) << 8) | ((v >> 8) & 0xff),
        32 => {
            let v = v as u32;
            v.swap_bytes() as u64
        }
        _ => v.swap_bytes(),
    }
}
