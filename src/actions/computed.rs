use std::collections::HashSet;

use serde_json::{Map, Value};
use streamdeck_lib::prelude::*;

use crate::render::render_number;
use crate::topics::COUNTER_CHANGED;

pub struct ComputedAction {
    config: ExprConfig,
    deps: HashSet<String>,
}

impl Default for ComputedAction {
    fn default() -> Self {
        Self {
            config: ExprConfig::default(),
            deps: HashSet::new(),
        }
    }
}

impl ActionStatic for ComputedAction {
    const ID: &'static str = super::ids::COMPUTED;
}

impl Action for ComputedAction {
    fn id(&self) -> &str {
        Self::ID
    }

    fn topics(&self) -> &'static [&'static str] {
        &[COUNTER_CHANGED.name]
    }

    fn init(&mut self, cx: &Context, ctx_id: &str) {
        cx.sd().get_settings(ctx_id);
        render_number(cx, ctx_id, 0);
    }

    fn did_receive_settings(&mut self, cx: &Context, ev: &incoming::DidReceiveSettings) {
        self.config = parse_settings(&ev.settings);
        self.deps = collect_idents(self.config.expression.as_deref().unwrap_or_default());
        let v = compute(cx, &self.config);
        render_number(cx, ev.context, v);
    }

    fn on_notify(&mut self, cx: &Context, ctx_id: &str, event: &ErasedTopic) {
        if let Some(msg) = event.downcast(COUNTER_CHANGED) {
            // Skip if we have explicit deps and the changed key isn't one of them
            if !self.deps.is_empty() && !self.deps.contains(&msg.counter_key) {
                return;
            }
            let v = compute(cx, &self.config);
            render_number(cx, ctx_id, v);
        }
    }
}

// ── Settings ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
struct ExprConfig {
    expression: Option<String>,
    missing_as_zero: bool,
}

fn parse_settings(v: &Map<String, Value>) -> ExprConfig {
    let mut c = ExprConfig::default();
    if let Some(s) = v.get("expression").and_then(|x| x.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            c.expression = Some(t.to_string());
        }
    }
    c.missing_as_zero = v
        .get("missingAsZero")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    c
}

// ── Tokenizer ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tok<'a> {
    Num(i64),
    Id(&'a str),
    IdQ,
    Op(BinOp),
    UMinus,
    LParen,
    RParen,
}

struct TokStream<'a> {
    toks: Vec<Tok<'a>>,
    qbuf: Vec<String>,
}

