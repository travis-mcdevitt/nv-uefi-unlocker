//! Single-card, fail-closed entry. All preflight operations are read-only.
use crate::{chainload, pci, tx};
use alloc::vec::Vec;
use log::info;
use tu102_guard::{Error, Identity, ReadOnlyPreflight};
use uefi::{boot, cstr16, Identify, Status};

struct Reader<'a>(&'a pci::PciIo);
impl Reader<'_> {
    fn cfg(&self, off: u32) -> Result<u16, Error> {
        self.0.config_try_u16(off).ok_or(Error::Read)
    }
    fn reg(&self, off: u64) -> Result<u32, Error> {
        self.0.bar_try_read_u32(0, off).ok_or(Error::Read)
    }
}
impl ReadOnlyPreflight for Reader<'_> {
    fn identity(&mut self) -> Result<Identity, Error> {
        let (a, b, c, d) = self.0.get_location().ok_or(Error::Read)?;
        if self.cfg(6)? & 0x10 == 0 {
            return Err(Error::Read);
        }
        let mut next = self.cfg(0x34)? as u8;
        let mut seen = [false; 256];
        let mut pm = None;
        for _ in 0..48 {
            if next == 0 {
                break;
            }
            if next < 0x40 || next > 0xf8 || next & 3 != 0 || seen[next as usize] {
                return Err(Error::Format);
            }
            seen[next as usize] = true;
            let cap = self.cfg(next as u32)?;
            if cap as u8 == 1 {
                if pm.replace(self.cfg(next as u32 + 4)?).is_some() {
                    return Err(Error::Ambiguous);
                }
            }
            next = (cap >> 8) as u8;
        }
        if next != 0 {
            return Err(Error::Format);
        }
        Ok(Identity {
            location: [a, b, c, d],
            vendor: self.cfg(0)?,
            device: self.cfg(2)?,
            subvendor: self.cfg(0x2c)?,
            subsystem: self.cfg(0x2e)?,
            command: self.cfg(4)?,
            pmcsr: pm.ok_or(Error::Read)?,
            chip: self.reg(0)?,
            capacity: self.reg(0x100ce0)?,
            wpr_hi: self.reg(0x1fa828)?,
            gsp: self.reg(0x110100)?,
            sec2: self.reg(0x840100)?,
        })
    }
    fn prom(&mut self) -> Result<Vec<u8>, Error> {
        let mut words = alloc::vec![0u32;1024*1024/4];
        if self.0.bar_read_u32_buf(0, 0x300000, &mut words) != Status::SUCCESS {
            return Err(Error::Read);
        }
        Ok(words.iter().flat_map(|v| v.to_le_bytes()).collect())
    }
}

pub struct CompletionReader<'a, 'b>(pub &'a tx::TxGpu<'b>);
impl tu102_guard::CompletionIo for CompletionReader<'_, '_> {
    fn read(&mut self, offset: u32) -> Option<u32> {
        self.0.read32(offset).ok()
    }
    fn wait_ms(&mut self) {
        boot::stall(core::time::Duration::from_millis(1));
    }
}

pub struct PostBootReader<'a, 'b>(pub &'a tx::TxGpu<'b>);
impl tu102_guard::CompletionIo for PostBootReader<'_, '_> {
    fn read(&mut self, offset: u32) -> Option<u32> {
        // Preserve successful all-ones PLM reads. None means the PCI I/O
        // protocol rejected the read; the field-specific policy handles values.
        self.0.pci.bar_try_read_u32(self.0.bar, offset as u64)
    }
    fn wait_ms(&mut self) {}
}

/// Never return to Boot Services/OS after an uncertain active transaction:
/// leaked DMA pages must remain reserved until full power removal.
fn stop() -> ! {
    info!("GUARDED STOP: no retry, reset, reconnect or OS handoff. Fully power off manually.");
    crate::logging::finish();
    loop {
        boot::stall(core::time::Duration::from_secs(1));
    }
}

