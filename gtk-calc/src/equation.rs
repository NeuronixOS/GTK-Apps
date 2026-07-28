//! Equation / display state — rusty counterpart to MathEquation.
//!
//! Tracks the current expression text, undo/redo, last answer, and solve.

use crate::engine::{self, format_number, AngleUnit, CalcError, EvalContext};

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub equation: String,
    pub answer: String,
}

#[derive(Debug)]
pub struct Equation {
    text: String,
    /// Cursor position in characters (not bytes).
    cursor: usize,
    undo_stack: Vec<String>,
    redo_stack: Vec<String>,
    pub ans: f64,
    pub angle: AngleUnit,
    pub precision: u32,
    pub base: u32,
    pub word_size: u32,
    /// After a successful solve, the next digit starts a new entry.
    just_solved: bool,
    pub history: Vec<HistoryEntry>,
    pub status: String,
}

impl Equation {
    pub fn new(angle: AngleUnit, precision: u32, base: u32, word_size: u32) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            ans: 0.0,
            angle,
            precision,
            base,
            word_size,
            just_solved: false,
            history: Vec::new(),
            status: String::new(),
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_text(&mut self, s: impl Into<String>) {
        self.push_undo();
        self.text = s.into();
        self.cursor = self.text.chars().count();
        self.just_solved = false;
        self.status.clear();
    }

    pub fn clear(&mut self) {
        if self.text.is_empty() {
            self.status.clear();
            return;
        }
        self.push_undo();
        self.text.clear();
        self.cursor = 0;
        self.just_solved = false;
        self.status.clear();
    }

    pub fn insert(&mut self, s: &str) {
        if self.just_solved && starts_new_entry(s) {
            self.push_undo();
            self.text.clear();
            self.cursor = 0;
            self.just_solved = false;
        } else if self.just_solved {
            self.just_solved = false;
        }
        self.push_undo();
        let mut chars: Vec<char> = self.text.chars().collect();
        let insert_chars: Vec<char> = s.chars().collect();
        let n = insert_chars.len();
        chars.splice(self.cursor..self.cursor, insert_chars);
        self.text = chars.into_iter().collect();
        self.cursor += n;
        self.status.clear();
    }

    pub fn insert_digit(&mut self, d: u8) {
        self.insert(&d.to_string());
    }

    pub fn insert_function(&mut self, name: &str) {
        // Insert name(  ) with cursor between parens
        if self.just_solved {
            self.push_undo();
            self.text.clear();
            self.cursor = 0;
            self.just_solved = false;
        }
        self.push_undo();
        let snippet = format!("{name}()");
        let mut chars: Vec<char> = self.text.chars().collect();
        let insert_chars: Vec<char> = snippet.chars().collect();
        let open_paren_at = self.cursor + name.chars().count() + 1;
        chars.splice(self.cursor..self.cursor, insert_chars);
        self.text = chars.into_iter().collect();
        self.cursor = open_paren_at;
        self.status.clear();
    }

    pub fn insert_brackets(&mut self) {
        self.insert("()");
        // Move cursor between parentheses
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn square(&mut self) {
        self.insert("^2");
    }

    pub fn solve(&mut self) -> Result<String, CalcError> {
        let expr = self.text.trim();
        if expr.is_empty() {
            return Err(CalcError::Empty);
        }
        let ctx = EvalContext {
            angle: self.angle,
            ans: self.ans,
            word_size: self.word_size,
        };
        let result = engine::solve(expr, &ctx)?;
        let formatted = format_number(result, self.precision, self.base);
        self.history.push(HistoryEntry {
            equation: expr.to_string(),
            answer: formatted.clone(),
        });
        if self.history.len() > 100 {
            self.history.remove(0);
        }
        self.push_undo();
        self.ans = result;
        self.text = formatted.clone();
        self.cursor = self.text.chars().count();
        self.just_solved = true;
        self.status.clear();
        Ok(formatted)
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.text.clone());
            self.text = prev;
            self.cursor = self.text.chars().count();
            self.just_solved = false;
            self.status.clear();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.text.clone());
            self.text = next;
            self.cursor = self.text.chars().count();
            self.just_solved = false;
            self.status.clear();
        }
    }

    pub fn set_error(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(self.text.clone());
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }
}

fn starts_new_entry(s: &str) -> bool {
    let c = s.chars().next().unwrap_or('\0');
    c.is_ascii_digit() || c == '.' || c == '(' || is_function_start(s)
}

fn is_function_start(s: &str) -> bool {
    matches!(
        s,
        "sin"
            | "cos"
            | "tan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "asin"
            | "acos"
            | "atan"
            | "asinh"
            | "acosh"
            | "atanh"
            | "log"
            | "ln"
            | "sqrt"
            | "abs"
            | "floor"
            | "π"
            | "e"
            | "√"
    ) || s.ends_with('(')
}
