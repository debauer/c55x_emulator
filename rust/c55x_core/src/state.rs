//! CPU registers and the memory-mapped registers (port of registers.py).

use crate::err::{emu, R};

pub const AC_MASK: u64 = (1 << 40) - 1;
pub const ST1_INTM: u32 = 1 << 11;
pub const ST1_M40: u32 = 1 << 10;
pub const ST1_SXMD: u32 = 1 << 8;
const ST1_CPL: u32 = 1 << 14;
pub const ST1_FRCT: u32 = 1 << 6;
pub const ST1_SATD: u32 = 1 << 9;
pub const ST3_SMUL: u32 = 1 << 1;
pub const ST3_SST: u32 = 1 << 0;
pub const ST2_RDM: u32 = 1 << 10;
const ST1_UNSUPPORTED: u32 = 1 << 5; // C54CM
pub const ST3_SATA: u32 = 1 << 5;

/// Plain storage registers, in registers.PLAIN order.
pub const PLAIN: [&str; 16] = [
    "trn0", "trn1", "bk03", "bk47", "bkc", "bsa01", "bsa23", "bsa45", "bsa67", "bsac", "pdp",
    "dbier0", "dbier1", "bios37", "xpc", "rptc",
];
pub const BK03: usize = 2;
pub const BK47: usize = 3;
pub const BKC: usize = 4;
pub const BSA01: usize = 5;
const RPTC: u8 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reg {
    Ac(u8), T(u8), Xar(u8), Ar(u8), Xsp, Sp, Xcdp, Cdp, Xdp, Dp, Ssp, Csr, Brc0, Brs1, Tc1,
    Tc2, Reta, Brc1, Lcrpc, Xssp, Plain(u8),
}

/// (width, signed, accumulator) as in registers.RegisterFile._register_table
pub fn reg_meta(r: Reg) -> (u8, bool, bool) {
    match r {
        Reg::Ac(_) => (40, true, true),
        Reg::T(_) | Reg::Ar(_) | Reg::Sp | Reg::Cdp | Reg::Dp => (16, true, false),
        Reg::Xar(_) | Reg::Xsp | Reg::Xcdp | Reg::Xdp | Reg::Xssp => (23, false, false),
        Reg::Tc1 | Reg::Tc2 => (1, false, false),
        Reg::Reta => (24, false, false),
        Reg::Lcrpc => (32, false, false),
        _ => (16, false, false),
    }
}

pub fn reg_by_name(name: &str) -> Option<Reg> {
    let idx = |p: &str, n: u8| -> Option<u8> {
        name.strip_prefix(p).and_then(|s| s.parse::<u8>().ok()).filter(|&i| i < n)
    };
    if let Some(i) = idx("XAR", 8) { return Some(Reg::Xar(i)); }
    if let Some(i) = idx("AR", 8) { return Some(Reg::Ar(i)); }
    if let Some(i) = idx("AC", 4) { return Some(Reg::Ac(i)); }
    if name.len() == 2 { if let Some(i) = idx("T", 4) { return Some(Reg::T(i)); } }
    Some(match name {
        "XSP" => Reg::Xsp, "SP" => Reg::Sp, "XCDP" => Reg::Xcdp, "CDP" => Reg::Cdp,
        "XDP" => Reg::Xdp, "DP" => Reg::Dp, "SSP" => Reg::Ssp, "CSR" => Reg::Csr,
        "BRC0" => Reg::Brc0, "BRS1" => Reg::Brs1, "TC1" => Reg::Tc1, "TC2" => Reg::Tc2,
        "RETA" => Reg::Reta, "RPTC" => Reg::Plain(RPTC), "BRC1" => Reg::Brc1,
        "LCRPC" => Reg::Lcrpc, "XSSP" => Reg::Xssp,
        _ => {
            let lower = name.to_ascii_lowercase();
            return PLAIN.iter().position(|p| *p == lower).map(|i| Reg::Plain(i as u8));
        }
    })
}

#[derive(Default)]
pub struct State {
    pub ac: [u64; 4],
    pub t: [u32; 4],
    pub xar: [u32; 8],
    pub xcdp: u32, pub xdp: u32, pub xsp: u32, pub ssp: u32,
    pub tc1: u32, pub tc2: u32, pub carry: u32,
    pub st1: u32, pub st2: u32, pub st3: u32,
    pub ier0: u32, pub ier1: u32, pub ifr0: u32, pub ifr1: u32,
    pub ivpd: u32, pub ivph: u32, pub dbstat: u32,
    pub brc0: u32, pub brc1: u32, pub brs1: u32, pub csr: u32,
    pub rsa: [u32; 2], pub rea: [u32; 2],
    pub loop_active: [u8; 2], // 0 none, 1 local, 2 ext (loops.py)
    pub plain: [u32; 16],
    pub reta: u32, pub cfct: u32, pub pc: u32,
    pub cycles: u64,
    pub skip_next: bool,
    pub skip_data_next: bool,              // after a false 'if (cond) execute(D_Unit)'
    pub in_idle: bool,
}

