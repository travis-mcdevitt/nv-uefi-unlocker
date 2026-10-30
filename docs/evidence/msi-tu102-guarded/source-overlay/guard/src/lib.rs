//! File/byte-only policy shared by host tests and the guarded UEFI build.
//! No PCI/MMIO writes, firmware execution, filesystem access, or UEFI calls.
#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use core::ops::Range;
use sha2::{Digest, Sha256};

pub const LOCATION: [usize; 4] = [0, 0x26, 0, 0];
pub const PCI_SHA: &str = "2fd04d5696dee29d4fb7d4bed72a3120eead2180b77f02e6008cc674f4115c4f";
pub const FWSEC_SHA: &str = "d54e1512845a091631abf374a0b748a226203bb0cf9ee4d7b0316023a957ea6f";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Bounds,
    Format,
    Ambiguous,
    Identity,
    Warm,
    Hash,
    Geometry,
    Policy,
    Read,
    Asset,
}
type Result<T> = core::result::Result<T, Error>;

fn bytes(d: &[u8], o: usize, n: usize) -> Result<&[u8]> {
    d.get(o..o.checked_add(n).ok_or(Error::Bounds)?)
        .ok_or(Error::Bounds)
}
fn u16le(d: &[u8], o: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes(d, o, 2)?.try_into().map_err(|_| Error::Bounds)?,
    ))
}
fn u32le(d: &[u8], o: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes(d, o, 4)?.try_into().map_err(|_| Error::Bounds)?,
    ))
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or(Error::Bounds)
}

