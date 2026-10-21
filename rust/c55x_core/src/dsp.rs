//! D-unit operations of the DSP code (Jukebox2 MP3 decoder): SATD saturation of
//! accumulator results, fractional products, rnd() and mant / exp.

use crate::core::Core;
use crate::expr::{m40, Expr, Op};
use crate::state::{ST1_FRCT, ST1_SATD, ST2_RDM, ST3_SMUL};

/// SATD (SPRU371F, ST1): a D-unit arithmetic result written to an accumulator that
/// overflows 32 bits (M40 = 0) or 40 bits (M40 = 1) is replaced by the largest positive
/// or negative value. The overflow flags (ACOVx) are not modeled.
/// d_unit_result: is src such a result; saturate_acc: the clamp.
pub fn d_unit_result(src: &Expr) -> bool {
    matches!(src, Expr::AddSub { .. } | Expr::Neg(_) | Expr::Abs(_) | Expr::Rnd(_)
        | Expr::UnsignedProduct(..) | Expr::SignedAccumulatorProduct(..) | Expr::SignedMemorySquare(..)
        | Expr::Bin { op: Op::Mul | Op::Shl | Op::Shr, .. })
}

pub fn saturate_acc(m: &Core, v: i128) -> i128 {
    if m.st.st1 & ST1_SATD == 0 {
        return v;
    }
    let bits = if m40(m) { 40 } else { 32 };
    v.clamp(-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
}

/// rnd (SPRU375G): add 2^15 (RDM = 0) or round to nearest even (RDM = 1), then clear
/// bits 15-0.
pub fn rnd(m: &Core, v: i128) -> i128 {
    let low = v & 0xFFFF;
    let up = if m.st.st2 & ST2_RDM == 0 { true } else { low > 0x8000 || (low == 0x8000 && (v >> 16) & 1 == 1) };
    (if up { v + 0x8000 } else { v }) & !0xFFFF
}

/// ACy = mant(ACx), Tx = -exp(ACx) (SPRU375G 5-132) of the sign extended 40-bit v:
/// leading = shifts that align v on signed 40 bits; exponent: 8 - leading (8000h for
/// 0), else the mantissa: v aligned on signed 32 bits.
pub fn mant(v: i128, exponent: bool) -> i128 {
    let leading = 39 - (128 - (if v >= 0 { v } else { !v }).leading_zeros() as i128);
    match (exponent, v == 0) {
        (true, true) => 0x8000,
        (true, false) => 8 - leading,
        (false, _) => if leading >= 8 { v << (leading - 8) } else { v >> (8 - leading) },
    }
}

/// Product of two multiplier operands: FRCT shifts it left by one; with SMUL and SATD as
/// well, 8000h * 8000h gives 7FFF FFFFh (SPRU371F, ST1 / ST3).
pub fn product(m: &Core, x: i128, y: i128) -> i128 {
    let st = &m.st;
    if st.st1 & ST1_FRCT == 0 {
        x * y
    } else if x == -0x8000 && y == -0x8000 && st.st3 & ST3_SMUL != 0 && st.st1 & ST1_SATD != 0 {
        0x7FFF_FFFF
    } else {
        (x * y) << 1
    }
}
