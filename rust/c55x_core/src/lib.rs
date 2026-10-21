//! TMS320C55x CPU core for the Python emulator (scripts/c55x_emu, see rust_core.py).

mod addr;
mod ata;
mod ata_pio;
mod compile;
mod core;
mod dma;
mod dest;
mod dsp;
mod icache;
mod err;
mod expr;
mod fast;
mod fuse;
mod mem;
mod mcbsp;
mod parts;
mod run;
mod state;
mod sync;
mod timers;
mod stmt;


use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

use crate::core::{peek_event, Core};
use crate::err::Error;

fn to_py(py: Python<'_>, e: Error) -> PyErr {
    match e {
        Error::Py(e) => e,
        Error::Emu(msg) | Error::Unsupported(msg) => match py.import("c55x_emu.registers") {
            Ok(m) => match m.getattr("EmulationError").and_then(|c| c.call1((msg,))) {
                Ok(inst) => PyErr::from_value(inst),
                Err(e) => e,
            },
            Err(e) => e,
        },
    }
}

#[pyclass(unsendable, name = "Core")]
struct PyCore {
    core: Core,
}

impl PyCore {
    /// Load the state from the Python Cpu (it is authoritative between calls).
    fn enter<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let c = &mut self.core;
        let cpu = c.cpu.bind(py).clone();
        c.load(py, &cpu)?;
        c.load_hooks(py)?;
        c.next_event = peek_event(py, &cpu)?;
        let idle = cpu.getattr("bios_idle")?;
        c.bios_idle = if idle.is_none() { None } else { Some(idle.extract()?) };
        c.poll_loops = crate::icache::PcFilter::new(cpu.getattr("poll_loops")?.extract()?);
        Ok(cpu)
    }

    /// Store the state back (also after an error, so the Python side can report it).
    fn leave<T>(&mut self, py: Python<'_>, cpu: &Bound<'_, PyAny>, r: Result<T, Error>) -> PyResult<T> {
        self.core.store(py, cpu)?;
        r.map_err(|e| to_py(py, e))
    }
}

#[pymethods]
impl PyCore {
    #[new]
    fn new(py: Python<'_>, cpu: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(PyCore { core: Core::new(py, cpu)? })
    }

    /// Cpu.run: until idle (-> True), max_steps, a pc in stop or the cycle limit until.
    #[pyo3(signature = (max_steps, stop, until=None))]
    fn run(&mut self, py: Python<'_>, max_steps: u64, stop: crate::icache::PcSet, until: Option<u64>) -> PyResult<bool> {
        let cpu = self.enter(py)?;
        let r = self.core.run(max_steps, &crate::icache::PcFilter::new(stop), until);
        self.leave(py, &cpu, r)
    }

    /// Cpu.step: one instruction (an enabled pending interrupt is taken first).
    fn step(&mut self, py: Python<'_>) -> PyResult<()> {
        let cpu = self.enter(py)?;
        let r = self.core.step();
        self.leave(py, &cpu, r)
    }

    /// Cpu.call: push the return context like a call instruction; the pc is set to target.
    fn call(&mut self, py: Python<'_>, target: u32, ret: u32) -> PyResult<()> {
        let cpu = self.enter(py)?;
        let r = self.core.call(target, ret).map(|t| self.core.st.pc = t);
        self.leave(py, &cpu, r)
    }

    /// Return from the current function like a return instruction (the pc is set), for
    /// functions replaced by Python code (machine.py hle).
    fn ret(&mut self, py: Python<'_>) -> PyResult<()> {
        let cpu = self.enter(py)?;
        let r = self.core.ret().map(|t| self.core.st.pc = t);
        self.leave(py, &cpu, r)
    }

    /// Attach the ATA disk (ata.py): disk.Disk-like object, its size, IDENTIFY words.
    fn ata_attach(&mut self, disk: Py<PyAny>, sectors: u64, identify: Vec<u16>) -> PyResult<()> {
        if identify.len() != 256 { return Err(pyo3::exceptions::PyValueError::new_err("IDENTIFY: 256 words")); }
        self.core.ata = Some(Box::new(crate::ata::Ata::new(disk, sectors, identify)));
        Ok(())
    }

