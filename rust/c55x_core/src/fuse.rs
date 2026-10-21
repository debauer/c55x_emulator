//! Assignments compiled once: source closure (fast.rs), destination closure and the
//! static SATD / CARRY / TCx decisions. The most common instruction shape, one
//! assignment without condition, flow or repeat, runs as a single closure without the
//! generic part machinery; the order is that of parts.rs read / commit.

use std::rc::Rc;

use crate::core::Core;
use crate::dest::set;
use crate::dsp::{d_unit_result, saturate_acc};
use crate::err::R;
use crate::expr::{carry_of, is_acc, m40, sext, Expr, Op, M32};
use crate::fast::{arith_fx, raw_fx, Fx};
use crate::state::{Reg, AC_MASK, ST3_SATA};

pub type Run = Box<dyn Fn(&mut Core) -> R<()>>;
type Store = Box<dyn Fn(&mut Core, i128) -> R<()>>;

/// dest::set
fn store_fx(d: Expr) -> Store {
    match d {
        Expr::Reg(Reg::Ac(i), _) => Box::new(move |m, v| { m.st.ac[i as usize] = (v & AC_MASK as i128) as u64; Ok(()) }),
        Expr::Reg(r, _) => Box::new(move |m, v| { m.st.set(r, v); Ok(()) }),
        Expr::Mem { ea, words, port, .. } => match (port, words) {
            (true, _) => Box::new(move |m, v| { let a = ea.eval(m)?; m.port_write(a, v) }),
            (false, 2) => Box::new(move |m, v| { let a = ea.eval(m)?; m.write32(a, v & M32) }),
            _ => Box::new(move |m, v| { let a = ea.eval(m)?; m.write(a, v) }),
        },
        d => { let d = Rc::new(d); Box::new(move |m, v| set(m, &d, v)) }
    }
}

/// dst = src: carry when an accumulator gets a non-product add / subtract, test when
/// the destination is TC1 / TC2 (value != 0).
pub struct Assign {
    src: Rc<Expr>,
    fx: Fx,
    store: Store,
    sat: bool,                  // SATD applies: accumulator destination, D-unit result
    sat_a: bool,
    carry: bool,
    unary_carry: Option<bool>, // Some(true): accumulator; Some(false): 16-bit ALU
    logical_carry: Option<Fx>,
    test: bool,
}

impl Assign {
    pub fn new(dst: Expr, src: Expr) -> Assign {
        let register_alu = matches!(&dst, Expr::Reg(Reg::Ac(_) | Reg::T(_) | Reg::Ar(_), _));
        let carry = register_alu && matches!(src, Expr::AddSub { product: false, .. });
        let test = matches!(dst, Expr::Reg(Reg::Tc1, _) | Expr::Reg(Reg::Tc2, _));
        let sat = is_acc(&dst) && d_unit_result(&src);
        let sat_a = matches!(&dst, Expr::Reg(Reg::T(_) | Reg::Ar(_), _)) && d_unit_result(&src);
        let acc = is_acc(&dst);
        let unary_carry = match &src {
            Expr::Abs(_) if acc => Some(true),
            Expr::Neg(_) if acc || sat_a => Some(acc),
            _ => None,
        };
        let logical_carry: Option<Fx> = match &src {
            Expr::Bin { op: op @ (Op::LShl | Op::LShr), a, b, .. }
                if register_alu && matches!(a.as_ref(), Expr::Reg(..))
                    && matches!(b.as_ref(), Expr::Num(_) | Expr::Reg(Reg::T(_), _)) => {
                let value = raw_fx(a, true, true);
                let count = arith_fx(b, false, false, false);
                let acc_source = is_acc(a);
                let tx_count = matches!(b.as_ref(), Expr::Reg(Reg::T(_), _));
                let direction = if *op == Op::LShl { 1 } else { -1 };
                Some(Box::new(move |m| {
                    let bits = if acc_source { if m40(m) { 40 } else { 32 } } else { 16 };
                    let v = value(m)?;
                    let n = count(m)?;
                    let n = (if tx_count { n.clamp(-32, 31) } else { n }) * direction;
                    if n == 0 { return Ok(0); }
                    let bit = if n > 0 { bits - n } else { -n - 1 };
                    Ok(if bit < 0 || bit >= bits { 0 } else { (v >> bit) & 1 })
                }))
            }
            _ => None,
        };
        let src = Rc::new(src);
        // ABS sign selection depends on the destination ALU and M40, while
        // accumulator operations retain all 40 source bits (SWPU067E 5-11).
        let fx: Fx = match src.as_ref() {
            Expr::AddSub { sub, a, b, c, product: false, .. } if sat_a => {
                // SWPU067E ADD/SUB: A-unit arithmetic uses signed low16,
                // independently of the D-unit SXMD/M40 settings.
                let (left, right) = (arith_fx(a, false, false, false),
                                     arith_fx(b, false, false, false));
                let (sub, carry) = (*sub, c.clone());
                Box::new(move |m| {
                    let a = sext(left(m)?, 16);
                    let b = sext(right(m)?, 16);
                    Ok(if sub {
                        carry.set(((a & 0xFFFF) >= (b & 0xFFFF)) as u8);
                        a - b
                    } else {
                        carry.set((((a & 0xFFFF) + (b & 0xFFFF)) >> 16) as u8);
                        a + b
                    })
                })
            }
            Expr::Abs(x) if acc => {
                let operand = arith_fx(x, false, true, false);
                Box::new(move |m| {
                    let v = operand(m)?;
                    let sign = if m40(m) { 39 } else { 31 };
                    Ok(if v & (1i128 << sign) != 0 { -v } else { v })
                })
            }
            Expr::Abs(x) if sat_a => {
                let operand = arith_fx(x, false, true, false);
                Box::new(move |m| Ok(sext(operand(m)?, 16).abs()))
            }
            Expr::Neg(x) if sat_a => {
                let operand = arith_fx(x, false, true, false);
                Box::new(move |m| Ok(-sext(operand(m)?, 16)))
            }
            _ => arith_fx(&src, false, true, false),
        };
        Assign { fx, src, store: store_fx(dst), sat, sat_a, carry, unary_carry, logical_carry, test }
    }