pub fn hash_matches(data: &[u8], expected: &str) -> bool {
    let digest = Sha256::digest(data);
    let hex = b"0123456789abcdef";
    expected.len() == 64
        && digest.iter().enumerate().all(|(i, b)| {
            expected.as_bytes()[i * 2] == hex[(b >> 4) as usize]
                && expected.as_bytes()[i * 2 + 1] == hex[(b & 15) as usize]
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub view: Range<usize>,
    pub descriptor: usize,
    pub image: Range<usize>,
    pub imem_size: u32,
    pub imem_pa: u32,
    pub imem_va: u32,
    pub sec_base: u32,
    pub sec_size: u32,
    pub data_off: u32,
    pub dmem_size: u32,
    pub dmem_pa: u32,
    pub entry: u32,
    pub interface: u32,
}

/// NVIDIA 610.43.03 IFR v3 and PCI/NPDS/NPDE definitions. No heuristic scan
/// for a convenient ROM header or pointer base; root directory selects the view.
pub fn parse(raw: &[u8]) -> Result<Parsed> {
    if raw.len() > 1024 * 1024 || raw.len() < 512 {
        return Err(Error::Bounds);
    }
    let start = if raw.get(..4) == Some(b"NVGI") {
        if (u32le(raw, 4)? >> 8) & 255 != 3 {
            return Err(Error::Format);
        }
        let total = (u32le(raw, 8)? & 0xfffff) as usize;
        let directory = add(u32le(raw, total)? as usize, 4096)?;
        if bytes(raw, directory, 4)? != b"RFRD" {
            return Err(Error::Format);
        }
        let start = u32le(raw, add(directory, 8)?)? as usize;
        if start < add(directory, 12)? || start & 3 != 0 {
            return Err(Error::Bounds);
        }
        start
    } else {
        0
    };
    let tail = raw.get(start..).ok_or(Error::Bounds)?;
    let mut current = 0usize;
    let mut base_size = None;
    let mut expansion = None;
    let mut end = None;
    for _ in 0..16 {
        if ![0xaa55, 0x4e56, 0xbb77].contains(&u16le(tail, current)?) {
            return Err(Error::Format);
        }
        let pd = add(current, u16le(tail, add(current, 24)?)? as usize)?;
        let h = bytes(tail, pd, 24)?;
        if ![b"PCIR", b"NPDS", b"RGIS"].contains(&h[..4].try_into().map_err(|_| Error::Format)?) {
            return Err(Error::Format);
        }
        let original = u16le(h, 16)? as usize * 512;
        let mut size = original;
        let mut last = h[21] & 128 != 0;
        let ext = add(add(pd, u16le(h, 10)? as usize)?, 15)? & !15;
        if tail.get(ext..add(ext, 4)?) == Some(b"NPDE") {
            if ![0x100, 0x101].contains(&u16le(tail, add(ext, 4)?)?) {
                return Err(Error::Format);
            }
            let len = u16le(tail, add(ext, 6)?)? as usize;
            if len < 10 {
                return Err(Error::Format);
            }
            bytes(tail, ext, len)?;
            size = u16le(tail, add(ext, 8)?)? as usize * 512;
            if len >= 11 {
                last = tail[ext + 10] & 128 != 0;
            } else if size < original {
                last = false;
            }
        }
        let next = add(current, size)?;
        if size == 0 || add(pd, 24)? > next {
            return Err(Error::Bounds);
        }
        bytes(tail, current, size)?;
        if h[20] == 0 && base_size.is_none() {
            base_size = Some(size);
        }
        if h[20] == 0xe0 && expansion.is_none() {
            expansion = Some(current);
        }
        if last {
            end = Some(next);
            break;
        }
        current = next;
    }
    let len = end.ok_or(Error::Format)?;
    let base_size = base_size.ok_or(Error::Format)?;
    let adjust = expansion
        .ok_or(Error::Format)?
        .checked_sub(base_size)
        .ok_or(Error::Bounds)?;
    let view = bytes(tail, 0, len)?;
    let mut bit = None;
    for at in 0..base_size.saturating_sub(11) {
        if view.get(at..at + 6) != Some(b"\xff\xb8BIT\0") {
            continue;
        }
        let hs = view[at + 8] as usize;
        if hs < 12 || add(at, hs)? > base_size {
            continue;
        }
        if bytes(view, at, hs)?
            .iter()
            .fold(0u8, |s, v| s.wrapping_add(*v))
            != 0
        {
            continue;
        }
        if bit.replace(at).is_some() {
            return Err(Error::Ambiguous);
        }
    }
    let bit = bit.ok_or(Error::Format)?;
    let hdr = view[bit + 8] as usize;
    let stride = view[bit + 9] as usize;
    let count = view[bit + 10] as usize; // +11 is checksum, NOT count's high byte.
    if ![6, 8].contains(&stride) {
        return Err(Error::Format);
    }
    let tokens = bytes(view, add(bit, hdr)?, stride * count)?;
    let mut table = None;
    for token in tokens.chunks_exact(stride) {
        if token[..2] != [0x70, 2] {
            continue;
        }
        let data_size = u16le(token, 2)? as usize;
        if data_size < 4 {
            return Err(Error::Format);
        }
        let pointer = if stride == 6 {
            u16le(token, 4)? as usize
        } else {
            u32le(token, 4)? as usize
        };
        let token_data = bytes(view, pointer, data_size)?;
        let t = add(adjust, u32le(token_data, 0)? as usize)?;
        if table.replace(t).is_some() {
            return Err(Error::Ambiguous);
        }
    }
    let table = table.ok_or(Error::Format)?;
    let h = bytes(view, table, 6)?;
    if h[0] != 1 || h[1] < 6 || h[2] < 6 || !(1..=32).contains(&h[3]) {
        return Err(Error::Format);
    }
    let entries = bytes(
        view,
        add(table, h[1] as usize)?,
        h[2] as usize * h[3] as usize,
    )?;
    let mut desc = None;
    for e in entries.chunks_exact(h[2] as usize) {
        if e[0] != 0x85 {
            continue;
        }
        let pointer = u32le(e, 2)? as usize;
        if pointer == 0 {
            return Err(Error::Format);
        }
        if desc.replace(add(adjust, pointer)?).is_some() {
            return Err(Error::Ambiguous);
        }
    }
    let desc = desc.ok_or(Error::Format)?;
    let d = bytes(view, desc, 60)?;
    let vd = u32le(d, 0)?;
    if vd & 1 == 0 || (vd >> 8) & 255 != 2 || vd >> 16 != 60 {
        return Err(Error::Format);
    }
    let stored = u32le(d, 4)? as usize;
    if stored == 0 || stored != u32le(d, 8)? as usize {
        return Err(Error::Geometry);
    }
    let imem = u32le(d, 24)?;
    let data_off = u32le(d, 40)?;
    let dmem = u32le(d, 48)?;
    let sec_base = u32le(d, 32)?;
    let sec_size = u32le(d, 36)?;
    let interface = u32le(d, 16)?;
    if imem == 0
        || dmem == 0
        || data_off != imem
        || add(imem as usize, dmem as usize)? != stored
        || sec_base == 0
        || add(sec_base as usize, sec_size as usize)? != imem as usize
        || add(interface as usize, 4)? > dmem as usize
    {
        return Err(Error::Geometry);
    }
    let image = add(desc, 60)?;
    bytes(view, image, stored)?;
    Ok(Parsed {
        view: start..add(start, len)?,
        descriptor: add(start, desc)?,
        image: add(start, image)?..add(add(start, image)?, stored)?,
        imem_size: imem,
        imem_pa: u32le(d, 20)?,
        imem_va: u32le(d, 28)?,
        sec_base,
        sec_size,
        data_off,
        dmem_size: dmem,
        dmem_pa: u32le(d, 44)?,
        entry: u32le(d, 12)?,
        interface,
    })
}

pub fn validate_rom(raw: &[u8]) -> Result<Parsed> {
    let p = parse(raw)?;
    if !hash_matches(&raw[p.view.clone()], PCI_SHA)
        || !hash_matches(&raw[p.image.clone()], FWSEC_SHA)
    {
        return Err(Error::Hash);
    }
    if (
        p.imem_size,
        p.sec_base,
        p.sec_size,
        p.data_off,
        p.dmem_size,
        p.interface,
    ) != (0x9a00, 0x400, 0x9600, 0x9a00, 0x3f0, 0xe0)
        || [p.imem_pa, p.imem_va, p.dmem_pa, p.entry] != [0; 4]
    {
        return Err(Error::Geometry);
    }
    Ok(p)
}

#[derive(Debug, Clone, Copy)]
pub struct Identity {
    pub location: [usize; 4],
    pub vendor: u16,
    pub device: u16,
    pub subvendor: u16,
    pub subsystem: u16,
    pub command: u16,
    pub pmcsr: u16,
    pub chip: u32,
    pub capacity: u32,
    pub wpr_hi: u32,
    pub gsp: u32,
    pub sec2: u32,
}
pub fn halted(value: u32) -> bool {
    value != u32::MAX && value & 0xffff0000 != 0xbadf0000 && value & 0x10 != 0
}
pub fn validate_identity(i: &Identity) -> Result<()> {
    if i.location != LOCATION
        || (i.vendor, i.device, i.subvendor, i.subsystem) != (0x10de, 0x1e07, 0x1462, 0x371d)
        || i.chip != 0x162000a1
        || i.capacity != 0x2c9
        || i.command == u16::MAX
        || i.command & 2 == 0
    {
        return Err(Error::Identity);
    }
    if i.pmcsr & 3 != 0 || i.wpr_hi != 0 || !halted(i.gsp) || !halted(i.sec2) {
        return Err(Error::Warm);
    }
    Ok(())
}

/// All optional write experiments other than the explicitly chosen SEC2 A
/// sequence are prohibited. Live code also compiles those alternate paths out.
pub const FORBIDDEN_MARKERS: &[&str] = &[
    "\\i_know_what_i_am_doing.txt",
    "\\pre-flr-on.txt",
    "\\tx-force-continue.txt",
    "\\tx-low-staging.txt",
    "\\tx-gop-disconnect.txt",
    "\\tx-meta-in-vram.txt",
    "\\tx-host-probe.txt",
    "\\tx-ptimer-seed.txt",
    "\\tu10x-no-ss.txt",
    "\\tu10x-fb-gb.txt",
    "\\chain-target.txt",
];
pub const RUN_MARKER: &str = "\\guarded-msi-run.txt";
pub const RUN_CONTENT: &[u8] = b"MSI-371D-90.02.42.00.35-SEC2-A";
pub fn validate_policy(run: Option<&[u8]>, c_load: bool, forbidden: bool) -> Result<()> {
    if run != Some(RUN_CONTENT) || !c_load || forbidden {
        Err(Error::Policy)
    } else {
        Ok(())
    }
}

pub trait ReadOnlyPreflight {
    fn identity(&mut self) -> Result<Identity>;
    fn prom(&mut self) -> Result<Vec<u8>>;
}
pub struct Prepared {
    pub identity: Identity,
    pub prom: Vec<u8>,
    pub parsed: Parsed,
}
/// The interface deliberately has no write/reset/enable method.
pub fn preflight(io: &mut impl ReadOnlyPreflight) -> Result<Prepared> {
    let identity = io.identity()?;
    validate_identity(&identity)?;
    let prom = io.prom()?;
    let parsed = validate_rom(&prom)?;
    Ok(Prepared {
        identity,
        prom,
        parsed,
    })
}

pub trait CompletionIo {
    fn read(&mut self, offset: u32) -> Option<u32>;
    fn wait_ms(&mut self);
}

pub trait OneShotPermission {
    fn valid(&mut self) -> bool;
    fn delete_and_flush(&mut self) -> bool;
    fn absent(&mut self) -> bool;
}

/// Never authorize from intent alone: durable consumption must succeed before
/// the caller starts a countdown or any GPU transaction.
pub fn consume_once(io: &mut impl OneShotPermission) -> bool {
    io.valid() && io.delete_and_flush() && io.absent()
}

/// Post-booter evidence. Keep transport errors separate from register values:
/// all ones is the intended open PLM value, but never a valid CPUCTL value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostBoot {
    pub gsp: Option<u32>,
    pub sec2: Option<u32>,
    pub plm: Option<u32>,
    pub wpr_hi: Option<u32>,
}

impl PostBoot {
    pub fn capture(io: &mut impl CompletionIo) -> Self {
        Self {
            gsp: io.read(0x110100),
            sec2: io.read(0x840100),
            plm: io.read(0x409650),
            wpr_hi: io.read(0x1fa828),
        }
    }

    /// Bits 0..3 identify failed GSP, SEC2, PLM and WPR checks respectively.
    pub fn failures(self) -> u8 {
        u8::from(!self.gsp.is_some_and(halted))
            | (u8::from(!self.sec2.is_some_and(halted)) << 1)
            | (u8::from(self.plm != Some(0xffffffff)) << 2)
            | (u8::from(self.wpr_hi != Some(0)) << 3)
    }
}

/// Exact FEAT PLM states accepted after the existing host relock write.
/// Both words clear the L0 write bit (4); ef retains bits 5/6 unlike 8f.
/// The latter was read back on this TU102 and in the pinned TU106 evidence.
/// This is host write protection, not a claim of full factory restoration.
/// See docs/guarded-msi-2080ti.md for the NVIDIA bit definitions and limits.
pub fn feat_host_write_closed(plm: Option<u32>) -> bool {
    matches!(plm, Some(0xffffff8f) | Some(0xffffffef))
}

/// Final register evidence before restoring PCI command and releasing DMA.
/// Read every field once, preserving transport failures rather than stopping
/// at the first failed clause and hiding the remaining evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalState {
    pub gsp: Option<u32>,
    pub sec2: Option<u32>,
    pub plm: Option<u32>,
    pub ss0: Option<u32>,
    pub ss1: Option<u32>,
    pub wpr_hi: Option<u32>,
}

