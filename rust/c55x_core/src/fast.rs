//! Expressions compiled to closures: the same values as expr::get / arith / raw, with the
//! static decisions (width, signedness, accumulator, operator) taken once at compile
//! time instead of on every evaluation. Forms without a closure here evaluate their
//! subtree with expr::get. Evaluation order matches expr.rs (memory operands can modify
//! pointers).

use std::rc::Rc;

use crate::core::Core;
use crate::dsp::{product, rnd, saturate_acc};
use crate::err::R;
use crate::expr::{get, m40, meta, shift, shifted_accumulator, sext, sx, Expr, Op, ACM, M32};
use crate::expr::shift_acc;
use crate::state::Reg;

pub type Fx = Box<dyn Fn(&mut Core) -> R<i128>>;

fn tree(e: &Rc<Expr>) -> Fx {
    let e = e.clone();
    Box::new(move |m| get(m, &e))
}

fn reg(r: Reg) -> Fx {
    match r {
        Reg::Ac(i) => Box::new(move |m| Ok(m.st.ac[i as usize] as i128)),
        Reg::T(i) => Box::new(move |m| Ok(m.st.t[i as usize] as i128)),
        Reg::Ar(i) => Box::new(move |m| Ok((m.st.xar[i as usize] & 0xFFFF) as i128)),
        _ => Box::new(move |m| Ok(m.st.get(r))),
    }
}

/// expr::get
pub fn get_fx(e: &Rc<Expr>) -> Fx {
    match &**e {
        Expr::Num(n) => { let n = *n as i128; Box::new(move |_| Ok(n)) }
        Expr::Reg(r, _) => reg(*r),
        Expr::Mem { ea, words, port, .. } => {
            let ea = *ea;
            match (*port, *words) {
                (true, _) => Box::new(move |m| { let a = ea.eval(m)?; Ok(m.port_read(a)? as i128) }),
                (false, 2) => Box::new(move |m| { let a = ea.eval(m)?; m.read32(a) }),
                _ => Box::new(move |m| { let a = ea.eval(m)?; Ok(m.read(a)? as i128) }),
            }
        }
        Expr::Mar(ea) => { let ea = *ea; Box::new(move |m| Ok(ea.eval(m)? as i128)) }
        Expr::MinMax(a, b, max) => {
            let (fa, fb, max) = (arith_fx(a, true, true, false), arith_fx(b, true, true, false), *max);
            Box::new(move |m| { let x = fa(m)?; let y = fb(m)?; Ok(if max { x.max(y) } else { x.min(y) }) })
        }
        Expr::Abs(x) => { let f = arith_fx(x, false, true, false); Box::new(move |m| Ok(f(m)?.abs())) }
        Expr::Rnd(x) => { let f = arith_fx(x, false, true, true); Box::new(move |m| { let v = f(m)?; Ok(rnd(m, v)) }) }
        Expr::Saturate(x) => {
            let f = arith_fx(x, false, true, true);
            Box::new(move |m| Ok(f(m)?.clamp(-(1i128 << 31), (1i128 << 31) - 1)))
        }
        Expr::Bit(x, k) => { let (f, k) = (raw_fx(x, false, false), *k); Box::new(move |m| Ok((f(m)? >> k) & 1)) }
        Expr::Neg(x) => { let f = arith_fx(x, false, true, false); Box::new(move |m| Ok(-f(m)?)) }
        Expr::Not(x) => { let f = raw_fx(x, false, false); Box::new(move |m| Ok(!f(m)?)) }
        Expr::LNot(x) => { let f = get_fx(x); Box::new(move |m| Ok((f(m)? == 0) as i128)) }
        Expr::Bin { op, a, b, mask } => bin_fx(*op, a, b, *mask),
        Expr::AddSub { sub, a, b, c, .. } => {
            let (fa, fb) = (add_operand_fx(a), add_operand_fx(b));
            let (sub, c) = (*sub, c.clone());
            Box::new(move |m| {
                let va = fa(m)?;
                let vb = fb(m)?;
                let bits = if m40(m) { 40 } else { 32 };
                let msk = (1i128 << bits) - 1;
                Ok(if sub {
                    c.set(((va & msk) >= (vb & msk)) as u8);
                    va - vb
                } else {
                    c.set((((va & msk) + (vb & msk)) >> bits) as u8);
                    va + vb
                })
            })
        }
        _ => tree(e),
    }
}

/// expr::arith
fn add_operand_fx(e: &Rc<Expr>) -> Fx {
    let value = arith_fx(e, false, true, true);
    if shifted_accumulator(e) {
        Box::new(move |m| {
            let v = value(m)?;
            Ok(saturate_acc(m, v))
        })
    } else { value }
}