    /// Read phase: (value, CARRY result if the assignment sets it).
    #[inline]
    pub fn read(&self, m: &mut Core) -> R<(i128, Option<u8>)> {
        let v = (self.fx)(m)?;
        let v = if self.test { (v != 0) as i128 } else { v };
        let carry = if let Some(carry) = &self.logical_carry {
            Some(carry(m)? as u8)
        } else if let Some(acc) = self.unary_carry {
            let bits = if acc { if m40(m) { 40 } else { 32 } } else { 16 };
            Some(((v & ((1i128 << bits) - 1)) == 0) as u8)
        } else if self.carry {
            carry_of(m, &self.src)?.flatten()
        } else { None };
        // Read ALU modes with operands, before parallel X-phase writes.
        let v = if self.sat_a && m.st.st3 & ST3_SATA != 0 {
            v.clamp(-32768, 32767)
        } else { v };
        let v = if self.sat { saturate_acc(m, v) } else { v };
        Ok((v, carry))
    }

    /// Commit phase.
    #[inline]
    pub fn commit(&self, m: &mut Core, v: i128, carry: Option<u8>) -> R<()> {
        (self.store)(m, v)?;
        if let Some(c) = carry { m.st.carry = c as u32 }
        Ok(())
    }

    /// The assignment as a whole instruction.
    pub fn run(self) -> Run {
        Box::new(move |m| {
            let (v, c) = self.read(m)?;
            self.commit(m, v, c)
        })
    }
}

/// Up to four assignments run as one instruction, phase by phase as parts::run_parts:
/// the address-phase (first) ones read and commit before the others.
pub fn run_all(mut list: Vec<(bool, Assign)>) -> Run {
    assert!(!list.is_empty() && list.len() <= 4);
    if list.len() == 1 {
        if let Some((_, a)) = list.pop() {
            return a.run();
        }
    }
    Box::new(move |m| {
        for phase_first in [true, false] {
            let (mut vs, mut cs) = ([0i128; 4], [None; 4]);
            for (k, (first, a)) in list.iter().enumerate() {
                if *first == phase_first { (vs[k], cs[k]) = a.read(m)?; }
            }
            for (k, (first, a)) in list.iter().enumerate() {
                if *first == phase_first { a.commit(m, vs[k], cs[k])?; }
            }
        }
        Ok(())
    })
}