impl FinalState {
    pub fn capture(io: &mut impl CompletionIo) -> Self {
        Self {
            gsp: io.read(0x110100),
            sec2: io.read(0x840100),
            plm: io.read(0x409650),
            ss0: io.read(0x409664),
            ss1: io.read(0x40966c),
            wpr_hi: io.read(0x1fa828),
        }
    }

    /// Bits 0..5: GSP HALT, SEC2 HALT, host write protection, SS0, SS1, WPR2.
    pub fn failures(self) -> u8 {
        u8::from(!self.gsp.is_some_and(halted))
            | (u8::from(!self.sec2.is_some_and(halted)) << 1)
            | (u8::from(!feat_host_write_closed(self.plm)) << 2)
            | (u8::from(self.ss0 != Some(0x88888888)) << 3)
            | (u8::from(self.ss1 != Some(8)) << 4)
            | (u8::from(self.wpr_hi != Some(0)) << 5)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion {
    pub ready: bool,
    pub stopped: bool,
}
/// WPR2 appearing alone does not establish completion. Unknown reads never
/// count as halted. The caller may release DMA only when stopped is true.
pub fn wait_fwsec(io: &mut impl CompletionIo, frts: u64, samples: u32) -> Completion {
    if frts == 0 || frts & 0xfff != 0 || frts > 0x0000_00ff_fff0_0000 {
        return Completion {
            ready: false,
            stopped: false,
        };
    }
    let mut stopped = false;
    for _ in 0..samples {
        let lo = io.read(0x1fa824);
        let hi = io.read(0x1fa828);
        stopped = io.read(0x110100).is_some_and(halted);
        if stopped && lo == Some((frts >> 8) as u32) && hi == Some(((frts + 0xe0000) >> 8) as u32) {
            return Completion {
                ready: true,
                stopped: true,
            };
        }
        io.wait_ms();
    }
    Completion {
        ready: false,
        stopped,
    }
}

/// Pins from upstream manifest and issue 17's 6503869058 asset inventory.
/// These identify generic four-PLM assets, NOT PR49's dummy-RM/direct-SS set.
pub const ASSETS: &[(&str, usize, &str)] = &[
    (
        "booter_load_tu10x.bin",
        59224,
        "51507de61777cf46f3703a084cdfa896c21f6bc0c8af8f245bd09b2e9c49bf54",
    ),
    (
        "fwimage_tu10x.bin",
        29339080,
        "c10c2866e360154e822087957bc4269168e44f8d45922110e67fd751355806f9",
    ),
    (
        "fwsignature_tu10x.bin",
        4096,
        "f5020943a001134927f247f5d9aeccb38a51111fcd490bc91eade76c6e1f437e",
    ),
    (
        "gsp_boot_desc_tu10x.bin",
        76,
        "9e7cca2cfdc5ce601b0adba39967850046108df6c2d23fff95578ca3f0ccbe01",
    ),
    (
        "gsp_boot_tu10x.bin",
        24576,
        "de676bae28a480011d3d012db14bef539324e62a841a9627863c689bea168af3",
    ),
    (
        "sec2_bl_tu10x.bin",
        768,
        "f21f1cfbb8fffa8067685314367fc4c50f3f84dd99eda7c19db4292d9269b9d9",
    ),
    (
        "v67-tu10x-four-plm.bin",
        64000,
        "b0ec90294d1de3c67e024631c0a11000f281470d76eed806fc24c66b7fd1ef41",
    ),
];
pub fn validate_assets(blobs: &[&[u8]]) -> Result<()> {
    if blobs.len() != ASSETS.len() {
        return Err(Error::Asset);
    }
    for (data, (_, len, hash)) in blobs.iter().zip(ASSETS) {
        if data.len() != *len || !hash_matches(data, hash) {
            return Err(Error::Asset);
        }
    }
    Ok(())
}
