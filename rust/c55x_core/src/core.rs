//! Core: CPU state, memory (the Python bus bytearray), Python callbacks and the compiled
//! instruction cache. Python stays authoritative between runs: run() loads the state
//! from the Python Cpu and stores it back. During a run, Python code (peripherals,
//! events) sees a synchronized view: cycles, pc and IFR before every callback.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::rc::Rc;

use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyList};

use crate::ata::{self, Ata};
use crate::dma::Dma;
use crate::err::{emu, Error, R};
use crate::icache::{ICache, PcFilter};
use crate::mcbsp::McBsp;
use crate::state::State;
use crate::stmt::{compile_insn, Insn};

pub const MEM_BYTES: usize = 1 << 24;
pub const MMR_END: u32 = 0x60;
const WINDOW_PAGES: u32 = 1024 / 256 + 2; // CodeCache.WINDOW plus margin

pub enum Body {
    Native(Insn),
    Unsupported(String),                // executing it raises EmulationError
}

pub struct Compiled {
    pub addr: u32,
    pub size: u32,
    pub text: String,
    pub body: Body,
}

pub struct Core {
    pub st: State,
    pub ovf_pending: bool, // a Tx shift count saturated during this read phase (SPRU374E 4.70.3)
    pub(crate) mem: *mut u8,
    _mem_obj: Py<PyByteArray>,
    pub cpu: Py<PyAny>,
    bus: Py<PyAny>,
    code: Py<PyAny>,
    ir_build: Py<PyAny>,
    pub(crate) hooks: Vec<(u32, u32)>,
    pub(crate) hook_lo: u32,                       // lowest hooked word: one compare on the fast path
    pub(crate) code_pages: Vec<bool>,
    cache: ICache,
    pub next_event: u64,
    pub irq_counts: [u64; 32],
    pub native: u64,
    pub compiles: u64,
    pub py_writes: u64,
    pub py_calls: u64,
    pub trace: Option<(usize, std::collections::VecDeque<u32>)>, // (max length, executed pcs)
    pub odd_longs: Option<std::collections::BTreeMap<u32, u64>>, // pc -> long accesses at odd addresses
    pub bios_idle: Option<(u32, u32)>,
    pub poll_loops: PcFilter,
    pub dma: Dma,
    pub mcbsp: [McBsp; 2],
    pub pending_sync: VecDeque<u32>,
    pub flushing: bool,
    pub timers: BinaryHeap<Reverse<(u64, usize, u32)>>, // (cycle, McBSP, generation)
    pub audio: Vec<(f64, Vec<i16>)>,                    // (sample rate, interleaved L/R)
    pub ata: Option<Box<Ata>>,                          // attached by ata.py
}

fn cfg<'py>(o: &Bound<'py, PyAny>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    o.getattr(name)
}

impl Core {
    pub fn new(py: Python<'_>, cpu: &Bound<'_, PyAny>) -> PyResult<Core> {
        let bus = cfg(cpu, "bus")?;
        let mem_any = cfg(&bus, "mem")?;
        let mem_obj = mem_any.cast::<PyByteArray>()?.clone();
        if mem_obj.len() != MEM_BYTES {
            return Err(pyo3::exceptions::PyValueError::new_err("bus memory size"));
        }
        let mem = unsafe { mem_obj.as_bytes_mut().as_mut_ptr() };
        let code = cfg(cpu, "code")?;
        let mut code_pages = vec![false; MEM_BYTES >> 8];
        for p in cfg(&code, "pages")?.try_iter()? {
            let p: i64 = p?.extract()?;
            if p >= 0 && (p as usize) < code_pages.len() {
                code_pages[p as usize] = true;
            }
        }
        let ir_build = py.import("c55x_emu.ir")?.getattr("build")?;
        Ok(Core {
            st: State::default(), ovf_pending: false, mem, _mem_obj: mem_obj.unbind(), cpu: cpu.clone().unbind(),
            bus: bus.unbind(), code: code.unbind(), ir_build: ir_build.unbind(), hooks: Vec::new(), hook_lo: u32::MAX, code_pages,
            cache: ICache::new(), next_event: u64::MAX,
            irq_counts: [0; 32], native: 0, compiles: 0, py_writes: 0, py_calls: 0, trace: None, odd_longs: None, bios_idle: None,
            poll_loops: PcFilter::default(), dma: Dma::default(), mcbsp: Default::default(),
            pending_sync: VecDeque::new(), flushing: false, timers: BinaryHeap::new(), audio: Vec::new(), ata: None,
        })
    }

    /// Re-read the hooked data ranges (peripherals register after the Cpu exists).
    pub fn load_hooks(&mut self, py: Python<'_>) -> PyResult<()> {
        self.hooks.clear();
        for h in self.bus.bind(py).getattr("data_hooks")?.try_iter()? {
            let h = h?;
            self.hooks.push((h.get_item(0)?.extract::<u32>()?, h.get_item(1)?.extract::<u32>()?));
        }
        if self.ata.is_some() {                 // served by ata.rs in the hooked path
            self.hooks.extend([(ata::BASE, ata::BASE + 8), (ata::ALT, ata::ALT + 1)]);
        }
        self.hook_lo = self.hooks.iter().map(|&(lo, _)| lo).min().unwrap_or(u32::MAX);
        Ok(())
    }