fn prepare_assets() -> Option<tx::TxAssets> {
    let mut blobs = Vec::new();
    for (name, _, _) in tu102_guard::ASSETS {
        blobs.push(tx::read_boot_file(&alloc::format!("\\{name}"))?);
    }
    let refs: Vec<&[u8]> = blobs.iter().map(Vec::as_slice).collect();
    tu102_guard::validate_assets(&refs).ok()?;
    let mut iter = blobs.into_iter();
    Some(tx::TxAssets {
        family: "tu10x",
        booter: iter.next()?,
        gsp_image: iter.next()?,
        stock_sig: iter.next()?,
        boot_desc: Some(iter.next()?),
        gsp_boot: iter.next()?,
        bl: Some(iter.next()?),
        v67: Some(iter.next()?),
    })
}

fn select_card() -> Option<pci::FoundGpu> {
    let handles =
        boot::locate_handle_buffer(boot::SearchType::ByProtocol(&pci::PciIo::GUID)).ok()?;
    let mut found = None;
    for handle in handles.iter() {
        let p = unsafe {
            boot::open_protocol::<pci::PciIo>(
                boot::OpenProtocolParams {
                    handle: *handle,
                    agent: boot::image_handle(),
                    controller: None,
                },
                boot::OpenProtocolAttributes::GetProtocol,
            )
        }
        .ok()?;
        let vendor = p.config_try_u16(0)?;
        // Upstream enables bridge bus-master bits with Drop-based rollback.
        // This candidate requires an already-enabled path and never mutates it.
        if p.config_try_u16(0xe)? & 0x7f == 1 {
            let (segment, _, _, _) = p.get_location()?;
            let secondary = (p.config_try_u16(0x18)? >> 8) as usize;
            let subordinate = (p.config_try_u16(0x1a)? & 255) as usize;
            if segment == tu102_guard::LOCATION[0]
                && secondary <= tu102_guard::LOCATION[1]
                && subordinate >= tu102_guard::LOCATION[1]
                && p.config_try_u16(4)? & 4 == 0
            {
                return None;
            }
        }
        if vendor != pci::NVIDIA_VENDOR_ID {
            continue;
        }
        if p.config_try_u16(0xa)? >> 8 != 3 {
            continue;
        }
        let (seg, bus, dev, fun) = p.get_location()?;
        let g = pci::FoundGpu {
            handle: *handle,
            device_id: p.config_try_u16(2)?,
            seg,
            bus,
            dev,
            fun,
        };
        if found.replace(g).is_some() {
            return None;
        }
    }
    let g = found?;
    (g.device_id == 0x1e07 && [g.seg, g.bus, g.dev, g.fun] == tu102_guard::LOCATION).then_some(g)
}

pub fn run() -> Status {
    crate::logging::init(crate::VERSION);
    info!("GUARDED BUILD 004: final-state diagnostics and host-write protection");
    #[cfg(not(feature = "guarded-auto"))]
    info!("GUARDED MSI 371D: experimental SEC2-A; manual confirmation mandatory");
    #[cfg(feature = "guarded-auto")]
    info!("GUARDED AUTO: one-use permission replaces keyboard confirmation; all hardware guards remain mandatory");
    if boot::set_watchdog_timer(0, 0x10000, None).is_err() {
        info!("GUARDED: watchdog disable failed; refusing transaction");
        crate::logging::finish();
        return Status::ABORTED;
    }
    let Some(windows) = chainload::guarded_windows() else {
        info!("GUARDED: exactly one existing Windows Boot Manager required; zero GPU writes");
        crate::logging::finish();
        return Status::ABORTED;
    };
    let changed = attempt();
    match chainload::start_loader(windows, cstr16!(r"\EFI\Microsoft\Boot\bootmgfw.efi")) {
        Ok(()) => info!("GUARDED: Windows Boot Manager returned"),
        Err(e) => info!("GUARDED: Windows handoff failed {:?}", e.status()),
    }
    if changed {
        stop();
    }
    crate::logging::finish();
    Status::ABORTED
}

