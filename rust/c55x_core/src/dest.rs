//! Destinations: writing an expression that names a location (port of V.set).

use crate::core::Core;
use crate::err::{emu, R};
use crate::expr::{get, Expr, M32};
use crate::state::Reg;

pub fn set(m: &mut Core, e: &Expr, v: i128) -> R<()> {
    match e {
        Expr::Reg(r, _) => {
            // SPRU374E 4.70.3: a saturated Tx shift count reports overflow on the
            // destination accumulator.
            if let Reg::Ac(i) = r {
                if std::mem::take(&mut m.ovf_pending) { m.st.acov[*i as usize] = 1; }
            }
            m.st.set(*r, v)
        }
        Expr::Mem { ea, words, port, .. } => {
            let a = ea.eval(m)?;
            if *port { m.port_write(a, v)? } else if *words == 2 { m.write32(a, v & M32)? } else { m.write(a, v)? }
        }
        Expr::Mmr { addr, words, .. } => {
            if *words == 2 {
                m.st.mmr(*addr, Some(((v >> 16) & 0xFFFF) as u32))?;
                m.st.mmr(*addr ^ 1, Some((v & 0xFFFF) as u32))?;
            } else {
                m.st.mmr(*addr, Some((v & 0xFFFF) as u32))?;
            }
        }
        Expr::Pair(hi, lo) => { m.st.set(*hi, v >> 16); m.st.set(*lo, v) }
        Expr::Half(x, high) => {
            let s = if *high { 16 } else { 0 };
            let cur = get(m, x)?;
            set(m, x, (cur & !(0xFFFF << s)) | ((v & 0xFFFF) << s))?;
        }
        Expr::Byte { ea, port, high, cell, .. } => {
            let a = match cell.take() { Some(a) => a, None => ea.eval(m)? };
            let s = if *high { 8 } else { 0 };
            let old = if *port { m.port_read(a)? } else { m.read(a)? } as i128;
            let nv = (old & !(0xFF << s)) | ((v & 0xFF) << s);
            if *port { m.port_write(a, nv)? } else { m.write(a, nv)? }
        }
        _ => return emu("destination is not writable".into()),
    }
    Ok(())
}

pub fn writable(e: &Expr) -> bool {
    match e {
        Expr::Half(x, _) => writable(x),
        _ => matches!(e, Expr::Reg(..) | Expr::Mem { .. } | Expr::Mmr { .. } | Expr::Pair(..) | Expr::Byte { .. }),
    }
}