fn mask(v: i128, bits: u32) -> u32 {
    (v & ((1i128 << bits) - 1)) as u32
}

impl State {
    pub fn get(&self, r: Reg) -> i128 {
        (match r {
            Reg::Ac(i) => self.ac[i as usize],
            Reg::T(i) => self.t[i as usize] as u64,
            Reg::Xar(i) => self.xar[i as usize] as u64,
            Reg::Ar(i) => (self.xar[i as usize] & 0xFFFF) as u64,
            Reg::Xsp => self.xsp as u64, Reg::Sp => (self.xsp & 0xFFFF) as u64,
            Reg::Xcdp => self.xcdp as u64, Reg::Cdp => (self.xcdp & 0xFFFF) as u64,
            Reg::Xdp => self.xdp as u64, Reg::Dp => (self.xdp & 0xFFFF) as u64,
            Reg::Ssp => self.ssp as u64, Reg::Csr => self.csr as u64,
            Reg::Brc0 => self.brc0 as u64, Reg::Brs1 => self.brs1 as u64,
            Reg::Tc1 => self.tc1 as u64, Reg::Tc2 => self.tc2 as u64,
            Reg::Reta => self.reta as u64, Reg::Brc1 => self.brc1 as u64,
            Reg::Lcrpc => ((self.cfct as u64) << 24) | self.reta as u64,
            Reg::Xssp => ((self.xsp & 0x7F0000) | self.ssp) as u64,
            Reg::Plain(i) => self.plain[i as usize] as u64,
        }) as i128
    }

    pub fn set(&mut self, r: Reg, v: i128) {
        match r {
            Reg::Ac(i) => self.ac[i as usize] = (v & AC_MASK as i128) as u64,
            Reg::T(i) => self.t[i as usize] = mask(v, 16),
            Reg::Xar(i) => self.xar[i as usize] = mask(v, 23),
            Reg::Ar(i) => {
                let x = &mut self.xar[i as usize];
                *x = (*x & 0x7F0000) | mask(v, 16)
            }
            Reg::Xsp => self.xsp = mask(v, 23),
            Reg::Sp => self.xsp = (self.xsp & 0x7F0000) | mask(v, 16),
            Reg::Xcdp => self.xcdp = mask(v, 23),
            Reg::Cdp => self.xcdp = (self.xcdp & 0x7F0000) | mask(v, 16),
            Reg::Xdp => self.xdp = mask(v, 23),
            Reg::Dp => self.xdp = (self.xdp & 0x7F0000) | mask(v, 16),
            Reg::Ssp => self.ssp = mask(v, 16),
            Reg::Csr => self.csr = mask(v, 16),
            Reg::Brc0 => self.brc0 = mask(v, 16),
            Reg::Brs1 => self.brs1 = mask(v, 16),
            Reg::Tc1 => self.tc1 = mask(v, 1),
            Reg::Tc2 => self.tc2 = mask(v, 1),
            Reg::Reta => self.reta = mask(v, 24),
            Reg::Brc1 => { self.brc1 = mask(v, 16); self.brs1 = self.brc1 }
            Reg::Lcrpc => { self.cfct = ((v >> 24) & 0xFF) as u32; self.reta = mask(v, 24) }
            Reg::Xssp => { self.xsp = (mask(v, 23) & 0x7F0000) | (self.xsp & 0xFFFF); self.ssp = mask(v, 16) }
            Reg::Plain(i) => self.plain[i as usize] = mask(v, 16),
        }
    }

    pub fn st0(&self) -> u32 {
        (self.tc1 << 13) | (self.tc2 << 12) | (self.carry << 11) | ((self.xdp >> 7) & 0x1FF)
    }

    fn set_st0(&mut self, v: u32) {
        self.tc1 = (v >> 13) & 1;
        self.tc2 = (v >> 12) & 1;
        self.carry = (v >> 11) & 1;
        self.xdp = (self.xdp & 0x7F0000) | ((v & 0x1FF) << 7);
    }

    pub fn set_st1(&mut self, v: u32) -> R<()> {
        if v & ST1_UNSUPPORTED != 0 || v & ST1_CPL != ST1_CPL {
            return emu(format!("unsupported ST1 mode 0x{:04x} at pc 0x{:06x}", v, self.pc));
        }
        self.st1 = v;
        Ok(())
    }

    fn ac_part(&mut self, i: usize, shift: u32, m: u64, write: Option<u32>) -> u32 {
        match write {
            None => ((self.ac[i] >> shift) & m) as u32,
            Some(v) => {
                let cur = self.ac[i];
                self.ac[i] = ((cur & !(m << shift)) | ((v as u64 & m) << shift)) & AC_MASK;
                0
            }
        }
    }

