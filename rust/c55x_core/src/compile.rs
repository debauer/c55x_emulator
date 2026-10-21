//! ir.py expression nodes -> Expr (port of values.Compiler.value and friends). Context
//! flags follow the Python calls: unsigned (inside uns()), io (readport/writeport).

use std::cell::Cell;
use std::rc::Rc;

use pyo3::prelude::*;

use crate::addr::{unsupported_mode, Ea, Mode};
use crate::err::{unsupported, R};
use crate::expr::{meta, Expr, Op};
use crate::state::{reg_by_name, reg_meta, Reg};

pub type Node<'py> = Bound<'py, PyAny>;

pub fn item<'py>(n: &Node<'py>, i: usize) -> R<Node<'py>> {
    Ok(n.get_item(i)?)
}

pub fn text(n: &Node<'_>, i: usize) -> R<String> {
    Ok(n.get_item(i)?.extract::<String>()?)
}

pub fn int(n: &Node<'_>, i: usize) -> R<i64> {
    Ok(n.get_item(i)?.extract::<i64>()?)
}

pub fn reg(name: &str) -> R<Reg> {
    match reg_by_name(name) {
        Some(r) => Ok(r),
        None => unsupported(format!("register {}", name)),
    }
}

fn is_bin(n: &Node<'_>, op: &str) -> R<bool> {
    Ok(text(n, 0)? == "bin" && text(n, 1)? == op)
}

pub fn ea(n: &Node<'_>, words: i32) -> R<Ea> {
    let mode = text(n, 1)?;
    let arg = item(n, 3)?;
    Ok(match mode.as_str() {
        "abs" => Ea::Abs(arg.extract::<i64>()? as u32),
        "abs16" => Ea::Abs16(arg.extract::<i64>()? as u32),
        "port" => Ea::Port(arg.extract::<i64>()? as u32),
        "sp" | "dp" => Ea::Sp(arg.extract::<i64>()? as i32),
        _ => {
            let r = text(n, 2)?;
            let p = if r == "CDP" { 8 } else {
                match r.strip_prefix("AR").and_then(|s| s.parse::<u8>().ok()) {
                    Some(i) if i < 8 => i,
                    _ => return unsupported(format!("pointer {}", r)),
                }
            };
            let k = || -> R<i32> { Ok(arg.extract::<i64>()? as i32) };
            let m = match mode.as_str() {
                "ind" => Mode::Ind,
                "idx" => Mode::Idx(k()?),
                "idxreg" => Mode::IdxReg(reg(&arg.extract::<String>()?)?),
                "post" => Mode::Post(k()?),
                "pre" => Mode::Pre(k()?),
                "short" => Mode::Short(k()?),
                "postreg" => {
                    let sign = if arg.get_item(0)?.extract::<String>()? == "+" { 1 } else { -1 };
                    let what = arg.get_item(1)?;
                    match what.extract::<i64>() {
                        Ok(v) => Mode::PostK(sign * v as i32),
                        Err(_) => Mode::PostReg(sign, reg(&what.extract::<String>()?)?),
                    }
                }
                other => return unsupported_mode(other),
            };
            Ea::Ptr { n: p, mode: m, words }
        }
    })
}

fn mem(n: &Node<'_>, words: u8, unsigned: bool, io: bool) -> R<Expr> {
    let port = text(n, 1)? == "port" || io;
    Ok(Expr::Mem { ea: ea(n, words as i32)?, words, port, signed: !unsigned })
}

pub fn node(n: &Node<'_>, unsigned: bool, io: bool, mmap: bool) -> R<Expr> {
    let kind = text(n, 0)?;
    Ok(match kind.as_str() {
        "num" => Expr::Num(int(n, 1)?),
        "reg" => {
            let name = text(n, 1)?;
            if name == "Carry" || name == "Borrow" {
                return Ok(Expr::CarryIn(name == "Borrow"));
            }
            let r = reg(&name)?;
            Expr::Reg(r, reg_meta(r).1 && !unsigned)
        }
        "mmr" => Expr::Mmr { addr: int(n, 1)? as u32, words: 1, signed: !unsigned },
        "mem" => mem(n, 1, unsigned, io)?,
        "call" => call(&text(n, 1)?, &item(n, 2)?, unsigned, io, mmap)?,
        "un" => {
            let x = Rc::new(node(&item(n, 2)?, unsigned, io, mmap)?);
            match text(n, 1)?.as_str() {
                "-" => Expr::Neg(x),
                "~" => Expr::Not(x),
                _ => Expr::LNot(x),
            }
        }
        "bin" => bin(n, unsigned, io, mmap)?,
        other => return unsupported(format!("node {}", other)),
    })
}