    // ---- Python callbacks --------------------------------------------------------------
    /// Call into Python with cycles, pc and IFR synchronized; picks up IFR, the next event
    /// and code invalidations afterwards.
    pub fn py<T>(&mut self, f: impl FnOnce(Python<'_>, &Bound<'_, PyAny>, &Bound<'_, PyAny>) -> PyResult<T>) -> R<T> {
        self.py_calls += 1;
        Python::attach(|py| {
            let cpu = self.cpu.bind(py).clone();
            let bus = self.bus.bind(py).clone();
            cpu.setattr(intern!(py, "cycles"), self.st.cycles)?;
            cpu.setattr(intern!(py, "pc"), self.st.pc)?;
            cpu.setattr(intern!(py, "ifr0"), self.st.ifr0)?;
            cpu.setattr(intern!(py, "ifr1"), self.st.ifr1)?;
            let r = f(py, &cpu, &bus);
            self.st.ifr0 = cpu.getattr(intern!(py, "ifr0"))?.extract()?;
            self.st.ifr1 = cpu.getattr(intern!(py, "ifr1"))?.extract()?;
            self.after_python(py, &cpu)?;
            Ok(r?)
        })
    }

    fn after_python(&mut self, py: Python<'_>, cpu: &Bound<'_, PyAny>) -> R<()> {
        self.next_event = peek_event(py, cpu)?;
        let dropped = self.code.bind(py).getattr(intern!(py, "dropped"))?;
        let dropped = dropped.cast::<PyList>().map_err(PyErr::from)?;
        if !dropped.is_empty() {
            for p in dropped.iter() {
                let p: i64 = p.extract()?;
                if p >= 0 && (p as usize) < self.code_pages.len() {
                    self.code_pages[p as usize] = false;
                }
                if p >= 0 {
                    self.cache.drop_page(p as u32);
                }
            }
            dropped.call_method0(intern!(py, "clear"))?;
        }
        Ok(())
    }

    pub fn fire_events(&mut self) -> R<()> {
        self.py(|py, cpu, _bus| { cpu.call_method0(intern!(py, "_fire_events"))?; Ok(()) })
    }

    pub fn clock_stopped(&mut self) -> R<bool> {
        self.py(|py, cpu, _bus| cpu.getattr(intern!(py, "idle_ctl"))?.call_method0("clock_stopped")?.extract())
    }

    // ---- instructions -------------------------------------------------------------------
    #[inline]
    pub fn fetch(&mut self, pc: u32) -> R<Rc<Compiled>> {
        match self.cache.get(pc) {
            Some(c) => Ok(c.clone()),
            None => self.compile_at(pc),
        }
    }

    /// Cache miss: decode and compile through ir.py.
    #[cold]
    #[inline(never)]
    fn compile_at(&mut self, pc: u32) -> R<Rc<Compiled>> {
        self.compiles += 1;
        let compiled = Python::attach(|py| -> R<Compiled> {
            let insn = self.code.bind(py).call_method1(intern!(py, "get"), (pc,))?;
            if insn.is_none() {
                return emu(format!("no decoded instruction at 0x{:06x}", pc));
            }
            let size: u32 = insn.getattr(intern!(py, "size"))?.extract()?;
            let text: String = insn.getattr(intern!(py, "text"))?.extract()?;
            let resolve = self.cpu.bind(py).getattr(intern!(py, "resolve"))?;
            let body = match self.ir_build.bind(py).call1((&insn, resolve)) {
                Err(e) => Body::Unsupported(e.to_string()),
                Ok(ir) => match compile_insn(&ir, pc, size) {
                    Ok(i) => Body::Native(i),
                    Err(Error::Unsupported(why)) => Body::Unsupported(why),
                    Err(e) => return Err(e),
                },
            };
            Ok(Compiled { addr: pc, size, text, body })
        })?;
        self.sync_code_pages(pc)?;
        let c = Rc::new(compiled);
        self.cache.insert(pc, c.clone());
        Ok(c)
    }

    /// Mirror CodeCache.pages around pc (a decode adds a window of pages): writes into
    /// pages holding decoded instructions go through the Python bus, which invalidates.
    fn sync_code_pages(&mut self, pc: u32) -> R<()> {
        Python::attach(|py| -> R<()> {
            let pages = self.code.bind(py).getattr(intern!(py, "pages"))?;
            let first = (pc >> 8).saturating_sub(1);
            for p in first..=(pc >> 8) + WINDOW_PAGES {
                if (p as usize) < self.code_pages.len() {
                    self.code_pages[p as usize] = pages.contains(p)?;
                }
            }
            Ok(())
        })
    }

    /// The idle instruction asks the idle configuration (idlectl.py): CPU domain idle?
    pub fn idle_execute(&mut self) -> R<bool> {
        let r = self.py(|py, cpu, _bus| {
            let ctl = cpu.getattr(intern!(py, "idle_ctl"))?;
            if ctl.is_none() { return Ok(None); }
            Ok(Some(ctl.call_method0("execute")?.extract::<bool>()?))
        })?;
        match r { Some(v) => Ok(v), None => emu("idle without an idle control model".into()) }
    }

    /// Run a compiled instruction; an unsupported form is an EmulationError.
    pub fn exec_compiled(&mut self, c: &Compiled) -> R<Option<u32>> {
        match &c.body {
            Body::Native(i) => crate::stmt::exec(self, i),
            Body::Unsupported(why) => emu(format!("0x{:06x}: '{}': {}", c.addr, c.text, why)),
        }
    }

    /// Only the address phase of a compiled instruction (stmt::exec_address_phase).
    pub fn exec_address_phase(&mut self, c: &Compiled) -> R<()> {
        match &c.body {
            Body::Native(i) => crate::stmt::exec_address_phase(self, i),
            Body::Unsupported(why) => emu(format!("0x{:06x}: '{}': {}", c.addr, c.text, why)),
        }
    }
}

pub fn peek_event(py: Python<'_>, cpu: &Bound<'_, PyAny>) -> PyResult<u64> {
    let events = cpu.getattr(intern!(py, "events"))?;
    if events.len()? == 0 {
        return Ok(u64::MAX);
    }
    events.get_item(0)?.get_item(0)?.extract()
}