/// expr::arith
pub fn arith_fx(e: &Rc<Expr>, cmp: bool, use_sx: bool, use_m40: bool) -> Fx {
    let (w, signed, acc) = meta(e);
    let g = get_fx(e);
    if w == 0 {
        return g;
    }
    if acc && cmp {
        return if signed {
            Box::new(move |m| { let v = g(m)?; Ok(sext(v, if use_m40 && m40(m) { 40 } else { 32 })) })
        } else {
            Box::new(move |m| { let v = g(m)?; Ok(v & if use_m40 && m40(m) { ACM } else { M32 }) })
        };
    }
    let mask = (1i128 << w) - 1;
    if signed && use_sx && !acc && !cmp {
        return Box::new(move |m| { let v = g(m)?; Ok(if sx(m) { sext(v, w) } else { v & mask }) });
    }
    if signed { Box::new(move |m| Ok(sext(g(m)?, w))) } else { Box::new(move |m| Ok(g(m)? & mask)) }
}

/// expr::raw
pub fn raw_fx(e: &Rc<Expr>, logical_ac: bool, use_m40: bool) -> Fx {
    let (w, _, acc) = meta(e);
    let g = get_fx(e);
    if w == 0 {
        return g;
    }
    if acc && logical_ac {
        return Box::new(move |m| { let v = g(m)?; Ok(v & if use_m40 && m40(m) { ACM } else { M32 }) });
    }
    let mask = (1i128 << w) - 1;
    Box::new(move |m| Ok(g(m)? & mask))
}

/// expr::mac_operand
fn mac_fx(e: &Rc<Expr>) -> Fx {
    if meta(e).2 {
        let g = get_fx(e);
        return Box::new(move |m| Ok(sext(g(m)? >> 16, 17)));
    }
    arith_fx(e, false, true, false)
}

/// expr::bin
fn bin_fx(op: Op, a: &Rc<Expr>, b: &Rc<Expr>, mask: Option<i64>) -> Fx {
    match op {
        Op::And | Op::Or | Op::Xor => {
            let (x, y) = (raw_fx(a, false, false), raw_fx(b, false, false));
            match op {
                Op::And => Box::new(move |m| { let p = x(m)?; Ok(p & y(m)?) }),
                Op::Or => Box::new(move |m| { let p = x(m)?; Ok(p | y(m)?) }),
                _ => Box::new(move |m| { let p = x(m)?; Ok(p ^ y(m)?) }),
            }
        }
        Op::LShl | Op::LShr => {
            // a shift count in Tx is signed (-32..31) whatever SXMD says (SPRU375G)
            let (fnn, fv) = (arith_fx(b, false, false, false), raw_fx(a, true, true));
            let sign = if op == Op::LShl { 1 } else { -1 };
            let tx = matches!(b.as_ref(), Expr::Reg(Reg::T(_), _));
            let acc = meta(a).2;
            Box::new(move |m| {
                let count = fnn(m)?;
                let n = (if tx { count.clamp(-32, 31) } else { count }) * sign;
                let v = fv(m)?;
                let r = shift(v, n)?;
                Ok(if acc { r & if m40(m) { ACM } else { M32 } } else if let Some(k) = mask { r & k as i128 } else { r })
            })
        }
        Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let (x, y) = (arith_fx(a, true, true, true), arith_fx(b, true, true, true));
            Box::new(move |m| {
                let (p, q) = (x(m)?, y(m)?);
                Ok((match op { Op::Eq => p == q, Op::Ne => p != q, Op::Lt => p < q, Op::Le => p <= q,
                               Op::Gt => p > q, _ => p >= q }) as i128)
            })
        }
        Op::Shl | Op::Shr => {
            let fnn = arith_fx(b, false, false, true);
            let sign = if op == Op::Shl { 1 } else { -1 };
            let tx = matches!(b.as_ref(), Expr::Reg(Reg::T(_), _));
            if meta(a).2 {
                let g = get_fx(a);
                Box::new(move |m| {
                    let count = fnn(m)?;
                    let n = if tx {
                        // SPRU374E 4.70.3: a Tx count outside -32..+31 saturates and reports overflow
                        m.shift_ovf = count < -32 || count > 31;
                        count.clamp(-32, 31)
                    } else { count } * sign;
                    let value = g(m)?;
                    shift_acc(m, value, n)
                })
            // SPRU374E 4.70.3: a Tx count outside -32..+31 saturates and reports overflow
            } else if tx {
                let fa = arith_fx(a, false, true, true);
                Box::new(move |m| {
                    let count = fnn(m)?;
                    m.shift_ovf = count < -32 || count > 31;
                    shift(fa(m)?, count.clamp(-32, 31) * sign)
                })
            } else {
                let fa = arith_fx(a, false, true, true);
                Box::new(move |m| shift(fa(m)?, fnn(m)? * sign))
            }
        }
        Op::Mul => {
            let (x, y) = (mac_fx(a), mac_fx(b));
            Box::new(move |m| { let p = x(m)?; let q = y(m)?; Ok(product(m, p, q)) })
        }
    }
}
