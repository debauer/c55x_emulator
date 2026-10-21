//! Data memory and I/O space as the core sees them: MMRs in the state, peripheral and
//! code-page words through the Python bus, the rest straight in the bus bytearray.

use pyo3::intern;
use pyo3::prelude::*;

use crate::core::{Core, MEM_BYTES, MMR_END};
use crate::err::R;

impl Core {
    pub fn mem_byte(&self, byte_addr: u32) -> u8 {
        unsafe { *self.mem.add(byte_addr as usize & (MEM_BYTES - 1)) }
    }

    fn hooked(&self, word: u32) -> bool {
        word >= self.hook_lo && self.hooks.iter().any(|&(lo, hi)| lo <= word && word < hi)
    }

    pub fn read(&mut self, word: u32) -> R<u32> {
        let word = word & 0x7FFFFF;
        if word < MMR_END {
            return self.st.mmr(word, None);
        }
        if self.hooked(word) {
            return self.read_hooked(word);
        }
        let b = (2 * word) as usize;
        unsafe { Ok(((*self.mem.add(b) as u32) << 8) | *self.mem.add(b + 1) as u32) }
    }

    pub fn write(&mut self, word: u32, value: i128) -> R<()> {
        let word = word & 0x7FFFFF;
        let v = (value & 0xFFFF) as u32;
        if word < MMR_END {
            self.st.mmr(word, Some(v))?;
            return Ok(());
        }
        if self.hooked(word) || self.code_pages[(word >> 7) as usize] {
            return self.write_python(word, v);
        }
        let b = (2 * word) as usize;
        unsafe {
            *self.mem.add(b) = (v >> 8) as u8;
            *self.mem.add(b + 1) = v as u8;
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn read_hooked(&mut self, word: u32) -> R<u32> {
        if self.is_ata(word) {
            return self.ata_read(word);
        }
        self.py(|py, _cpu, bus| bus.call_method1(intern!(py, "read"), (word,))?.extract())
    }

    #[cold]
    #[inline(never)]
    fn write_python(&mut self, word: u32, v: u32) -> R<()> {
        if self.is_ata(word) {
            return self.ata_write(word, v);
        }
        self.py_writes += 1;
        self.py(|py, _cpu, bus| { bus.call_method1(intern!(py, "write"), (word, v))?; Ok(()) })
    }

    /// Count legal odd long accesses per PC when telemetry is enabled.
    #[inline(always)]
    fn odd_long(&mut self, word: u32) {
        if word & 1 != 0 {
            if let Some(m) = self.odd_longs.as_mut() {
                *m.entry(self.st.pc).or_insert(0) += 1;
            }
        }
    }

    pub fn read32(&mut self, word: u32) -> R<i128> {
        self.odd_long(word);
        // SWPU073E 3.3.2: addressed word is MSW, paired word is address XOR 1.
        Ok(((self.read(word)? as i128) << 16) | self.read(word ^ 1)? as i128)
    }

    pub fn write32(&mut self, word: u32, v: i128) -> R<()> {
        self.odd_long(word);
        self.write(word, v >> 16)?;
        self.write(word ^ 1, v)
    }

    pub fn port_read(&mut self, port: u32) -> R<u32> {
        let port = port & 0xFFFF;
        if let Some(v) = self.dma_read(port).or_else(|| self.mcbsp_read(port)) {
            return Ok(v);
        }
        self.py(|py, _cpu, bus| bus.call_method1(intern!(py, "port_read"), (port,))?.extract())
    }

    pub fn port_write(&mut self, port: u32, v: i128) -> R<()> {
        let (port, v) = (port & 0xFFFF, (v & 0xFFFF) as u32);
        if self.dma_write(port, v)? || self.mcbsp_write(port, v)? {
            return Ok(());
        }
        self.py(|py, _cpu, bus| { bus.call_method1(intern!(py, "port_write"), (port, v))?; Ok(()) })
    }
}