    /// An ATA register access from Python (tools, tests), as the CPU would make it.
    #[pyo3(signature = (word, value=None))]
    fn ata_io(&mut self, py: Python<'_>, word: u32, value: Option<u32>) -> PyResult<u32> {
        if !self.core.is_ata(word) { return Err(pyo3::exceptions::PyValueError::new_err("not an ATA register")); }
        let cpu = self.enter(py)?;
        let r = match value { None => self.core.ata_read(word), Some(v) => self.core.ata_write(word, v).map(|_| 0) };
        self.leave(py, &cpu, r)
    }

    /// ATA counters: ({command: count}, {unmodeled access: count}).
    fn ata_stats(&self) -> PyResult<(std::collections::BTreeMap<u8, u64>, std::collections::BTreeMap<String, u64>)> {
        let a = self.core.ata.as_ref().ok_or_else(|| pyo3::exceptions::PyValueError::new_err("no ATA attached"))?;
        Ok((a.commands.clone(), a.unknown.clone()))
    }

    /// Let cycles pass without executing instructions: due core timers and Python events fire.
    fn advance(&mut self, py: Python<'_>, cycles: u64) -> PyResult<()> {
        let cpu = self.enter(py)?;
        let r = self.core.advance(cycles);
        self.leave(py, &cpu, r)
    }

    /// DMA channel n registers (0x0c00 + 0x20 n .. + 15), for debugging.
    fn dma_regs(&self, n: usize) -> PyResult<Vec<u16>> {
        if n >= 6 { return Err(pyo3::exceptions::PyValueError::new_err("DMA channel 0..5")); }
        Ok(self.core.dma_regs(n))
    }

    /// Record the last max_len executed pcs (0 switches tracing off).
    fn set_trace(&mut self, max_len: usize) {
        self.core.trace = if max_len == 0 { None } else { Some((max_len, Default::default())) };
    }

    /// The recorded pcs, oldest first.
    fn trace(&self) -> Vec<u32> {
        self.core.trace.as_ref().map(|(_, t)| t.iter().copied().collect()).unwrap_or_default()
    }

    /// The four accumulator overflow flags ACOV0..ACOV3, 0 (clear) or 1 (set) each.
    fn acov(&self) -> Vec<u32> {
        self.core.st.acov.to_vec()
    }

    /// Count long accesses at odd addresses per pc (on: start afresh, off: stop).
    fn set_odd_longs(&mut self, on: bool) {
        self.core.odd_longs = if on { Some(Default::default()) } else { None };
    }

    /// {pc: count} of the long accesses at odd addresses since set_odd_longs(True).
    fn odd_longs(&self) -> std::collections::BTreeMap<u32, u64> {
        self.core.odd_longs.clone().unwrap_or_default()
    }

    /// Counters: executed instructions, compiled instructions, Python bus writes and calls.
    fn stats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("native", self.core.native)?;
        d.set_item("compiles", self.core.compiles)?;
        d.set_item("py_writes", self.core.py_writes)?;
        d.set_item("py_calls", self.core.py_calls)?;
        d.set_item("dma_blocks", self.core.dma.blocks.to_vec())?;
        d.set_item("mcbsp_underruns", self.core.mcbsp.iter().map(|m| m.underruns).collect::<Vec<_>>())?;
        Ok(d)
    }

    /// Audio McBSP0 sent since the last call: [(sample rate, little-endian int16 L/R bytes)].
    fn take_audio<'py>(&mut self, py: Python<'py>) -> Vec<(f64, Bound<'py, PyBytes>)> {
        std::mem::take(&mut self.core.audio).into_iter()
            .map(|(rate, s)| {
                let raw: Vec<u8> = s.iter().flat_map(|v| v.to_le_bytes()).collect();
                (rate, PyBytes::new(py, &raw))
            })
            .collect()
    }
}

#[pymodule]
fn c55x_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCore>()?;
    m.add("VERSION", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
