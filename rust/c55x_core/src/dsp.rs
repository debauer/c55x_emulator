//! D-unit operations of the DSP code (Jukebox2 MP3 decoder): SATD saturation of
//! accumulator results, fractional products, rnd() and mant / exp.

use crate::core::Core;
use crate::expr::{m40, Expr, Op};
use crate::state::{ST1_FRCT, ST1_SATD, ST2_RDM, ST3_SMUL};

/// SATD (SPRU371F, ST1): a D-unit arithmetic result written to an accumulator that
/// overflows 32 bits (M40 = 0) or 40 bits (M40 = 1) is replaced by the largest positive
/// or negative value. Every such overflow reports the destination ACOVy
/// (SPRU374E 4.48.2 for the accumulate; the ADD/SUB sections agree; C54CM = 0
/// detection covers each operation, v3.x CPU 1.5.1): the pending flag set here is
/// latched into the destination accumulator at the store (fuse.rs / dest.rs).
/// d_unit_result: is src such a result; saturate_acc: the detection and clamp.
pub fn d_unit_result(src: &Expr) -> bool {
    // UnsignedProduct is exempt: SPRU374E 4.47.10/11 zero-extend the 32-bit
    // unsigned product to 40 bits - a signed-domain overflow check (and a
    // SATD clamp) would corrupt it, and u16 x u16 cannot overflow 32 bits.
    matches!(src, Expr::AddSub { .. } | Expr::Neg(_) | Expr::Abs(_) | Expr::Rnd(_)
        | Expr::SignedAccumulatorProduct(..) | Expr::SignedMemorySquare(..)
        | Expr::Bin { op: Op::Mul | Op::Shl | Op::Shr, .. })
}

pub fn saturate_acc(m: &mut Core, v: i128) -> i128 {
    let bits = if m40(m) { 40 } else { 32 };
    let (lo, hi) = (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1);
    if v < lo || v > hi {
        m.ovf_pending = true;
        if m.st.st1 & ST1_SATD != 0 {
            return v.clamp(lo, hi);
        }
    }
    v
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
pub fn product(m: &mut Core, x: i128, y: i128) -> i128 {
    if m.st.st1 & ST1_FRCT == 0 {
        return x * y;
    }
    if x == -0x8000 && y == -0x8000 && m.st.st3 & ST3_SMUL != 0 {
        // SPRU374E 4.48.2: multiplication overflow detection depends on
        // SMUL; the documented case reports the destination overflow
        m.ovf_pending = true;
        if m.st.st1 & ST1_SATD != 0 {
            return 0x7FFF_FFFF;
        }
    }
    (x * y) << 1
}
