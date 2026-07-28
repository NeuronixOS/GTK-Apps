//! Number ↔ display string formatting (automatic / fixed / bases).

/// Format a result for the equation display.
pub fn format_number(value: f64, precision: u32, base: u32) -> String {
    if !value.is_finite() {
        if value.is_nan() {
            return "NaN".into();
        }
        return if value.is_sign_positive() {
            "∞".into()
        } else {
            "−∞".into()
        };
    }

    if base != 10 && value.fract().abs() < 1e-9 && value.abs() <= u64::MAX as f64 {
        return format_integer_base(value as i64 as u64, base);
    }

    // Prefer integer display when close enough
    if value.fract().abs() < 1e-12 * value.abs().max(1.0) && value.abs() < 1e15 {
        return format!("{}", value.round() as i64);
    }

    let prec = precision.clamp(1, 17) as usize;
    let abs = value.abs();

    // Scientific for very large / small
    if (abs >= 1e12 || (abs > 0.0 && abs < 1e-6)) && abs != 0.0 {
        let s = format!("{value:.prec$e}");
        return tidy_scientific(&s);
    }

    let s = format!("{value:.prec$}");
    trim_trailing_zeros(&s)
}

fn format_integer_base(v: u64, base: u32) -> String {
    match base {
        2 => format!("0b{v:b}"),
        8 => format!("0o{v:o}"),
        16 => format!("0x{v:X}"),
        _ => v.to_string(),
    }
}

fn trim_trailing_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let mut out = s.trim_end_matches('0').to_string();
    if out.ends_with('.') {
        out.pop();
    }
    if out.is_empty() || out == "-" {
        "0".into()
    } else {
        out
    }
}

fn tidy_scientific(s: &str) -> String {
    // "1.230000e+2" → "1.23e+2"
    if let Some((mant, exp)) = s.split_once('e') {
        let mant = trim_trailing_zeros(mant);
        format!("{mant}e{exp}")
    } else {
        s.to_string()
    }
}

/// Format an integer for the programming bit panel / status.
pub fn format_bits(value: f64, word_size: u32) -> Option<u64> {
    if !value.is_finite() || value.fract().abs() > 1e-9 {
        return None;
    }
    let bits = word_size.min(64);
    let mask = if bits >= 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    Some((value as i64 as u64) & mask)
}
