//! Simple expression evaluator for the `Eval` action.
//!
//! # Grammar
//! ```text
//! expr         = or_expr
//! or_expr      = and_expr ('||' and_expr)*
//! and_expr     = compare_expr ('&&' compare_expr)*
//! compare_expr = add_expr (('==' | '<' | '<=' | '>' | '>=') add_expr)?
//! add_expr     = mul_expr (('+' | '-') mul_expr)*
//! mul_expr     = unary   (('*' | '/' | '%') unary)*
//! unary    = '-' unary | primary
//! primary  = '(' expr ')' | string_lit | number_lit | func_call | var_ref | bare_ident
//! var_ref  = ('output' | 'param' | 'local') '.' ident
//! bare_ident = ident          (locals-first lookup, falls back to output buffer)
//! func_call = ident '(' (expr (',' expr)*)? ')'
//! string_lit = '\'' [^']* '\''
//! number_lit = [0-9]+ ('.' [0-9]+)?
//! ident    = [a-zA-Z_][a-zA-Z0-9_]*
//! ```
//!
//! # Operator precedence (high → low)
//! 1. Grouping `()`, literals, function calls, variable references
//! 2. Unary `-`
//! 3. `*` `/` `%`
//! 4. `+` `-`
//! 5. `==` `<` `<=` `>` `>=`  → always return `Bool`
//! 6. `&&`  — both operands must be `Bool`
//! 7. `||`  — both operands must be `Bool`
//!
//! # Type coercion
//! - `+` — if both sides parse as `f64`, returns `Number`; otherwise string concat → `String`
//! - `-`, `*`, `/`, unary `-` — both sides must be numeric or an error is returned
//!
//! # Variable namespaces
//! - `output.key`             — last value for `key` in the output buffer, or `""`
//! - `param.key`              — value for `key` in workflow params (immutable), or `""`
//! - `local.key` / bare ident — locals first, falls back to output buffer (last value), or `""`

use std::collections::HashMap;

use crate::output::Output;

// ── Public value type ──────────────────────────────────────────────────────────

/// A runtime value produced during expression evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(std::string::String),
    Number(f64),
    Bool(bool),
}

impl Value {
    /// Try to interpret this value as an `f64`.
    /// For `Number`, returns the inner value.
    /// For `String`, attempts `str::parse::<f64>()`.
    /// `Bool` is not coerced to avoid surprises.
    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::String(s) => s.parse().ok(),
            Value::Bool(_) => None,
        }
    }

    /// Convert to the canonical string representation.
    /// Integer-valued floats are formatted without a decimal point (`4`, not `4.0`).
    pub fn into_string(self) -> std::string::String {
        match self {
            Value::String(s) => s,
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    format!("{}", n as i64)
                } else {
                    format!("{n}")
                }
            }
            Value::Bool(b) => b.to_string(),
        }
    }

    /// Assert this value is `Bool` and return it, or return an error.
    pub fn into_bool(self) -> Result<bool, std::string::String> {
        match self {
            Value::Bool(b) => Ok(b),
            other => Err(format!(
                "expected a boolean expression, got `{}`",
                other.into_string()
            )),
        }
    }
}

// ── Tokenizer ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Str(std::string::String),
    Ident(std::string::String),
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    Comma,
    EqEq,
    BangEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    AmpAmp,
    PipePipe,
}

