//! Executing the data parts of an instruction (port of stmt.py): every part reads in the
//! read phase and writes in the commit phase; parts flagged first (address phase) do both
//! before the others.

use crate::addr::circ_step;
use crate::core::Core;
use crate::dest::set;
use crate::err::R;
use crate::expr::{arith, get, is_acc, meta, sext, Expr};
use crate::state::{Reg, AC_MASK, ST1_SATD, ST1_SXMD};
use crate::stmt::Part;

/// A part's read-phase result. A plain struct passed by reference: moving a packed enum
/// through R<_> caused store-forwarding stalls on every instruction.
#[derive(Default)]
struct Pending { v: i128, carry: Option<u8>, vs: Vec<i128> }

fn read(m: &mut Core, p: &Part, out: &mut Pending) -> R<()> {
    out.v = match p {
        Part::RegToggle(r, _) | Part::RegBit(r, _, _) | Part::HiLoad(_, r) => m.st.get(*r),
        Part::BitCount(_, a, b, _) => {
            ((m.st.get(*a) & m.st.get(*b)) as u64 & AC_MASK).count_ones() as i128
        }
        Part::Extremum(dst, source, max) => {
            let old = m.st.get(*dst);
            let value = arith(m, source, false, true, false)?;
            let bits = if matches!(dst, Reg::Ac(_)) {
                if crate::expr::m40(m) { 40 } else { 32 }
            } else { 16 };
            let (left, right) = (sext(value, bits), sext(old, bits));
            let take = if *max { left > right } else { left < right };
            out.carry = Some((!take) as u8);
            if take { value } else { old }
        }
        Part::TestSet(_, loc, _, _) => get(m, loc)?,
        Part::Subc(op, src, _) => {
            let x = m.st.get(*src);
            let divisor = arith(m, op, false, true, false)? << 15;
            // SWPU067E 5-631: carry is the complement of the bit-31 borrow,
            // independently of M40; the quotient decision still uses bit 39.
            out.carry = Some(((x & 0xFFFF_FFFF) >= (divisor & 0xFFFF_FFFF)) as u8);
            let diff = (x - divisor) & AC_MASK as i128;
            if diff >> 39 == 0 { (diff << 1) + 1 } else { x << 1 }
        }
        Part::DualAlu(_, a, b, sub) => {
            // Read each aligned Lmem once, including its two-word DAGEN update.
            // SWPU067E ADD/SUB dual: independent low16 and high24 datapaths.
            let half = |m: &mut Core, e: &Expr| -> R<(i128, i128)> {
                let value = if let Expr::Mem { ea, words: 2, port: false, .. } = e {
                    let address = ea.eval(m)?;
                    // Dual Lmem keeps its addressed word high, even at odd addresses.
                    ((m.read(address)? as i128) << 16) | m.read(address ^ 1)? as i128
                } else { get(m, e)? };
                let high = if is_acc(e) { sext(value >> 16, 24) }
                    else if m.st.st1 & ST1_SXMD != 0 { sext(value >> 16, 16) }
                    else { (value >> 16) & 0xFFFF };
                Ok((high, sext(value, 16)))
            };
            let (ah, al) = half(m, a)?;
            let (bh, bl) = half(m, b)?;
            let (mut high, mut low) = if *sub { (ah - bh, al - bl) } else { (ah + bh, al + bl) };
            out.carry = Some(if *sub { ((ah & 0xFFFF) >= (bh & 0xFFFF)) as u8 }
                else { (((ah & 0xFFFF) + (bh & 0xFFFF)) > 0xFFFF) as u8 });
            if m.st.st1 & ST1_SATD != 0 {
                high = high.clamp(-32768, 32767);
                low = low.clamp(-32768, 32767);
            }
            ((high & 0xFFFFFF) << 16) | (low & 0xFFFF)
        }
        Part::Push(items) => {
            out.vs.clear();
            for (e, _) in items { let x = get(m, e)?; out.vs.push(x) }
            0
        }
        Part::PshBoth(e) => get(m, e)?,
        Part::DblPushAcc(r) => m.st.get(*r),
        Part::MarArith(r, sign, src) => if *sign == 0 { arith(m, src, false, true, false)? } else {
            let base = get(m, r)?;
            let delta = *sign as i128 * arith(m, src, false, true, false)?;
            match r {
                Expr::Reg(Reg::Ar(n), _) => circ_step(m, *n, (base & 0xFFFF) as i64, delta as i64)? as i128,
                Expr::Reg(Reg::Cdp, _) => circ_step(m, 8, (base & 0xFFFF) as i64, delta as i64)? as i128,
                _ => base + delta,
            }
        },
        Part::Assign(a) => {
            let (v, c) = a.read(m)?;
            out.carry = c;
            v
        }
        _ => 0,
    };
    Ok(())
}

