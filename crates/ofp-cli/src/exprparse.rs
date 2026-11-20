//! The table expression language, parsed into the tree the core evaluates.
//!
//! ```text
//! u8[3]   i8[3]   u16le[4]   u16be[4]   i16le[4]   i16be[4]   u24le[4]   u32le[4]
//! 12   0x7fff   1609.344
//! * /        then  + -        then  << >>      then  &   then  ^   then  |
//! == != < <= > >=   then  &&   then  ||   then  condition ? a : b
//! !x   -x   ( … )
//! ```
//!
//! Bit operators bind tighter than comparisons, so `u8[2] & 0x80 == 0` means `(u8[2] & 0x80) == 0`.

use ofp_core::{BinOp, Expr};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Sym(&'static str),
}

const SYMBOLS: [&str; 24] = ["<<", ">>", "==", "!=", "<=", ">=", "&&", "||", "+", "-", "*", "/", "&", "|", "^", "<", ">", "!", "?", ":", "(", ")", "[", "]"];

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    'outer: while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            if c == '0' && i + 1 < b.len() && (b[i + 1] == b'x' || b[i + 1] == b'X') {
                i += 2;
                while i < b.len() && (b[i] as char).is_ascii_hexdigit() {
                    i += 1;
                }
                let v = u64::from_str_radix(&src[start + 2..i], 16).map_err(|_| format!("bad number '{}'", &src[start..i]))?;
                out.push(Tok::Num(v as f64));
            } else {
                while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                out.push(Tok::Num(src[start..i].parse().map_err(|_| format!("bad number '{}'", &src[start..i]))?));
            }
            continue;
        }
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < b.len() && ((b[i] as char).is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(src[start..i].to_string()));
            continue;
        }
        for s in SYMBOLS {
            if src[i..].starts_with(s) {
                out.push(Tok::Sym(s));
                i += s.len();
                continue 'outer;
            }
        }
        return Err(format!("unexpected '{c}'"));
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
    Expr::Bin(op, Box::new(a), Box::new(b))
}

