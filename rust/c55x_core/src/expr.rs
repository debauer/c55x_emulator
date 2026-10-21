//! Expression IR and evaluation (port of values.py). Every node has the static metadata
//! of the Python V object (width 0 = exact value, signed, accumulator); get() returns what
//! V.get returns, set() what V.set does. Operand evaluation order follows the Python
//! closures because memory operands can modify pointers.

use std::cell::Cell;
use std::rc::Rc;

use crate::addr::Ea;
use crate::core::Core;
use crate::err::{emu, R};
use crate::dsp::{mant, product, rnd, saturate_acc};
use crate::state::{reg_meta, Reg, AC_MASK, ST1_M40, ST1_SXMD, ST3_SST};

pub const M32: i128 = 0xFFFF_FFFF;
pub const ACM: i128 = AC_MASK as i128;

pub fn sext(v: i128, width: u32) -> i128 {
    let v = v & ((1i128 << width) - 1);
    if v >> (width - 1) != 0 { v - (1i128 << width) } else { v }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Op { Mul, And, Or, Xor, Shl, Shr, LShl, LShr, Eq, Ne, Lt, Le, Gt, Ge }

pub enum Expr {
    Num(i64),
    Reg(Reg, bool),                              // signed
    CarryIn(bool),                               // borrow
    Mem { ea: Ea, words: u8, port: bool, signed: bool },
    Mmr { addr: u32, words: u8, signed: bool },
    UnsignedProduct(Rc<Expr>, Rc<Expr>),
    SignedAccumulatorProduct(Rc<Expr>, Rc<Expr>),
    SignedMemorySquare(Rc<Expr>),
    Mar(Ea),
    Pair(Reg, Reg),
    Half(Rc<Expr>, bool),                       // high
    Field { src: Rc<Expr>, positions: Vec<u32>, expand: bool },
    Byte { ea: Ea, port: bool, high: bool, signed: bool, cell: Cell<Option<u32>> },
    MinMax(Rc<Expr>, Rc<Expr>, bool),          // max
    Abs(Rc<Expr>),
    Mant(Rc<Expr>, bool),                       // false: mantissa, true: -exp
    Rnd(Rc<Expr>),
    Saturate(Rc<Expr>),
    Exp(Rc<Expr>),
    Bit(Rc<Expr>, u32),
    Neg(Rc<Expr>),
    Not(Rc<Expr>),
    LNot(Rc<Expr>),
    Bin { op: Op, a: Rc<Expr>, b: Rc<Expr>, mask: Option<i64> },
    AddSub { sub: bool, a: Rc<Expr>, b: Rc<Expr>, product: bool, chain: bool, shifted16: bool, c: Rc<Cell<u8>> },
}

/// (width, signed, accumulator)
pub fn meta(e: &Expr) -> (u32, bool, bool) {
    match e {
        Expr::Num(_) => (0, true, false),
        Expr::Reg(r, signed) => { let (w, _, acc) = reg_meta(*r); (w as u32, *signed, acc) }
        Expr::CarryIn(_) | Expr::Bit(..) | Expr::LNot(_) => (1, false, false),
        Expr::Mem { words, signed, port, .. } => (if *words == 2 && !*port { 32 } else { 16 }, *signed, false),
        // an MMR access is a memory operand: sign extension per SXMD unless uns()
        Expr::Mmr { words, signed, .. } => (if *words == 2 { 32 } else { 16 }, *signed, false),
        Expr::Mar(_) => (23, false, false),
        Expr::Pair(..) => (32, false, false),
        Expr::Half(..) => (16, true, false),
        Expr::Field { .. } => (16, false, false),
        Expr::Byte { signed, .. } => (8, *signed, false),
        Expr::Not(_) => (0, false, false),
        Expr::Bin { op, .. } => match op {
            Op::And | Op::Or | Op::Xor | Op::LShl | Op::LShr => (0, false, false),
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => (1, false, false),
            _ => (0, true, false),
        },
        _ => (0, true, false),
    }
}

/// (meta(e), get(e)) with one dispatch for the common leaves.
fn meta_get(m: &mut Core, e: &Expr) -> R<((u32, bool, bool), i128)> {
    Ok(match e {
        Expr::Num(n) => ((0, true, false), *n as i128),
        Expr::Reg(r, signed) => { let (w, _, acc) = reg_meta(*r); ((w as u32, *signed, acc), m.st.get(*r)) }
        _ => (meta(e), get(m, e)?),
    })
}

pub fn sx(m: &Core) -> bool { m.st.st1 & ST1_SXMD != 0 }
pub fn m40(m: &Core) -> bool { m.st.st1 & ST1_M40 != 0 }

pub fn shifted_accumulator(e: &Expr) -> bool {
    matches!(e, Expr::Bin { op: Op::Shl | Op::Shr, a, .. } if meta(a).2)
}

fn add_operand(m: &mut Core, e: &Expr) -> R<i128> {
    let value = arith(m, e, false, true, true)?;
    // Native C55x shifted ADD/SUB saturates the intermediate shifter result.
    // C54CM is rejected by the state layer; MOV stores have separate rules.
    Ok(if shifted_accumulator(e) { saturate_acc(m, value) } else { value })
}

/// values.arith: use_sx / use_m40 tell whether the Python call passed sx / m40.
pub fn arith(m: &mut Core, e: &Expr, cmp: bool, use_sx: bool, use_m40: bool) -> R<i128> {
    let ((w, signed, acc), g) = meta_get(m, e)?;
    if w == 0 { return Ok(g); }
    if acc && cmp {
        let wide = use_m40 && m40(m);
        return Ok(if signed { sext(g, if wide { 40 } else { 32 }) } else { g & if wide { ACM } else { M32 } });
    }
    let mask = (1i128 << w) - 1;
    if signed && use_sx && !acc && !cmp {
        return Ok(if sx(m) { sext(g, w) } else { g & mask });
    }
    Ok(if signed { sext(g, w) } else { g & mask })
}

/// values.raw
pub fn raw(m: &mut Core, e: &Expr, logical_ac: bool, use_m40: bool) -> R<i128> {
    let ((w, _, acc), g) = meta_get(m, e)?;
    if w == 0 { return Ok(g); }
    if acc && logical_ac {
        return Ok(g & if use_m40 && m40(m) { ACM } else { M32 });
    }
    Ok(g & ((1i128 << w) - 1))
}

fn mac_operand(m: &mut Core, e: &Expr) -> R<i128> {
    if meta(e).2 {
        return Ok(sext(get(m, e)? >> 16, 17));
    }
    arith(m, e, false, true, false)
}

/// Python int shift; amounts beyond what 40-bit values shifted into i128 allow are errors.
pub fn shift(v: i128, n: i128) -> R<i128> {
    if n > 80 || n < -127 {
        return emu(format!("shift by {} not supported", n));
    }
    Ok(if n >= 0 { v << n } else { v >> -n })
}

/// MOV accumulator-to-memory uses the full shifter input independently of M40.
/// Keep the untruncated result until optional rounding and explicit saturation.
fn store_value(m: &mut Core, e: &Expr) -> R<i128> {
    match e {
        Expr::Reg(Reg::Ac(_), _) => {
            let value = get(m, e)?;
            Ok(if sx(m) { sext(value, 40) } else { value & ACM })
        }
        Expr::Rnd(x) => { let value = store_value(m, x)?; Ok(rnd(m, value)) }
        Expr::Saturate(x) => Ok(store_value(m, x)?.clamp(-(1i128 << 31), (1i128 << 31) - 1)),
        Expr::Bin { op: op @ (Op::Shl | Op::Shr), a, b, .. } if meta(a).2 => {
            let count = arith(m, b, false, false, false)?;
            let count = if matches!(b.as_ref(), Expr::Reg(Reg::T(_), _)) {
                // SPRU374E 4.70.3: a Tx count outside -32..+31 saturates and reports overflow
                m.tx_shift_sat = count < -32 || count > 31;
                count.clamp(-32, 31)
            } else { count };
            let value = store_value(m, a)?;
            shift(value, count * if *op == Op::Shl { 1 } else { -1 })
        }
        _ => get(m, e),
    }
}

pub fn get(m: &mut Core, e: &Expr) -> R<i128> {
    Ok(match e {
        Expr::Num(n) => *n as i128,
        Expr::Reg(r, _) => m.st.get(*r),
        Expr::CarryIn(borrow) => (m.st.carry ^ *borrow as u32) as i128,
        Expr::Mem { ea, words, port, .. } => {
            let a = ea.eval(m)?;
            if *port { m.port_read(a)? as i128 } else if *words == 2 { m.read32(a)? } else { m.read(a)? as i128 }
        }
        Expr::Mmr { addr, words, .. } => {
            if *words == 2 {
                ((m.st.mmr(*addr, None)? as i128) << 16)
                    | m.st.mmr(*addr ^ 1, None)? as i128
            } else {
                m.st.mmr(*addr, None)? as i128
            }
        }
        Expr::UnsignedProduct(a, b) => {
            // TI cl55 4.4.1 emits this exact MPYMU form for multiplication
            // of two unsigned 16-bit C values into unsigned long.
            // SWPU067E 5-442 signed-input prose contradicts that codegen.
            let x = get(m, a)? & 0xFFFF;
            let y = get(m, b)? & 0xFFFF;
            product(m, x, y) & M32
        }
        Expr::SignedAccumulatorProduct(a, b) => {
            // MPY/MPYM ACx with Tx/Smem: preserve ACx(32:16), extend the
            // 32-bit product before rounding or accumulation (SWPU067E 5-433/438).
            let operand = |m: &mut Core, e: &Expr| -> R<i128> {
                let value = get(m, e)?;
                Ok(if meta(e).2 { sext(value >> 16, 17) } else { sext(value, 16) })
            };
            let x = operand(m, a)?;
            let y = operand(m, b)?;
            sext(product(m, x, y), 32)
        }
        Expr::SignedMemorySquare(a) => {
            // SQRM/SQAM/SQSM: dis55 repeats Smem in its algebraic notation,
            // but the instruction loads and postmodifies this operand once.
            // SWPU067E 5-581/586/589: signed input independent of SXMD.
            let x = sext(get(m, a)?, 16);
            sext(product(m, x, x), 32)
        }
        Expr::Mar(ea) => ea.eval(m)? as i128,
        Expr::Pair(hi, lo) => ((m.st.get(*hi) & 0xFFFF) << 16) | (m.st.get(*lo) & 0xFFFF),
        Expr::Half(x, high) => {
            if !matches!(**x, Expr::Reg(..)) && m.st.st3 & ST3_SST != 0 {
                return emu("store of a shifted accumulator with ST3.SST (saturate on store) not modeled".into());
            }
            (store_value(m, x)? >> if *high { 16 } else { 0 }) & 0xFFFF
        }
        Expr::Field { src, positions, expand } => {
            let s = raw(m, src, true, true)?;
            let mut v = 0;
            for (i, &p) in positions.iter().enumerate() {
                v |= if *expand { ((s >> i) & 1) << p } else { ((s >> p) & 1) << i };
            }
            v
        }
        Expr::Byte { ea, port, high, cell, .. } => {
            let a = ea.eval(m)?;
            cell.set(Some(a));
            let w = if *port { m.port_read(a)? } else { m.read(a)? };
            ((w >> if *high { 8 } else { 0 }) & 0xFF) as i128
        }
        Expr::MinMax(a, b, max) => {
            let x = arith(m, a, true, true, false)?;
            let y = arith(m, b, true, true, false)?;
            if *max { x.max(y) } else { x.min(y) }
        }
        Expr::Abs(x) => arith(m, x, false, true, false)?.abs(),
        Expr::Rnd(x) => { let v = arith(m, x, false, true, true)?; rnd(m, v) }
        // saturate (SPRU375G): to 32 bits whatever M40 says
        Expr::Saturate(x) => arith(m, x, false, true, true)?.clamp(-(1i128 << 31), (1i128 << 31) - 1),
        Expr::Exp(x) => {
            let v = sext(get(m, x)?, 40);
            if v == 0 { 0 } else { 39 - (128 - (if v >= 0 { v } else { !v }).leading_zeros() as i128) - 8 }
        }
        Expr::Mant(x, exponent) => mant(sext(get(m, x)?, 40), *exponent),
        Expr::Bit(x, k) => (raw(m, x, false, false)? >> k) & 1,
        Expr::Neg(x) => -arith(m, x, false, true, false)?,
        Expr::Not(x) => !raw(m, x, false, false)?,
        Expr::LNot(x) => (get(m, x)? == 0) as i128,
        Expr::Bin { op, a, b, mask } => return bin(m, *op, a, b, *mask),
        Expr::AddSub { sub, a, b, c, .. } => {
            let va = add_operand(m, a)?;
            let vb = add_operand(m, b)?;
            let bits = if m40(m) { 40 } else { 32 };
            let msk = (1i128 << bits) - 1;
            if *sub {
                c.set(((va & msk) >= (vb & msk)) as u8);
                va - vb
            } else {
                c.set((((va & msk) + (vb & msk)) >> bits) as u8);
                va + vb
            }
        }
    })
}

fn bin(m: &mut Core, op: Op, a: &Expr, b: &Expr, mask: Option<i64>) -> R<i128> {
    Ok(match op {
        Op::And | Op::Or | Op::Xor => {
            let x = raw(m, a, false, false)?;
            let y = raw(m, b, false, false)?;
            match op { Op::And => x & y, Op::Or => x | y, _ => x ^ y }
        }
        Op::LShl | Op::LShr => {
            // a shift count in Tx is signed (-32..31) whatever SXMD says (SPRU375G)
            let count = arith(m, b, false, false, false)?;
            let count = if matches!(b, Expr::Reg(Reg::T(_), _)) { count.clamp(-32, 31) } else { count };
            let n = count * if op == Op::LShl { 1 } else { -1 };
            let v = raw(m, a, true, true)?;
            let r = shift(v, n)?;
            if meta(a).2 { r & if m40(m) { ACM } else { M32 } } else if let Some(k) = mask { r & k as i128 } else { r }
        }
        Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => {
            let x = arith(m, a, true, true, true)?;
            let y = arith(m, b, true, true, true)?;
            (match op { Op::Eq => x == y, Op::Ne => x != y, Op::Lt => x < y, Op::Le => x <= y,
                        Op::Gt => x > y, _ => x >= y }) as i128
        }
        Op::Shl | Op::Shr => {
            let count = arith(m, b, false, false, true)?;
            let count = if matches!(b, Expr::Reg(Reg::T(_), _)) {
                // SPRU374E 4.70.3: a Tx count outside -32..+31 saturates and reports overflow
                m.tx_shift_sat = count < -32 || count > 31;
                count.clamp(-32, 31)
            } else { count };
            let n = count * if op == Op::Shl { 1 } else { -1 };
            if meta(a).2 {
                let (wide, signed) = (m40(m), sx(m));
                let bits = if wide { 40 } else { 32 };
                let mut v = get(m, a)? & ((1i128 << bits) - 1);
                if signed { v = sext(v, bits) }
                let r = shift(v, n)?;
                // M40 selects input extension and overflow position, not output width.
                // The accumulator write retains 40 bits and applies SATD if enabled.
                r
            } else {
                shift(arith(m, a, false, true, true)?, n)?
            }
        }
        Op::Mul => {
            let x = mac_operand(m, a)?;
            let y = mac_operand(m, b)?;
            product(m, x, y)
        }
    })
}

/// The CARRY result of the last get() (values.add_sub carry()): None = no carry function,
/// Some(None) = unaffected.
pub fn carry_of(m: &mut Core, e: &Expr) -> R<Option<Option<u8>>> {
    if let Expr::AddSub { sub, a, product, chain, shifted16, c, .. } = e {
        if *product { return Ok(None); }
        let mut v = c.get();
        if *chain {
            if let Some(inner) = carry_of(m, a)? {
                let ic = match inner { Some(x) => x, None => return emu("chained carry of an unaffected inner operation".into()) };
                v = if *sub { v & ic } else { v | ic };
            }
        }
        if *shifted16 {
            return Ok(Some(if *sub { if v != 0 { None } else { Some(0) } } else if v != 0 { Some(1) } else { None }));
        }
        return Ok(Some(Some(v)));
    }
    Ok(None)
}

pub fn is_acc(e: &Expr) -> bool {
    meta(e).2
}