fn commit(m: &mut Core, p: &Part, pend: &mut Pending) -> R<()> {
    let v = pend.v;
    match p {
        Part::Nop => {}
        Part::BitCount(dst, _, _, tc) => {
            m.st.set(*dst, v);
            m.st.set(*tc, v & 1);
        }
        Part::Extremum(dst, _, _) => {
            m.st.set(*dst, v);
            if let Some(carry) = pend.carry { m.st.carry = carry as u32; }
        }
        Part::StatusBit(addr, bit, val) => m.st.set_status_bit(*addr, *bit, *val)?,
        Part::RegToggle(r, bit) => m.st.set(*r, v ^ (1 << bit)),
        Part::TestSet(tc, loc, bit, val) => {
            m.st.set(*tc, (v >> bit) & 1);
            set(m, loc, if *val { v | (1 << bit) } else { v & !(1 << bit) })?;
        }
        Part::RegBit(r, bit, val) => m.st.set(*r, if *val { v | (1 << bit) } else { v & !(1 << bit) }),
        Part::Subc(_, _, d) => {
            m.st.set(*d, v);
            if let Some(carry) = pend.carry { m.st.carry = carry as u32; }
        }
        Part::DualAlu(d, _, _, _) => {
            m.st.set(*d, v);
            if let Some(carry) = pend.carry { m.st.carry = carry as u32; }
        }
        Part::HiLoad(acc, _) => {
            // SPRU375E 4.5.3: TAx sign extended (per SXMD) to 40 bits and moved to ACx(31-16)
            // with the guard bits; ACx(15-0) become 0 (cl55 builds longs as HI(ACx) = hi
            // followed by ACx |= lo, see docs/2026-10-11-reconstruction_fixes_TLP-CLEAR.md)
            let hi = if m.st.st1 & ST1_SXMD != 0 { sext(v, 16) } else { v & 0xFFFF };
            m.st.set(*acc, hi << 16);
        }
        Part::Push(items) => {
            let vs = std::mem::take(&mut pend.vs);
            for ((_, double), v) in items.iter().zip(vs).rev() {
                m.push(v)?;
                if *double { m.push(v >> 16)? }
            }
        },
        Part::PshBoth(_) => { m.push(v)?; m.sys_push(v >> 16)? }
        Part::DblPushAcc(_) => { m.push(v)?; m.push(v >> 16)? }
        Part::MarArith(r, _, _) => set(m, r, v)?,
        Part::Mar(e) => { get(m, e)?; }
        Part::Assign(a) => a.commit(m, v, pend.carry)?,
        Part::PopBoth(d) => {
            let hi = m.sys_pop()? as i128;
            let lo = m.pop()? as i128;
            let v = (hi << 16) | lo;   // ACx: bit 31 sign extended (2026-10-11-reconstruction_fixes O1)
            set(m, d, if is_acc(d) { sext(v, 32) } else { v })?;
        }
        Part::Pop(ds, double) => {
            if *double {
                let width = if is_acc(&ds[0]) { 32 } else { meta(&ds[0]).0 };
                let hi = m.pop()? as i128;
                let lo = m.pop()? as i128;
                set(m, &ds[0], sext((hi << 16) | lo, width))?;
            } else {
                for d in ds { let x = m.pop()? as i128; set(m, d, x)? }
            }
        }
        Part::Loop(kind, first, last) => m.loop_start(*kind, *first, *last)?,
    }
    Ok(())
}

pub fn run_parts(m: &mut Core, parts: &[(bool, Part)]) -> R<()> {
    if let [(_, p)] = parts {                 // one part: read and commit, no buffer
        let mut v = Pending::default();
        read(m, p, &mut v)?;
        return commit(m, p, &mut v);
    }
    // read and commit in place: moving Pending values stalls store forwarding
    let mut inline: [Pending; 4] = Default::default();
    let mut heap = Vec::new();
    let pend: &mut [Pending] = if parts.len() <= inline.len() { &mut inline[..parts.len()] } else {
        heap.resize_with(parts.len(), Pending::default);
        &mut heap
    };
    for phase_first in [true, false] {
        for ((first, p), v) in parts.iter().zip(pend.iter_mut()) {
            if *first == phase_first { read(m, p, v)? }
        }
        for ((first, p), v) in parts.iter().zip(pend.iter_mut()) {
            if *first == phase_first { commit(m, p, v)? }
        }
    }
    Ok(())
}