fn attempt() -> bool {
    let run = tx::read_boot_file(tu102_guard::RUN_MARKER);
    let forbidden = tu102_guard::FORBIDDEN_MARKERS.iter().any(|p| {
        uefi::CString16::try_from(*p)
            .ok()
            .is_none_or(|s| chainload::boot_volume_has(&s))
    });
    if tu102_guard::validate_policy(
        run.as_deref(),
        tx::read_boot_file("\\tx-sec2-c-load.txt").is_some(),
        forbidden,
    )
    .is_err()
    {
        info!("GUARDED: run policy rejected; zero GPU writes");
        return false;
    }
    let Some(g) = select_card() else {
        info!("GUARDED: unique pinned GPU selection failed");
        return false;
    };
    let Ok(pci) = (unsafe {
        boot::open_protocol::<pci::PciIo>(
            boot::OpenProtocolParams {
                handle: g.handle,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
    }) else {
        return false;
    };
    let Some(assets) = prepare_assets() else {
        info!("GUARDED: asset hashes/lengths rejected");
        return false;
    };
    let prepared = match tu102_guard::preflight(&mut Reader(&pci)) {
        Ok(p) => p,
        Err(e) => {
            info!("GUARDED: read-only preflight rejected {e:?}");
            return false;
        }
    };
    let gpu = tx::TxGpu { pci: &pci, bar: 0 };
    if crate::tx_fwsec::layout_from_registers(
        &gpu,
        "tu10x",
        assets.gsp_image.len() as u64,
        assets.gsp_boot.len() as u64,
    )
    .is_err()
    {
        info!("GUARDED: read-only layout rejected");
        return false;
    }
    #[cfg(not(feature = "guarded-auto"))]
    let confirmation = crate::keywait::wait_confirm(30);
    #[cfg(feature = "guarded-auto")]
    let confirmation = {
        if !consume_auto_permission() {
            info!("GUARDED AUTO: missing, invalid or unconsumed one-use permission; no GPU writes");
            return false;
        }
        info!("GUARDED AUTO: one-use permission deleted and flushed; proceeding in 10 seconds, any key aborts");
        crate::keywait::wait_confirm_armed(10)
    };
    if !matches!(confirmation, crate::keywait::Confirm::Proceed) {
        return false;
    }
    // Recheck identity immediately before detach; no cached warm-state assumption.
    if Reader(&pci)
        .identity()
        .and_then(|i| {
            tu102_guard::validate_identity(&i)?;
            if i.command != prepared.identity.command {
                return Err(Error::Identity);
            }
            Ok(())
        })
        .is_err()
    {
        return false;
    }
    // From this point even an error during detach is an active failure.
    if !crate::logging::file_open() {
        info!("GUARDED: persistent log unavailable; zero GPU writes");
        return false;
    }
    if let Err(e) = tx::run_transaction(&gpu, &assets, g.handle, &prepared) {
        info!("GUARDED: active transaction failed {e:?}");
        stop();
    }
    info!("GUARDED: verified transaction complete; Windows-only handoff");
    true
}

#[cfg(feature = "guarded-auto")]
fn consume_auto_permission() -> bool {
    use uefi::proto::media::file::{File, FileAttribute, FileMode};
    struct Permission;
    impl tu102_guard::OneShotPermission for Permission {
        fn valid(&mut self) -> bool {
            tx::read_boot_file("\\guarded-auto-once.txt").as_deref()
                == Some(b"MSI-371D-90.02.42.00.35-AUTO-ONCE")
        }
        fn delete_and_flush(&mut self) -> bool {
            let Ok(mut fs) = boot::get_image_file_system(boot::image_handle()) else { return false; };
            let Ok(mut dir) = fs.open_volume() else { return false; };
            let Ok(file) = dir.open(cstr16!(r"\guarded-auto-once.txt"), FileMode::ReadWrite, FileAttribute::empty()) else { return false; };
            file.delete().is_ok() && dir.flush().is_ok()
        }
        fn absent(&mut self) -> bool {
            let Ok(mut fs) = boot::get_image_file_system(boot::image_handle()) else { return false; };
            let Ok(mut dir) = fs.open_volume() else { return false; };
            matches!(dir.open(cstr16!(r"\guarded-auto-once.txt"), FileMode::Read, FileAttribute::empty()), Err(e) if e.status() == Status::NOT_FOUND)
        }
    }
    tu102_guard::consume_once(&mut Permission)
}