    /// MMR access (registers.RegisterFile._mmr_table); write = Some(value).
    pub fn mmr(&mut self, addr: u32, write: Option<u32>) -> R<u32> {
        let addr = match addr {
            0x06 => 0x02, 0x07 => 0x03, 0x0E => 0x23, 0x1B => 0x3D, 0x1C => 0x3F, 0x1D => 0x04,
            0x4D => 0x18, a => a,
        };
        let w = write.map(|v| v & 0xFFFF);
        macro_rules! reg {
            ($r:expr) => {{
                match w { None => return Ok((self.get($r) & 0xFFFF) as u32), Some(v) => self.set($r, v as i128) }
            }};
        }
        macro_rules! field {
            ($f:expr) => {{
                match w { None => return Ok($f & 0xFFFF), Some(v) => $f = v }
            }};
        }
        macro_rules! ext {
            ($f:expr, $shift:expr, $m:expr, $total:expr) => {{
                match w {
                    None => return Ok(($f >> $shift) & $m),
                    Some(v) => $f = (($f & !($m << $shift)) | ((v & $m) << $shift)) & $total,
                }
            }};
        }
        match addr {
            0x00 => field!(self.ier0),
            0x01 => match w { None => return Ok(self.ifr0), Some(v) => self.ifr0 &= !v },
            0x02 => match w { None => return Ok(self.st0() & 0xFFFF), Some(v) => self.set_st0(v) },
            0x03 => match w { None => return Ok(self.st1), Some(v) => self.set_st1(v)? },
            0x04 => match w {
                None => return Ok(self.st3),
                Some(v) => {
                    self.st3 = v
                }
            },
            // reserved (SPRU371F table 2-2); read as 0. Jukebox2 reads it when a list
            // search runs past the end: dbl(*(null + 4)) at 0x81079c, compared to a handle
            0x05 if w.is_none() => return Ok(0),
            0x08 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x24 | 0x25 | 0x26 | 0x28 | 0x29 | 0x2A => {
                let (i, part) = match addr {
                    0x08..=0x0A => (0, addr - 0x08), 0x0B..=0x0D => (1, addr - 0x0B),
                    0x24..=0x26 => (2, addr - 0x24), _ => (3, addr - 0x28),
                };
                let (shift, m) = [(0, 0xFFFF), (16, 0xFFFF), (32, 0xFF)][part as usize];
                return Ok(self.ac_part(i, shift, m, w));
            }
            0x10..=0x17 => reg!(Reg::Ar((addr - 0x10) as u8)),
            0x20..=0x23 => reg!(Reg::T((addr - 0x20) as u8)),
            0x18 => reg!(Reg::Sp), 0x27 => reg!(Reg::Cdp), 0x2E => reg!(Reg::Dp),
            0x3B => reg!(Reg::Csr), 0x1A => reg!(Reg::Brc0), 0x39 => reg!(Reg::Brc1),
            0x3A => reg!(Reg::Brs1), 0x4C => reg!(Reg::Ssp),
            0x2B => ext!(self.xdp, 16, 0x7F, 0x7FFFFF),
            0x4E => ext!(self.xsp, 16, 0x7F, 0x7FFFFF),
            0x4F => ext!(self.xcdp, 16, 0x7F, 0x7FFFFF),
            0x3C => ext!(self.rsa[0], 16, 0xFF, 0xFFFFFF), 0x3D => ext!(self.rsa[0], 0, 0xFFFF, 0xFFFFFF),
            0x3E => ext!(self.rea[0], 16, 0xFF, 0xFFFFFF), 0x3F => ext!(self.rea[0], 0, 0xFFFF, 0xFFFFFF),
            0x40 => ext!(self.rsa[1], 16, 0xFF, 0xFFFFFF), 0x41 => ext!(self.rsa[1], 0, 0xFFFF, 0xFFFFFF),
            0x42 => ext!(self.rea[1], 16, 0xFF, 0xFFFFFF), 0x43 => ext!(self.rea[1], 0, 0xFFFF, 0xFFFFFF),
            0x0F => field!(self.plain[0]), 0x38 => field!(self.plain[1]), 0x19 => field!(self.plain[BK03]),
            0x30 => field!(self.plain[BK47]), 0x31 => field!(self.plain[BKC]),
            0x32..=0x36 => field!(self.plain[BSA01 + (addr - 0x32) as usize]),
            0x2F => field!(self.plain[10]), 0x47 => field!(self.plain[11]), 0x48 => field!(self.plain[12]),
            0x37 => field!(self.plain[13]), 0x1E => field!(self.plain[14]), 0x44 => field!(self.plain[15]),
            0x45 => field!(self.ier1),
            0x46 => match w { None => return Ok(self.ifr1), Some(v) => self.ifr1 &= !v },
            0x49 => field!(self.ivpd), 0x4A => field!(self.ivph),
            0x4B => field!(self.st2),
            _ => return emu(format!("unmodeled MMR 0x{:02x} at pc 0x{:06x}", addr, self.pc)),
        }
        Ok(0)
    }

    pub fn set_status_bit(&mut self, addr: u32, bit: u32, val: bool) -> R<()> {
        let old = self.mmr(addr, None)?;
        self.mmr(addr, Some(if val { old | (1 << bit) } else { old & !(1 << bit) }))?;
        Ok(())
    }
}