impl Parser {
    fn peek_sym(&self) -> Option<&'static str> {
        match self.toks.get(self.pos) {
            Some(Tok::Sym(s)) => Some(s),
            _ => None,
        }
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.peek_sym() == Some(s) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(format!("expected '{s}'"))
        }
    }

    /// One left-associative level: `next (op next)*`.
    fn level(&mut self, ops: &[(&str, BinOp)], next: fn(&mut Parser) -> Result<Expr, String>) -> Result<Expr, String> {
        let mut left = next(self)?;
        'again: loop {
            for (s, op) in ops {
                if self.eat(s) {
                    left = bin(*op, left, next(self)?);
                    continue 'again;
                }
            }
            return Ok(left);
        }
    }

    fn ternary(&mut self) -> Result<Expr, String> {
        let cond = self.or()?;
        if self.eat("?") {
            let a = self.ternary()?;
            self.expect(":")?;
            let b = self.ternary()?;
            return Ok(Expr::If(Box::new(cond), Box::new(a), Box::new(b)));
        }
        Ok(cond)
    }

    fn or(&mut self) -> Result<Expr, String> {
        self.level(&[("||", BinOp::Either)], Parser::and)
    }
    fn and(&mut self) -> Result<Expr, String> {
        self.level(&[("&&", BinOp::Both)], Parser::compare)
    }
    fn compare(&mut self) -> Result<Expr, String> {
        self.level(&[("==", BinOp::Eq), ("!=", BinOp::Ne), ("<=", BinOp::Le), (">=", BinOp::Ge), ("<", BinOp::Lt), (">", BinOp::Gt)], Parser::bit_or)
    }
    fn bit_or(&mut self) -> Result<Expr, String> {
        self.level(&[("|", BinOp::Or)], Parser::bit_xor)
    }
    fn bit_xor(&mut self) -> Result<Expr, String> {
        self.level(&[("^", BinOp::Xor)], Parser::bit_and)
    }
    fn bit_and(&mut self) -> Result<Expr, String> {
        self.level(&[("&", BinOp::And)], Parser::shift)
    }
    fn shift(&mut self) -> Result<Expr, String> {
        self.level(&[("<<", BinOp::Shl), (">>", BinOp::Shr)], Parser::additive)
    }
    fn additive(&mut self) -> Result<Expr, String> {
        self.level(&[("+", BinOp::Add), ("-", BinOp::Sub)], Parser::multiplicative)
    }
    fn multiplicative(&mut self) -> Result<Expr, String> {
        self.level(&[("*", BinOp::Mul), ("/", BinOp::Div)], Parser::unary)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.toks.get(self.pos).cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(Expr::Num(n))
            }
            Some(Tok::Sym("(")) => {
                self.pos += 1;
                let e = self.ternary()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                self.expect("[")?;
                let at = match self.toks.get(self.pos) {
                    Some(Tok::Num(n)) if n.fract() == 0.0 && *n >= 0.0 && *n < 4096.0 => *n as usize,
                    _ => return Err(format!("{name}[…] needs a byte position")),
                };
                self.pos += 1;
                self.expect("]")?;
                Ok(match name.as_str() {
                    "u8" => Expr::U8(at),
                    "i8" => Expr::I8(at),
                    "u16le" => Expr::U16le(at),
                    "u16be" => Expr::U16be(at),
                    "i16le" => Expr::I16le(at),
                    "i16be" => Expr::I16be(at),
                    "u24le" => Expr::U24le(at),
                    "u32le" => Expr::U32le(at),
                    other => return Err(format!("unknown reader '{other}' (u8, i8, u16le, u16be, i16le, i16be, u24le, u32le)")),
                })
            }
            Some(Tok::Sym(s)) => Err(format!("unexpected '{s}'")),
            None => Err("the expression ends too early".into()),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr, String> {
    let mut p = Parser { toks: lex(src).map_err(|e| format!("'{src}': {e}"))?, pos: 0 };
    let e = p.ternary().map_err(|e| format!("'{src}': {e}"))?;
    if p.pos != p.toks.len() {
        return Err(format!("'{src}': unexpected text after the expression"));
    }
    if !e.within_limits() {
        return Err(format!("'{src}': the expression is too large"));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, p: &[u8]) -> Option<f64> {
        parse(src).unwrap().eval(p)
    }

    #[test]
    fn reads_and_arithmetic() {
        let p = [0x06, 0x30, 0x00, 0x07, 0x48, 0x03, 0x8c, 0x05, 0xbb, 0x00];
        assert_eq!(ev("u8[3]", &p), Some(7.0));
        assert_eq!(ev("u16le[4] / 10", &p), Some(84.0));
        assert_eq!(ev("u16be[4]", &p), Some(0x4803 as f64));
        assert_eq!(ev("u8[0] * 256 + u8[1]", &p), Some(1584.0));
        assert_eq!(ev("1 + 2 * 3", &p), Some(7.0));
        assert_eq!(ev("(1 + 2) * 3", &p), Some(9.0));
        assert_eq!(ev("-u8[3] + 10", &p), Some(3.0));
        assert_eq!(ev("u8[40]", &p), None);
        assert_eq!(ev("u8[3] / 0", &p), None);
    }

    #[test]
    fn bits_bind_tighter_than_comparisons() {
        let p = [0x33, 0x80];
        assert_eq!(ev("u16le[0] & 0x7fff", &p), Some(0x33 as f64));
        assert_eq!(ev("u16le[0] & 0x8000 == 0", &p), Some(0.0));
        assert_eq!(ev("u16le[0] & 0x8000 ? 1000 : 1609.344", &p), Some(1000.0));
        assert_eq!(ev("u8[0] >> 4", &p), Some(3.0));
        assert_eq!(ev("u8[0] == 0x33 && u8[1] >= 0x21", &p), Some(1.0));
        assert_eq!(ev("!(u8[0] == 0x33) || u8[1] < 5", &p), Some(0.0));
    }

    #[test]
    fn errors() {
        assert!(parse("u8[").is_err());
        assert!(parse("u9[1]").is_err());
        assert!(parse("1 +").is_err());
        assert!(parse("1 2").is_err());
        assert!(parse("u8[1] $ 2").is_err());
        assert!(parse(&"(".repeat(40)).is_err());
    }
}
