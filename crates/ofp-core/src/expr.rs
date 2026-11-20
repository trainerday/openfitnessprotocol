//! The value tree a table compiles to. The core only evaluates it: no parsing, no loops.

use alloc::boxed::Box;
use serde::{Deserialize, Serialize};

pub const MAX_EXPR_DEPTH: usize = 16;
pub const MAX_EXPR_NODES: usize = 64;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Logical and / or: operands are true when non-zero.
    Both,
    Either,
}

/// Offsets count from the start of the payload (after the company id, when the match rule has one).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Expr {
    Num(f64),
    U8(usize),
    I8(usize),
    U16le(usize),
    U16be(usize),
    I16le(usize),
    I16be(usize),
    U24le(usize),
    U32le(usize),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    /// condition, value when true, value when false
    If(Box<Expr>, Box<Expr>, Box<Expr>),
}

fn bytes<const N: usize>(p: &[u8], at: usize) -> Option<[u8; N]> {
    let end = at.checked_add(N)?;
    p.get(at..end)?.try_into().ok()
}

impl Expr {
    /// `None` when a read is outside the packet or a division is by zero: the packet has no value
    /// for this field.
    pub fn eval(&self, p: &[u8]) -> Option<f64> {
        Some(match self {
            Expr::Num(n) => *n,
            Expr::U8(at) => *p.get(*at)? as f64,
            Expr::I8(at) => (*p.get(*at)? as i8) as f64,
            Expr::U16le(at) => u16::from_le_bytes(bytes(p, *at)?) as f64,
            Expr::U16be(at) => u16::from_be_bytes(bytes(p, *at)?) as f64,
            Expr::I16le(at) => i16::from_le_bytes(bytes(p, *at)?) as f64,
            Expr::I16be(at) => i16::from_be_bytes(bytes(p, *at)?) as f64,
            Expr::U24le(at) => {
                let b: [u8; 3] = bytes(p, *at)?;
                u32::from_le_bytes([b[0], b[1], b[2], 0]) as f64
            }
            Expr::U32le(at) => u32::from_le_bytes(bytes(p, *at)?) as f64,
            Expr::Not(a) => truth(a.eval(p)? == 0.0),
            Expr::Neg(a) => -a.eval(p)?,
            Expr::If(c, a, b) => {
                if c.eval(p)? != 0.0 {
                    a.eval(p)?
                } else {
                    b.eval(p)?
                }
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = (a.eval(p)?, b.eval(p)?);
                match op {
                    BinOp::Add => x + y,
                    BinOp::Sub => x - y,
                    BinOp::Mul => x * y,
                    BinOp::Div => {
                        if y == 0.0 {
                            return None;
                        }
                        x / y
                    }
                    BinOp::And => ((x as i64) & (y as i64)) as f64,
                    BinOp::Or => ((x as i64) | (y as i64)) as f64,
                    BinOp::Xor => ((x as i64) ^ (y as i64)) as f64,
                    BinOp::Shl => {
                        let s = y as i64;
                        if !(0..=32).contains(&s) {
                            return None;
                        }
                        ((x as i64) << s) as f64
                    }
                    BinOp::Shr => {
                        let s = y as i64;
                        if !(0..=32).contains(&s) {
                            return None;
                        }
                        ((x as i64) >> s) as f64
                    }
                    BinOp::Eq => truth(x == y),
                    BinOp::Ne => truth(x != y),
                    BinOp::Lt => truth(x < y),
                    BinOp::Le => truth(x <= y),
                    BinOp::Gt => truth(x > y),
                    BinOp::Ge => truth(x >= y),
                    BinOp::Both => truth(x != 0.0 && y != 0.0),
                    BinOp::Either => truth(x != 0.0 || y != 0.0),
                }
            }
        })
    }

    /// (depth, node count) — checked when a bundle is loaded, so evaluation is bounded.
    pub fn size(&self) -> (usize, usize) {
        match self {
            Expr::Bin(_, a, b) => {
                let (da, na) = a.size();
                let (db, nb) = b.size();
                (1 + da.max(db), 1 + na + nb)
            }
            Expr::Not(a) | Expr::Neg(a) => {
                let (d, n) = a.size();
                (1 + d, 1 + n)
            }
            Expr::If(c, a, b) => {
                let (dc, nc) = c.size();
                let (da, na) = a.size();
                let (db, nb) = b.size();
                (1 + dc.max(da).max(db), 1 + nc + na + nb)
            }
            _ => (1, 1),
        }
    }

    pub fn within_limits(&self) -> bool {
        let (d, n) = self.size();
        d <= MAX_EXPR_DEPTH && n <= MAX_EXPR_NODES
    }
}

fn truth(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}