fn tokenize(expr: &str) -> TokStream<'_> {
    let bytes = expr.as_bytes();
    let mut i = 0usize;
    let mut toks = Vec::new();
    let mut qbuf = Vec::new();
    let mut prev_was_value = false;

    let skip_ws = |i: &mut usize| {
        while *i < bytes.len() && (bytes[*i] as char).is_ascii_whitespace() {
            *i += 1;
        }
    };

    let parse_quoted = |i: &mut usize| -> Option<String> {
        if *i >= bytes.len() {
            return None;
        }
        let quote = bytes[*i] as char;
        if !matches!(quote, '"' | '\'' | '`') {
            return None;
        }
        *i += 1;
        let start = *i;
        while *i < bytes.len() {
            let c = bytes[*i] as char;
            if c == quote {
                let s = &expr[start..*i];
                *i += 1;
                return Some(s.to_string());
            }
            *i += 1;
        }
        None
    };

    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        if c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            let n = expr[start..i].parse::<i64>().unwrap_or(0);
            toks.push(Tok::Num(n));
            prev_was_value = true;
            continue;
        }

        if matches!(c, '"' | '\'' | '`') {
            let mut j = i;
            if let Some(s) = parse_quoted(&mut j) {
                toks.push(Tok::IdQ);
                qbuf.push(s);
                i = j;
                prev_was_value = true;
                continue;
            } else {
                i += 1;
                continue;
            }
        }

        if c == '_' || c.is_ascii_alphabetic() {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            let ident = &expr[start..i];

            let mut j = i;
            skip_ws(&mut j);
            if ident.eq_ignore_ascii_case("var") && j < bytes.len() && (bytes[j] as char) == '(' {
                j += 1;
                skip_ws(&mut j);
                if let Some(s) = parse_quoted(&mut j) {
                    skip_ws(&mut j);
                    if j < bytes.len() && (bytes[j] as char) == ')' {
                        j += 1;
                        toks.push(Tok::IdQ);
                        qbuf.push(s);
                        i = j;
                        prev_was_value = true;
                        continue;
                    }
                }
            }

            toks.push(Tok::Id(ident));
            prev_was_value = true;
            continue;
        }

        match c {
            '+' => {
                toks.push(Tok::Op(BinOp::Add));
                prev_was_value = false;
                i += 1;
            }
            '*' => {
                toks.push(Tok::Op(BinOp::Mul));
                prev_was_value = false;
                i += 1;
            }
            '/' => {
                toks.push(Tok::Op(BinOp::Div));
                prev_was_value = false;
                i += 1;
            }
            '-' => {
                if prev_was_value {
                    toks.push(Tok::Op(BinOp::Sub));
                } else {
                    toks.push(Tok::UMinus);
                }
                prev_was_value = false;
                i += 1;
            }
            '(' => {
                toks.push(Tok::LParen);
                prev_was_value = false;
                i += 1;
            }
            ')' => {
                toks.push(Tok::RParen);
                prev_was_value = true;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    TokStream { toks, qbuf }
}

// ── Shunting-yard → RPN ──────────────────────────────────────────────────────

fn prec(t: Tok) -> i32 {
    match t {
        Tok::UMinus => 3,
        Tok::Op(BinOp::Mul) | Tok::Op(BinOp::Div) => 2,
        Tok::Op(BinOp::Add) | Tok::Op(BinOp::Sub) => 1,
        _ => -1,
    }
}

fn to_rpn<'a>(ts: &TokStream<'a>) -> Vec<Tok<'a>> {
    let mut out = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();

    for &t in &ts.toks {
        match t {
            Tok::Num(_) | Tok::Id(_) | Tok::IdQ => out.push(t),

            Tok::UMinus
            | Tok::Op(BinOp::Add)
            | Tok::Op(BinOp::Sub)
            | Tok::Op(BinOp::Mul)
            | Tok::Op(BinOp::Div) => {
                while let Some(&top) = ops.last() {
                    if matches!(
                        top,
                        Tok::UMinus
                            | Tok::Op(BinOp::Add)
                            | Tok::Op(BinOp::Sub)
                            | Tok::Op(BinOp::Mul)
                            | Tok::Op(BinOp::Div)
                    ) && prec(top) >= prec(t)
                    {
                        out.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.push(t);
            }

            Tok::LParen => ops.push(t),

            Tok::RParen => {
                while let Some(op) = ops.pop() {
                    if matches!(op, Tok::LParen) {
                        break;
                    }
                    out.push(op);
                }
            }
        }
    }

    while let Some(op) = ops.pop() {
        if !matches!(op, Tok::LParen) {
            out.push(op);
        }
    }
    out
}

// ── RPN evaluator ────────────────────────────────────────────────────────────

fn eval_rpn<'a, F>(rpn: &[Tok<'a>], qbuf: &[String], mut var: F, missing_as_zero: bool) -> i64
where
    F: FnMut(&str) -> (i64, bool),
{
    let mut st: Vec<i64> = Vec::new();
    let mut qi = 0usize;

    for &t in rpn {
        match t {
            Tok::Num(n) => st.push(n),

            Tok::Id(name) => {
                let (v, present) = var(name);
                st.push(if present { v } else if missing_as_zero { 0 } else { 1 });
            }

            Tok::IdQ => {
                let name = &qbuf[qi];
                qi += 1;
                let (v, present) = var(name);
                st.push(if present { v } else if missing_as_zero { 0 } else { 1 });
            }

            Tok::UMinus => {
                let x = st.pop().unwrap_or(0);
                st.push(x.saturating_neg());
            }

            Tok::Op(op) => {
                let b = st.pop().unwrap_or(0);
                let a = st.pop().unwrap_or(0);
                let v = match op {
                    BinOp::Add => a.saturating_add(b),
                    BinOp::Sub => a.saturating_sub(b),
                    BinOp::Mul => a.saturating_mul(b),
                    BinOp::Div => {
                        if b == 0 { 0 } else { a / b }
                    }
                };
                st.push(v);
            }

            _ => {}
        }
    }
    st.pop().unwrap_or(0)
}

fn collect_idents(expr: &str) -> HashSet<String> {
    let ts = tokenize(expr);
    let mut set = HashSet::new();
    let mut qi = 0usize;
    for t in &ts.toks {
        match *t {
            Tok::Id(name) => {
                set.insert(name.to_string());
            }
            Tok::IdQ => {
                set.insert(ts.qbuf[qi].clone());
                qi += 1;
            }
            _ => {}
        }
    }
    set
}

fn compute(cx: &Context, cfg: &ExprConfig) -> i64 {
    let expr = match &cfg.expression {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return 0,
    };
    let ts = tokenize(expr);
    let rpn = to_rpn(&ts);

    let globals = cx.globals();
    eval_rpn(
        &rpn,
        &ts.qbuf,
        |id| {
            let val = globals
                .get("counters")
                .and_then(|v| v.get(id).and_then(|v| v.as_i64()));
            match val {
                Some(n) => (n, true),
                None => (0, false),
            }
        },
        cfg.missing_as_zero,
    )
}