fn call(name: &str, args: &Node<'_>, unsigned: bool, io: bool, mmap: bool) -> R<Expr> {
    let argc = args.len()?;
    let arg = |i: usize| item(args, i);
    let kind = |i: usize| -> R<String> { text(&arg(i)?, 0) };
    Ok(match (name, argc) {
        ("uns", 1) if kind(0)? == "bin" && text(&arg(0)?, 1)? == "*" => {
            // MPYMU [9], bounded to the form independently emitted by TI
            // cl55 for unsigned 16-by-16 multiplication to unsigned long.
            let n = arg(0)?;
            let a = node(&item(&n, 2)?, false, io, mmap)?;
            let b = node(&item(&n, 3)?, false, io, mmap)?;
            let temp = |x: &Expr| matches!(x, Expr::Reg(Reg::T(_), _));
            let smem = |x: &Expr| matches!(x, Expr::Mem { words: 1, port: false, .. });
            if !((temp(&a) && smem(&b)) || (smem(&a) && temp(&b))) {
                return unsupported("unsigned product other than MPYMU Tx/Smem");
            }
            Expr::UnsignedProduct(Rc::new(a), Rc::new(b))
        }
        ("sqdst_difference", 1) => {
            // SQDST reports the complement of borrow on every subtraction
            // (SWPU067E 5-582), unlike the generic shifted-SUB carry rule.
            let mut value = node(&arg(0)?, false, io, mmap)?;
            match &mut value {
                Expr::AddSub { sub: true, shifted16, .. } => *shifted16 = false,
                _ => return unsupported("SQDST difference requires subtraction"),
            }
            value
        }
        ("sqdst_square", 1) => {
            // Internal SQDST lowering only, SWPU067E pp. 5-582..583:
            // signed ACx(32:16), FRCT/SMUL, then signed 32-bit extension.
            // Reuse the qualified product path without changing generic '*'.
            let x = Rc::new(node(&arg(0)?, false, io, mmap)?);
            if !matches!(&*x, Expr::Reg(Reg::Ac(_), true)) {
                return unsupported("SQDST square requires a signed accumulator");
            }
            Expr::SignedAccumulatorProduct(Rc::clone(&x), x)
        }
        ("uns", 1) => node(&arg(0)?, true, io, mmap)?,
        ("coef", 1) if kind(0)? == "mem" => node(&arg(0)?, unsigned, io, mmap)?,   // Cmem (CDP)
        ("dbl", 1) if kind(0)? == "mem" => mem(&arg(0)?, 2, unsigned, io)?,
        ("dbl", 1) if kind(0)? == "mmr" => Expr::Mmr { addr: int(&arg(0)?, 1)? as u32, words: 2, signed: !unsigned },
        ("mar", 1) if kind(0)? == "mem" => Expr::Mar(ea(&arg(0)?, 1)?),
        ("pair", 1) if kind(0)? == "reg" => {
            let (hi, lo) = match text(&arg(0)?, 1)?.as_str() {
                "T0" => ("T0", "T1"), "T2" => ("T2", "T3"), "AR0" => ("AR0", "AR1"),
                "AR2" => ("AR2", "AR3"), "AR4" => ("AR4", "AR5"), "AR6" => ("AR6", "AR7"),
                other => return unsupported(format!("pair({})", other)),
            };
            Expr::Pair(reg(hi)?, reg(lo)?)
        }
        ("HI", 1) | ("LO", 1) if kind(0)? == "reg" => Expr::Half(Rc::new(node(&arg(0)?, false, false, false)?), name == "HI"),
        ("HI", 1) if kind(0)? == "call" => {
            // SWPU067E MOV [8]: round, saturate, then store bits 31..16.
            let x = node(&arg(0)?, false, false, false)?;
            if !matches!(&x, Expr::Rnd(_) | Expr::Saturate(_)) {
                return unsupported("HI() of an unsupported function");
            }
            Expr::Half(Rc::new(x), true)
        }
        ("HI", 1) | ("LO", 1) if kind(0)? == "bin" => {
            // Smem = HI / LO(ACx << #k): store of a shifted accumulator
            let x = node(&arg(0)?, false, false, false)?;
            if !matches!(&x, Expr::Bin { op: Op::Shl | Op::Shr, a, .. } if meta(a).2) {
                return unsupported(format!("{}() of an expression other than a shifted accumulator", name));
            }
            Expr::Half(Rc::new(x), name == "HI")
        }
        ("field_extract", 2) | ("field_expand", 2) if kind(1)? == "num" => {
            let m = int(&arg(1)?, 1)?;
            Expr::Field {
                src: Rc::new(node(&arg(0)?, unsigned, io, mmap)?),
                positions: (0..16).filter(|i| m >> i & 1 != 0).collect(),
                expand: name == "field_expand",
            }
        }
        ("low_byte", 1) | ("high_byte", 1) if kind(0)? == "mem" => {
            let a = arg(0)?;
            Expr::Byte { ea: ea(&a, 1)?, port: text(&a, 1)? == "port" || io, high: name == "high_byte",
                         signed: !unsigned, cell: Cell::new(None) }
        }
        ("min", 2) | ("max", 2) => Expr::MinMax(Rc::new(node(&arg(0)?, unsigned, io, mmap)?),
                                                Rc::new(node(&arg(1)?, unsigned, io, mmap)?), name == "max"),
        ("abs", 1) => Expr::Abs(Rc::new(node(&arg(0)?, false, false, false)?)),
        ("mant", 1) | ("mant_exp", 1) => {
            let x = node(&arg(0)?, false, false, false)?;
            if !meta(&x).2 { return unsupported(format!("{}() of a non-accumulator", name)); }
            Expr::Mant(Rc::new(x), name == "mant_exp")
        }
        ("rnd", 1) => Expr::Rnd(Rc::new(node(&arg(0)?, false, false, false)?)),
        ("saturate", 1) => Expr::Saturate(Rc::new(node(&arg(0)?, false, false, false)?)),
        ("exp", 1) => {
            let x = node(&arg(0)?, false, false, false)?;
            if !meta(&x).2 { return unsupported("exp() of a non-accumulator"); }
            Expr::Exp(Rc::new(x))
        }
        ("bit", 2) => {
            let pos = arg(1)?;
            let k = match text(&pos, 0)?.as_str() {
                "mem" if text(&pos, 1)? == "sp" => int(&pos, 3)?,
                "num" => int(&pos, 1)?,
                _ => return unsupported(format!("function {}", name)),
            };
            Expr::Bit(Rc::new(node(&arg(0)?, unsigned, io, mmap)?), k as u32)
        }
        _ => return unsupported(format!("function {}", name)),
    })
}