fn tokenize(input: &str) -> Result<Vec<Token>, std::string::String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\r' | '\n' => {
                i += 1;
            }
            '\'' => {
                i += 1; // skip opening quote
                let mut s = std::string::String::new();
                loop {
                    if i >= chars.len() {
                        return Err("unterminated string literal (missing closing `'`)".into());
                    }
                    match chars[i] {
                        '\'' => {
                            i += 1;
                            break;
                        } // closing quote
                        '\\' => {
                            i += 1;
                            if i >= chars.len() {
                                return Err("unterminated escape sequence in string literal".into());
                            }
                            match chars[i] {
                                '\'' => s.push('\''),
                                '\\' => s.push('\\'),
                                'n' => s.push('\n'),
                                'r' => s.push('\r'),
                                't' => s.push('\t'),
                                c => {
                                    return Err(format!(
                                        "unknown escape sequence `\\{c}` in string literal"
                                    ));
                                }
                            }
                            i += 1;
                        }
                        c => {
                            s.push(c);
                            i += 1;
                        }
                    }
                }
                tokens.push(Token::Str(s));
            }
            '+' => {
                tokens.push(Token::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                i += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(Token::Percent);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '=' => {
                i += 1;
                if chars.get(i) == Some(&'=') {
                    tokens.push(Token::EqEq);
                    i += 1;
                } else {
                    return Err("unexpected `=` — did you mean `==`?".into());
                }
            }
            '!' => {
                i += 1;
                if chars.get(i) == Some(&'=') {
                    tokens.push(Token::BangEq);
                    i += 1;
                } else {
                    return Err("unexpected `!` — did you mean `!=`?".into());
                }
            }
            '<' => {
                i += 1;
                if chars.get(i) == Some(&'=') {
                    tokens.push(Token::LtEq);
                    i += 1;
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '>' => {
                i += 1;
                if chars.get(i) == Some(&'=') {
                    tokens.push(Token::GtEq);
                    i += 1;
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '&' => {
                i += 1;
                if chars.get(i) == Some(&'&') {
                    tokens.push(Token::AmpAmp);
                    i += 1;
                } else {
                    return Err("unexpected `&` — did you mean `&&`?".into());
                }
            }
            '|' => {
                i += 1;
                if chars.get(i) == Some(&'|') {
                    tokens.push(Token::PipePipe);
                    i += 1;
                } else {
                    return Err("unexpected `|` — did you mean `||`?".into());
                }
            }
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '.' {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let s: std::string::String = chars[start..i].iter().collect();
                let n = s
                    .parse::<f64>()
                    .map_err(|e| format!("invalid number `{s}`: {e}"))?;
                tokens.push(Token::Number(n));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let s: std::string::String = chars[start..i].iter().collect();
                tokens.push(Token::Ident(s));
            }
            c => return Err(format!("unexpected character `{c}`")),
        }
    }

    Ok(tokens)
}

// ── AST ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Expr {
    Num(f64),
    Str(std::string::String),
    /// `output.key` or `param.key`
    Var {
        ns: std::string::String,
        key: std::string::String,
    },
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Neg(Box<Expr>),
    Call {
        name: std::string::String,
        args: Vec<Expr>,
    },
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

// ── Parser ────────────────────────────────────────────────────────────────────

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn expect_eof(&self) -> Result<(), std::string::String> {
        if self.pos < self.tokens.len() {
            Err(format!(
                "unexpected token after expression: `{:?}`",
                self.tokens[self.pos]
            ))
        } else {
            Ok(())
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, std::string::String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, std::string::String> {
        let mut lhs = self.parse_and()?;
        while let Some(Token::PipePipe) = self.peek() {
            self.advance();
            let rhs = self.parse_and()?;
            lhs = Expr::BinOp {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, std::string::String> {
        let mut lhs = self.parse_compare()?;
        while let Some(Token::AmpAmp) = self.peek() {
            self.advance();
            let rhs = self.parse_compare()?;
            lhs = Expr::BinOp {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn parse_compare(&mut self) -> Result<Expr, std::string::String> {
        let lhs = self.parse_add()?;
        let op = match self.peek() {
            Some(Token::EqEq) => BinOp::Eq,
            Some(Token::BangEq) => BinOp::Ne,
            Some(Token::Lt) => BinOp::Lt,
            Some(Token::LtEq) => BinOp::LtEq,
            Some(Token::Gt) => BinOp::Gt,
            Some(Token::GtEq) => BinOp::GtEq,
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.parse_add()?;
        Ok(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn parse_add(&mut self) -> Result<Expr, std::string::String> {
        let mut lhs = self.parse_mul()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    let rhs = self.parse_mul()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Add,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Minus) => {
                    self.advance();
                    let rhs = self.parse_mul()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Sub,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, std::string::String> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.advance();
                    let rhs = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Mul,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Slash) => {
                    self.advance();
                    let rhs = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Div,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                Some(Token::Percent) => {
                    self.advance();
                    let rhs = self.parse_unary()?;
                    lhs = Expr::BinOp {
                        op: BinOp::Mod,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    };
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, std::string::String> {
        if let Some(Token::Minus) = self.peek() {
            self.advance();
            let inner = self.parse_unary()?;
            Ok(Expr::Neg(Box::new(inner)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, std::string::String> {
        match self.peek().cloned() {
            Some(Token::LParen) => {
                self.advance();
                let e = self.parse_expr()?;
                match self.advance() {
                    Some(Token::RParen) => Ok(e),
                    _ => Err("expected `)` to close grouping".into()),
                }
            }
            Some(Token::Number(n)) => {
                self.advance();
                Ok(Expr::Num(n))
            }
            Some(Token::Str(s)) => {
                self.advance();
                Ok(Expr::Str(s))
            }
            Some(Token::Ident(name)) => {
                self.advance();
                if name == "output" || name == "param" || name == "local" {
                    // explicit namespace: `output.key`, `param.key`, or `local.key`
                    match self.peek() {
                        Some(Token::Dot) => {
                            self.advance(); // consume dot
                            match self.advance() {
                                Some(Token::Ident(key)) => Ok(Expr::Var {
                                    ns: name,
                                    key: key.clone(),
                                }),
                                _ => Err(format!("expected identifier after `{name}.`")),
                            }
                        }
                        _ => Err(format!("`{name}` must be followed by `.key`")),
                    }
                } else if let Some(Token::LParen) = self.peek() {
                    // function call
                    self.advance(); // consume `(`
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        args.push(self.parse_expr()?);
                        while let Some(Token::Comma) = self.peek() {
                            self.advance();
                            args.push(self.parse_expr()?);
                        }
                    }
                    match self.advance() {
                        Some(Token::RParen) => Ok(Expr::Call { name, args }),
                        _ => Err("expected `)` to close function call".into()),
                    }
                } else {
                    // bare identifier → implicit local lookup
                    Ok(Expr::Var {
                        ns: "local".into(),
                        key: name,
                    })
                }
            }
            Some(t) => Err(format!("unexpected token `{t:?}` in expression")),
            None => Err("unexpected end of expression".into()),
        }
    }
}

// ── Evaluator ─────────────────────────────────────────────────────────────────

/// Validate expression syntax by parsing without evaluating.
/// Catches tokenisation errors, mismatched parentheses, and malformed expressions.
/// Does not evaluate the expression, so variable references are never resolved.
pub fn check_expr_syntax(expr: &str) -> Result<(), std::string::String> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser::new(&tokens);
    parser.parse_expr()?;
    parser.expect_eof()
}

/// Evaluate an expression string against runtime state.
///
/// Returns the result as a [`Value`], or an error string on failure.
/// Like [`eval_expr`] but requires the result to be `Bool`.
/// Returns an error if the expression evaluates to a `String` or `Number`.
pub fn eval_bool_expr(
    expr: &str,
    locals: &HashMap<std::string::String, std::string::String>,
    params: &HashMap<std::string::String, std::string::String>,
    output: &Output,
) -> Result<bool, std::string::String> {
    eval_expr(expr, locals, params, output)?.into_bool()
}

pub fn eval_expr(
    expr: &str,
    locals: &HashMap<std::string::String, std::string::String>,
    params: &HashMap<std::string::String, std::string::String>,
    output: &Output,
) -> Result<Value, std::string::String> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser::new(&tokens);
    let ast = parser.parse_expr()?;
    parser.expect_eof()?;
    eval_node(&ast, locals, params, output)
}

fn eval_node(
    node: &Expr,
    locals: &HashMap<std::string::String, std::string::String>,
    params: &HashMap<std::string::String, std::string::String>,
    output: &Output,
) -> Result<Value, std::string::String> {
    match node {
        Expr::Num(n) => Ok(Value::Number(*n)),
        Expr::Str(s) => Ok(Value::String(s.clone())),

        Expr::Var { ns, key } => {
            let s = match ns.as_str() {
                "output" => output.get(key).last().cloned().unwrap_or_default(),
                "param" => params.get(key).cloned().unwrap_or_default(),
                _ => {
                    // "local" or bare ident: locals first, fall back to output buffer
                    locals
                        .get(key)
                        .cloned()
                        .or_else(|| output.get(key).last().cloned())
                        .unwrap_or_default()
                }
            };
            Ok(Value::String(s))
        }

        Expr::Neg(inner) => {
            let v = eval_node(inner, locals, params, output)?;
            let n = v.as_f64().ok_or_else(|| {
                format!(
                    "unary `-` requires a number, got `{}`",
                    v.clone().into_string()
                )
            })?;
            Ok(Value::Number(-n))
        }

        Expr::BinOp { op, lhs, rhs } => {
            let lv = eval_node(lhs, locals, params, output)?;
            let rv = eval_node(rhs, locals, params, output)?;
            match op {
                BinOp::Add => {
                    // Try numeric first; fall back to string concat
                    match (lv.as_f64(), rv.as_f64()) {
                        (Some(a), Some(b)) => Ok(Value::Number(a + b)),
                        _ => Ok(Value::String(lv.into_string() + &rv.into_string())),
                    }
                }
                BinOp::Sub => {
                    let a = require_num(lv, "-")?;
                    let b = require_num(rv, "-")?;
                    Ok(Value::Number(a - b))
                }
                BinOp::Mul => {
                    let a = require_num(lv, "*")?;
                    let b = require_num(rv, "*")?;
                    Ok(Value::Number(a * b))
                }
                BinOp::Div => {
                    let a = require_num(lv, "/")?;
                    let b = require_num(rv, "/")?;
                    if b == 0.0 {
                        return Err("division by zero".into());
                    }
                    Ok(Value::Number(a / b))
                }
                BinOp::Mod => {
                    let a = require_num(lv, "%")? as i64;
                    let b = require_num(rv, "%")? as i64;
                    if b == 0 {
                        return Err("modulo by zero".into());
                    }
                    Ok(Value::Number((a % b) as f64))
                }
                BinOp::Eq => {
                    let result = match (lv.as_f64(), rv.as_f64()) {
                        (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
                        _ => lv.into_string() == rv.into_string(),
                    };
                    Ok(Value::Bool(result))
                }
                BinOp::Ne => {
                    let result = match (lv.as_f64(), rv.as_f64()) {
                        (Some(a), Some(b)) => (a - b).abs() >= f64::EPSILON,
                        _ => lv.into_string() != rv.into_string(),
                    };
                    Ok(Value::Bool(result))
                }
                BinOp::Lt => Ok(Value::Bool(require_num(lv, "<")? < require_num(rv, "<")?)),
                BinOp::LtEq => Ok(Value::Bool(
                    require_num(lv, "<=")? <= require_num(rv, "<=")?,
                )),
                BinOp::Gt => Ok(Value::Bool(require_num(lv, ">")? > require_num(rv, ">")?)),
                BinOp::GtEq => Ok(Value::Bool(
                    require_num(lv, ">=")? >= require_num(rv, ">=")?,
                )),
                BinOp::And => {
                    let a = lv.into_bool().map_err(|e| format!("`&&` left: {e}"))?;
                    let b = rv.into_bool().map_err(|e| format!("`&&` right: {e}"))?;
                    Ok(Value::Bool(a && b))
                }
                BinOp::Or => {
                    let a = lv.into_bool().map_err(|e| format!("`||` left: {e}"))?;
                    let b = rv.into_bool().map_err(|e| format!("`||` right: {e}"))?;
                    Ok(Value::Bool(a || b))
                }
            }
        }

        Expr::Call { name, args } => eval_call(name, args, locals, params, output),
    }
}

fn require_num(v: Value, op: &str) -> Result<f64, std::string::String> {
    v.as_f64().ok_or_else(|| {
        format!(
            "operator `{op}` requires a number, got `{}`",
            v.into_string()
        )
    })
}

fn eval_call(
    name: &str,
    args: &[Expr],
    locals: &HashMap<std::string::String, std::string::String>,
    params: &HashMap<std::string::String, std::string::String>,
    output: &Output,
) -> Result<Value, std::string::String> {
    let eval = |e: &Expr| eval_node(e, locals, params, output);

    match name {
        "split_lines" => {
            if args.len() != 2 {
                return Err(format!(
                    "split_lines() takes 2 arguments, got {}",
                    args.len()
                ));
            }
            let text = eval(&args[0])?.into_string();
            let idx_val = eval(&args[1])?;
            let idx = idx_val
                .as_f64()
                .ok_or("split_lines() second argument must be a number")?
                as i64;
            let lines: Vec<&str> = text.lines().collect();
            if lines.is_empty() {
                return Ok(Value::String(std::string::String::new()));
            }
            let i = if idx < 0 {
                (lines.len() as i64 + idx).max(0) as usize
            } else {
                idx as usize
            };
            let result = lines.get(i).copied().unwrap_or("").to_owned();
            Ok(Value::String(result))
        }

        "round" => {
            if args.len() != 1 {
                return Err(format!("round() takes 1 argument, got {}", args.len()));
            }
            let n = require_num(eval(&args[0])?, "round()")?;
            Ok(Value::Number(n.round()))
        }

        "floor" => {
            if args.len() != 1 {
                return Err(format!("floor() takes 1 argument, got {}", args.len()));
            }
            let n = require_num(eval(&args[0])?, "floor()")?;
            Ok(Value::Number(n.floor()))
        }

        "ceil" => {
            if args.len() != 1 {
                return Err(format!("ceil() takes 1 argument, got {}", args.len()));
            }
            let n = require_num(eval(&args[0])?, "ceil()")?;
            Ok(Value::Number(n.ceil()))
        }

        "min" => {
            if args.len() != 2 {
                return Err(format!("min() takes 2 arguments, got {}", args.len()));
            }
            let a = require_num(eval(&args[0])?, "min()")?;
            let b = require_num(eval(&args[1])?, "min()")?;
            Ok(Value::Number(a.min(b)))
        }

        "max" => {
            if args.len() != 2 {
                return Err(format!("max() takes 2 arguments, got {}", args.len()));
            }
            let a = require_num(eval(&args[0])?, "max()")?;
            let b = require_num(eval(&args[1])?, "max()")?;
            Ok(Value::Number(a.max(b)))
        }

        "trim" => {
            if args.len() != 1 {
                return Err(format!("trim() takes 1 argument, got {}", args.len()));
            }
            let s = eval(&args[0])?.into_string();
            Ok(Value::String(s.trim().to_owned()))
        }

        "strlen" => {
            if args.len() != 1 {
                return Err(format!("strlen() takes 1 argument, got {}", args.len()));
            }
            let s = eval(&args[0])?.into_string();
            Ok(Value::Number(s.len() as f64))
        }

        // output_count('key') — number of items stored under a key in the output buffer.
        // Useful for checking that a `multiple: true` Extract captured at least one result.
        "output_count" => {
            if args.len() != 1 {
                return Err(format!(
                    "output_count() takes 1 argument, got {}",
                    args.len()
                ));
            }
            let key = eval(&args[0])?.into_string();
            Ok(Value::Number(output.get(&key).len() as f64))
        }

        "dirname" => {
            if args.len() != 1 {
                return Err(format!("dirname() takes 1 argument, got {}", args.len()));
            }
            let p = eval(&args[0])?.into_string();
            let result = std::path::Path::new(&p)
                .parent()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(Value::String(result))
        }

        "basename" => {
            if args.len() != 1 {
                return Err(format!("basename() takes 1 argument, got {}", args.len()));
            }
            let p = eval(&args[0])?.into_string();
            let result = std::path::Path::new(&p)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(Value::String(result))
        }

        "path_join" => {
            if args.len() != 2 {
                return Err(format!("path_join() takes 2 arguments, got {}", args.len()));
            }
            let a = eval(&args[0])?.into_string();
            let b = eval(&args[1])?.into_string();
            let result = std::path::Path::new(&a)
                .join(&b)
                .to_string_lossy()
                .into_owned();
            Ok(Value::String(result))
        }

        // regex_match(str, pattern) — true if the pattern matches anywhere in str.
        "regex_match" => {
            if args.len() != 2 {
                return Err(format!(
                    "regex_match() takes 2 arguments, got {}",
                    args.len()
                ));
            }
            let s = eval(&args[0])?.into_string();
            let pattern = eval(&args[1])?.into_string();
            let re = fancy_regex::Regex::new(&pattern)
                .map_err(|e| format!("regex_match(): invalid pattern {pattern:?}: {e}"))?;
            Ok(Value::Bool(re.is_match(&s).unwrap_or(false)))
        }

        // regex_extract(str, pattern) — returns the first capture group if present,
        // otherwise the full match. Returns empty string if there is no match.
        "regex_extract" => {
            if args.len() != 2 {
                return Err(format!(
                    "regex_extract() takes 2 arguments, got {}",
                    args.len()
                ));
            }
            let s = eval(&args[0])?.into_string();
            let pattern = eval(&args[1])?.into_string();
            let re = fancy_regex::Regex::new(&pattern)
                .map_err(|e| format!("regex_extract(): invalid pattern {pattern:?}: {e}"))?;
            let result = re
                .captures(&s)
                .unwrap_or(None)
                .map(|caps| {
                    caps.get(1)
                        .or_else(|| caps.get(0))
                        .map(|m| m.as_str().to_owned())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            Ok(Value::String(result))
        }

        _ => Err(format!(
            "unknown function `{name}` (available: split_lines, round, floor, ceil, min, max, trim, strlen, output_count, dirname, basename, path_join, regex_match, regex_extract)"
        )),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_output() -> Output {
        Output::default()
    }

    fn eval(expr: &str) -> Result<Value, std::string::String> {
        eval_expr(expr, &HashMap::new(), &HashMap::new(), &empty_output())
    }

    fn evals(expr: &str) -> std::string::String {
        eval(expr).unwrap().into_string()
    }

    #[test]
    fn literal_string() {
        assert_eq!(evals("'hello'"), "hello");
    }

    #[test]
    fn literal_number() {
        assert_eq!(evals("42"), "42");
        assert_eq!(evals("3.14"), "3.14");
    }

    #[test]
    fn numeric_add() {
        assert_eq!(evals("1.5 + 2.5"), "4");
    }

    #[test]
    fn string_concat_when_not_both_numeric() {
        assert_eq!(evals("'a' + 'b'"), "ab");
        assert_eq!(evals("'hello' + ' world'"), "hello world");
    }

    #[test]
    fn numeric_string_add() {
        // both parse as number → numeric add
        assert_eq!(evals("'3' + '4'"), "7");
        assert_eq!(evals("'3' + 4"), "7");
    }

    #[test]
    fn arithmetic_precedence() {
        assert_eq!(evals("1 + 2 * 3"), "7");
        assert_eq!(evals("(1 + 2) * 3"), "9");
    }

    #[test]
    fn subtraction() {
        assert_eq!(evals("10 - 3"), "7");
    }

    #[test]
    fn division() {
        assert_eq!(evals("10 / 4"), "2.5");
    }

    #[test]
    fn unary_negation() {
        assert_eq!(evals("-5"), "-5");
        assert_eq!(evals("-(3 + 2)"), "-5");
    }

    #[test]
    fn output_variable() {
        let mut out = Output::default();
        out.push("x", "3");
        let result = eval_expr("output.x + 1", &HashMap::new(), &HashMap::new(), &out).unwrap();
        assert_eq!(result.into_string(), "4");
    }

    #[test]
    fn bare_identifier_is_implicit_local() {
        let mut locals = HashMap::new();
        locals.insert("score".into(), "10".into());
        let result = eval_expr("score + 5", &locals, &HashMap::new(), &empty_output()).unwrap();
        assert_eq!(result.into_string(), "15");
    }

    #[test]
    fn bare_identifier_in_function_arg() {
        let mut locals = HashMap::new();
        locals.insert("lines".into(), "a\nb\nc".into());
        let result = eval_expr(
            "split_lines(lines, -1)",
            &locals,
            &HashMap::new(),
            &empty_output(),
        )
        .unwrap();
        assert_eq!(result.into_string(), "c");
    }

    #[test]
    fn local_variable_via_local_ns() {
        let mut locals = HashMap::new();
        locals.insert("n".into(), "15".into());
        let result = eval_expr(
            "min(local.n, 10)",
            &locals,
            &HashMap::new(),
            &empty_output(),
        )
        .unwrap();
        assert_eq!(result.into_string(), "10");
    }

    #[test]
    fn param_variable_from_params() {
        let mut params = HashMap::new();
        params.insert("n".into(), "15".into());
        let result = eval_expr(
            "min(param.n, 10)",
            &HashMap::new(),
            &params,
            &empty_output(),
        )
        .unwrap();
        assert_eq!(result.into_string(), "10");
    }

    #[test]
    fn missing_variable_is_empty_string() {
        // empty string + 0 → cannot both parse as number → string concat
        assert_eq!(evals("output.missing + 'x'"), "x");
    }

    #[test]
    fn split_lines_last() {
        let mut out = Output::default();
        out.push("v", "a\r\nb\r\nc");
        let result = eval_expr(
            "split_lines(output.v, -1)",
            &HashMap::new(),
            &HashMap::new(),
            &out,
        )
        .unwrap();
        assert_eq!(result.into_string(), "c");
    }

    #[test]
    fn split_lines_index() {
        let mut out = Output::default();
        out.push("v", "line0\nline1\nline2");
        let result = eval_expr(
            "split_lines(output.v, 1)",
            &HashMap::new(),
            &HashMap::new(),
            &out,
        )
        .unwrap();
        assert_eq!(result.into_string(), "line1");
    }

    #[test]
    fn round_fn() {
        assert_eq!(evals("round(3.7)"), "4");
        assert_eq!(evals("round(3.2)"), "3");
    }

    #[test]
    fn floor_ceil_fn() {
        assert_eq!(evals("floor(3.9)"), "3");
        assert_eq!(evals("ceil(3.1)"), "4");
    }

    #[test]
    fn min_max_fn() {
        assert_eq!(evals("min(5, 3)"), "3");
        assert_eq!(evals("max(5, 3)"), "5");
    }

    #[test]
    fn trim_fn() {
        assert_eq!(evals("trim('  hello  ')"), "hello");
        assert_eq!(evals(r"trim('  hel\'lo  ')"), "hel'lo");
    }

    #[test]
    fn strlen_fn() {
        assert_eq!(evals("strlen('hello')"), "5");
    }

    #[test]
    fn output_count_fn() {
        let mut out = Output::default();
        out.push("items", "a");
        out.push("items", "b");
        out.push("items", "c");
        let count = eval_expr(
            "output_count('items')",
            &HashMap::new(),
            &HashMap::new(),
            &out,
        )
        .unwrap()
        .into_string();
        assert_eq!(count, "3");

        let empty = eval_expr(
            "output_count('missing') > 0",
            &HashMap::new(),
            &HashMap::new(),
            &out,
        )
        .unwrap();
        assert_eq!(empty, Value::Bool(false));
    }

    #[test]
    fn complex_expression() {
        let mut out = Output::default();
        out.push("x", "3");
        let result =
            eval_expr("(output.x + 1) * 2", &HashMap::new(), &HashMap::new(), &out).unwrap();
        assert_eq!(result.into_string(), "8");
    }

    #[test]
    fn division_by_zero_error() {
        assert!(eval("1 / 0").is_err());
    }

    #[test]
    fn unknown_function_error() {
        assert!(eval("foo(1)").is_err());
    }

    #[test]
    fn unterminated_string_error() {
        assert!(eval("'hello").is_err());
    }

    #[test]
    fn string_escape_single_quote() {
        assert_eq!(evals(r"'it\'s'"), "it's");
    }

    #[test]
    fn string_escape_backslash() {
        assert_eq!(evals(r"'C:\\Users'"), r"C:\Users");
    }

    #[test]
    fn string_escape_newline() {
        assert_eq!(evals("'a\\nb'"), "a\nb");
    }

    #[test]
    fn string_unknown_escape_error() {
        assert!(eval(r"'\q'").is_err());
    }

    // ── Bool / comparison ──────────────────────────────────────────────────────

    #[test]
    fn eq_numeric() {
        assert_eq!(eval("5 == 5").unwrap(), Value::Bool(true));
        assert_eq!(eval("5 == 6").unwrap(), Value::Bool(false));
    }

    #[test]
    fn eq_string() {
        assert_eq!(eval("'abc' == 'abc'").unwrap(), Value::Bool(true));
        assert_eq!(eval("'abc' == 'xyz'").unwrap(), Value::Bool(false));
    }

    #[test]
    fn ne_operator() {
        assert_eq!(eval("5 != 6").unwrap(), Value::Bool(true));
        assert_eq!(eval("5 != 5").unwrap(), Value::Bool(false));
        assert_eq!(eval("'abc' != 'xyz'").unwrap(), Value::Bool(true));
        assert_eq!(eval("'abc' != 'abc'").unwrap(), Value::Bool(false));
        assert_eq!(eval("'' != ''").unwrap(), Value::Bool(false));
    }

    #[test]
    fn lt_gt_operators() {
        assert_eq!(eval("3 < 5").unwrap(), Value::Bool(true));
        assert_eq!(eval("5 < 3").unwrap(), Value::Bool(false));
        assert_eq!(eval("5 > 3").unwrap(), Value::Bool(true));
        assert_eq!(eval("3 > 5").unwrap(), Value::Bool(false));
    }

    #[test]
    fn lteq_gteq_operators() {
        assert_eq!(eval("5 <= 5").unwrap(), Value::Bool(true));
        assert_eq!(eval("5 <= 4").unwrap(), Value::Bool(false));
        assert_eq!(eval("5 >= 5").unwrap(), Value::Bool(true));
        assert_eq!(eval("4 >= 5").unwrap(), Value::Bool(false));
    }

    #[test]
    fn modulo_operator() {
        assert_eq!(evals("10 % 3"), "1");
        assert_eq!(evals("100 % 10"), "0");
        assert_eq!(evals("7 % 4"), "3");
    }

    #[test]
    fn modulo_with_comparison() {
        // (output.count % 10 == 0) pattern
        let mut out = Output::default();
        out.push("count", "30");
        let result = eval_expr(
            "output.count % 10 == 0",
            &HashMap::new(),
            &HashMap::new(),
            &out,
        )
        .unwrap();
        assert_eq!(result, Value::Bool(true));

        let mut out2 = Output::default();
        out2.push("count", "31");
        let result2 = eval_expr(
            "output.count % 10 == 0",
            &HashMap::new(),
            &HashMap::new(),
            &out2,
        )
        .unwrap();
        assert_eq!(result2, Value::Bool(false));
    }

    #[test]
    fn bool_into_bool() {
        assert_eq!(eval("5 == 5").unwrap().into_bool(), Ok(true));
    }

    #[test]
    fn non_bool_into_bool_errors() {
        assert!(eval("42").unwrap().into_bool().is_err());
        assert!(eval("'hello'").unwrap().into_bool().is_err());
    }

    #[test]
    fn eval_bool_expr_fn() {
        let out = Output::default();
        assert_eq!(
            eval_bool_expr("3 > 1", &HashMap::new(), &HashMap::new(), &out),
            Ok(true)
        );
        assert!(eval_bool_expr("42", &HashMap::new(), &HashMap::new(), &out).is_err());
    }

    #[test]
    fn modulo_by_zero_error() {
        assert!(eval("5 % 0").is_err());
    }

    // ── Logical &&  ||  ────────────────────────────────────────────────────────

    #[test]
    fn and_operator() {
        assert_eq!(eval("1 < 2 && 3 < 4").unwrap(), Value::Bool(true));
        assert_eq!(eval("1 < 2 && 3 > 4").unwrap(), Value::Bool(false));
        assert_eq!(eval("1 > 2 && 3 < 4").unwrap(), Value::Bool(false));
        assert_eq!(eval("1 > 2 && 3 > 4").unwrap(), Value::Bool(false));
    }

    #[test]
    fn or_operator() {
        assert_eq!(eval("1 < 2 || 3 > 4").unwrap(), Value::Bool(true));
        assert_eq!(eval("1 > 2 || 3 < 4").unwrap(), Value::Bool(true));
        assert_eq!(eval("1 > 2 || 3 > 4").unwrap(), Value::Bool(false));
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // (false && true) || true  → false || true → true
        assert_eq!(eval("1 > 2 && 3 < 4 || 5 < 6").unwrap(), Value::Bool(true));
        // true || (false && true)  → true || false → true
        assert_eq!(eval("1 < 2 || 3 > 4 && 5 < 6").unwrap(), Value::Bool(true));
    }

    #[test]
    fn logical_with_parens() {
        // (true || false) && false  → true && false → false
        assert_eq!(
            eval("(1 < 2 || 3 > 4) && 5 > 6").unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn logical_non_bool_operand_errors() {
        assert!(eval("1 && 2 < 3").is_err());
        assert!(eval("1 < 2 || 'hello'").is_err());
    }

    #[test]
    fn eq_combined_with_and() {
        assert_eq!(eval("1 == 1 && 2 == 2").unwrap(), Value::Bool(true));
        assert_eq!(eval("1 == 1 && 2 == 3").unwrap(), Value::Bool(false));
        assert_eq!(eval("1 == 2 && 2 == 2").unwrap(), Value::Bool(false));
    }

    #[test]
    fn eq_combined_with_or() {
        assert_eq!(eval("1 == 2 || 2 == 2").unwrap(), Value::Bool(true));
        assert_eq!(eval("1 == 2 || 3 == 4").unwrap(), Value::Bool(false));
    }

    #[test]
    fn range_check_with_and() {
        // 1 < x && x < 10 pattern
        let mut locals = HashMap::new();
        locals.insert("x".into(), "5".into());
        let r = |expr| eval_expr(expr, &locals, &HashMap::new(), &empty_output()).unwrap();
        assert_eq!(r("1 < x && x < 10"), Value::Bool(true));
        locals.insert("x".into(), "0".into());
        let r2 = |expr| eval_expr(expr, &locals, &HashMap::new(), &empty_output()).unwrap();
        assert_eq!(r2("1 < x && x < 10"), Value::Bool(false));
    }

    #[test]
    fn precedence_and_over_or_with_eq() {
        // true || (false && false) → true
        assert_eq!(
            eval("1 == 1 || 2 == 3 && 4 == 5").unwrap(),
            Value::Bool(true)
        );
        // (false || false) && true → but parsed as: false || (false && true) → false
        assert_eq!(
            eval("1 == 2 || 2 == 3 && 1 == 1").unwrap(),
            Value::Bool(false)
        );
    }

    // ── regex_match ────────────────────────────────────────────────────────────

    #[test]
    fn regex_match_full_match() {
        // ^abc$ only matches the exact string "abc"
        assert_eq!(
            eval("regex_match('abc', '^abc$')").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval("regex_match('abcd', '^abc$')").unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            eval("regex_match('xabc', '^abc$')").unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn regex_match_partial() {
        // without anchors, matches anywhere in the string
        assert_eq!(
            eval("regex_match('hello world', 'world')").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval("regex_match('hello world', 'xyz')").unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn regex_match_case_insensitive_flag() {
        // Use raw Rust strings so \\ reaches the expression parser as the escape for a literal backslash.
        assert_eq!(
            eval(r"regex_match('Git-2.53.0.2-64-bit.exe', '(?i)^git-.+\\.exe$')").unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval(r"regex_match('notepad.exe', '(?i)^git-.+\\.exe$')").unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn regex_match_invalid_pattern_errors() {
        assert!(eval("regex_match('abc', '[invalid')").is_err());
    }

    // ── regex_extract ──────────────────────────────────────────────────────────

    #[test]
    fn regex_extract_capture_group() {
        // first capture group returned when present
        assert_eq!(
            eval("regex_extract('Git-2.53.0.2-64-bit.exe', 'Git-([0-9.]+)')").unwrap(),
            Value::String("2.53.0.2".into())
        );
    }

    #[test]
    fn regex_extract_full_match_when_no_group() {
        // no capture group → returns the full match
        assert_eq!(
            eval("regex_extract('version: 42', '[0-9]+')").unwrap(),
            Value::String("42".into())
        );
    }

    #[test]
    fn regex_extract_empty_on_no_match() {
        assert_eq!(
            eval("regex_extract('hello', '^[0-9]+$')").unwrap(),
            Value::String("".into())
        );
    }

    #[test]
    fn regex_extract_invalid_pattern_errors() {
        assert!(eval("regex_extract('abc', '[bad')").is_err());
    }
}
