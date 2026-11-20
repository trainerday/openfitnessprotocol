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
    /// A part of the table's `layout`, by its index: read where it sits in this packet (parts before it
    /// may be present or not). No value when the part is not in this packet.
    Part(usize),
    /// In a field with a `rate`: how many counts per second the counter went up by, between the last
    /// two events.
    Rate,
    /// The wheel's circumference in metres, as the app set it (for speed from a wheel counter).
    Wheel,
}

/// The type of a part of a layout: how many bytes it takes and how they are read.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PartType {
    U8,
    I8,
    U16le,
    I16le,
    U24le,
    I24le,
    U32le,
    /// Bytes that are skipped (a value no field reads).
    Skip1,
    Skip2,
    Skip3,
    Skip4,
}

impl PartType {
    pub fn width(self) -> usize {
        match self {
            PartType::U8 | PartType::I8 | PartType::Skip1 => 1,
            PartType::U16le | PartType::I16le | PartType::Skip2 => 2,
            PartType::U24le | PartType::I24le | PartType::Skip3 => 3,
            PartType::U32le | PartType::Skip4 => 4,
        }
    }

    fn read(self, p: &[u8], at: usize) -> Option<f64> {
        Some(match self {
            PartType::U8 => *p.get(at)? as f64,
            PartType::I8 => (*p.get(at)? as i8) as f64,
            PartType::U16le => u16::from_le_bytes(bytes(p, at)?) as f64,
            PartType::I16le => i16::from_le_bytes(bytes(p, at)?) as f64,
            PartType::U24le => {
                let b: [u8; 3] = bytes(p, at)?;
                u32::from_le_bytes([b[0], b[1], b[2], 0]) as f64
            }
            PartType::I24le => {
                let b: [u8; 3] = bytes(p, at)?;
                let fill = if b[2] & 0x80 != 0 { 0xff } else { 0 };
                i32::from_le_bytes([b[0], b[1], b[2], fill]) as f64
            }
            PartType::U32le => u32::from_le_bytes(bytes(p, at)?) as f64,
            PartType::Skip1 | PartType::Skip2 | PartType::Skip3 | PartType::Skip4 => return None,
        })
    }
}

/// What an expression may read besides the packet: where each part of the layout sits in it, the
/// field's rate, the wheel size. Empty for tables without a layout or rates.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
    /// Each part's type and, when it is in this packet, its position.
    pub parts: &'a [(PartType, Option<usize>)],
    pub rate: Option<f64>,
    pub wheel_m: Option<f64>,
    /// The repeating part's index, and which of its elements is read (for a list field).
    pub repeat: Option<(usize, usize)>,
}

impl Cx<'_> {
    pub const NONE: Cx<'static> = Cx { parts: &[], rate: None, wheel_m: None, repeat: None };
}

fn bytes<const N: usize>(p: &[u8], at: usize) -> Option<[u8; N]> {
    let end = at.checked_add(N)?;
    p.get(at..end)?.try_into().ok()
}

impl Expr {
    /// `None` when a read is outside the packet or a division is by zero: the packet has no value
    /// for this field.
    pub fn eval(&self, p: &[u8]) -> Option<f64> {
        self.eval_in(p, &Cx::NONE)
    }

    /// The same, for a table with a layout or rates.
    pub fn eval_in(&self, p: &[u8], cx: &Cx) -> Option<f64> {
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
            Expr::Part(i) => {
                let (ty, at) = cx.parts.get(*i)?;
                let element = match cx.repeat {
                    Some((r, k)) if r == *i => k,
                    _ => 0,
                };
                ty.read(p, (*at)? + element * ty.width())?
            }
            Expr::Rate => cx.rate?,
            Expr::Wheel => cx.wheel_m?,
            Expr::Not(a) => truth(a.eval_in(p, cx)? == 0.0),
            Expr::Neg(a) => -a.eval_in(p, cx)?,
            Expr::If(c, a, b) => {
                if c.eval_in(p, cx)? != 0.0 {
                    a.eval_in(p, cx)?
                } else {
                    b.eval_in(p, cx)?
                }
            }
            Expr::Bin(op, a, b) => {
                let (x, y) = (a.eval_in(p, cx)?, b.eval_in(p, cx)?);
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

    /// The highest layout part this expression reads, and whether it reads the rate.
    pub fn uses(&self) -> (Option<usize>, bool) {
        match self {
            Expr::Part(i) => (Some(*i), false),
            Expr::Rate => (None, true),
            Expr::Bin(_, a, b) => merge(a.uses(), b.uses()),
            Expr::Not(a) | Expr::Neg(a) => a.uses(),
            Expr::If(c, a, b) => merge(merge(c.uses(), a.uses()), b.uses()),
            _ => (None, false),
        }
    }

    pub fn within_limits(&self) -> bool {
        let (d, n) = self.size();
        d <= MAX_EXPR_DEPTH && n <= MAX_EXPR_NODES
    }
}

fn merge(a: (Option<usize>, bool), b: (Option<usize>, bool)) -> (Option<usize>, bool) {
    (a.0.max(b.0), a.1 || b.1)
}

fn truth(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}