fn bin(n: &Node<'_>, unsigned: bool, io: bool, mmap: bool) -> R<Expr> {
    let op = text(n, 1)?;
    let (na, nb) = (item(n, 2)?, item(n, 3)?);
    let a = Rc::new(node(&na, unsigned, io, mmap)?);
    let b = Rc::new(node(&nb, unsigned, io, mmap)?);
    let simple = |op: Op, a: Rc<Expr>, b: Rc<Expr>| Expr::Bin { op, a, b, mask: None };
    Ok(match op.as_str() {
        "&" => simple(Op::And, a, b), "|" => simple(Op::Or, a, b), "^" => simple(Op::Xor, a, b),
        "<<<" | ">>>" => {
            let (w, _, acc) = meta(&a);
            let mask = if acc || w == 0 { None } else { Some((1i64 << w) - 1) };
            Expr::Bin { op: if op == "<<<" { Op::LShl } else { Op::LShr }, a, b, mask }
        }
        "==" => simple(Op::Eq, a, b), "!=" => simple(Op::Ne, a, b), "<" => simple(Op::Lt, a, b),
        "<=" => simple(Op::Le, a, b), ">" => simple(Op::Gt, a, b), ">=" => simple(Op::Ge, a, b),
        "<<" => simple(Op::Shl, a, b), ">>" => simple(Op::Shr, a, b),
        "*" => {
            let acc = |x: &Expr| matches!(x, Expr::Reg(Reg::Ac(_), true));
            let scalar = |x: &Expr| matches!(x, Expr::Reg(Reg::T(_), true)
                | Expr::Mem { words: 1, port: false, signed: true, .. });
            let smem = |x: &Expr| matches!(x,
                Expr::Mem { words: 1, port: false, signed: true, .. });
            if smem(&a) && smem(&b) && na.eq(&nb)? {
                Expr::SignedMemorySquare(a)
            } else if (acc(&a) && scalar(&b)) || (scalar(&a) && acc(&b)) {
                Expr::SignedAccumulatorProduct(a, b)
            } else {
                simple(Op::Mul, a, b)
            }
        }
        "+" | "-" => {
            let shifted16 = is_bin(&nb, "<<")? && text(&item(&nb, 2)?, 0)? == "mem"
                && text(&item(&nb, 3)?, 0)? == "num" && int(&item(&nb, 3)?, 1)? == 16;
            Expr::AddSub {
                sub: op == "-", product: is_bin(&na, "*")? || is_bin(&nb, "*")?
                    || (text(&nb, 0)? == "call" && text(&nb, 1)? == "sqdst_square"),
                chain: is_bin(&na, &op)?, shifted16, a, b, c: Rc::new(Cell::new(0)),
            }
        }
        other => return unsupported(format!("operator {}", other)),
    })
}
