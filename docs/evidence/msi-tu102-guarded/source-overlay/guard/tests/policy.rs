use tu102_guard::*;
fn identity() -> Identity {
    Identity {
        location: LOCATION,
        vendor: 0x10de,
        device: 0x1e07,
        subvendor: 0x1462,
        subsystem: 0x371d,
        command: 7,
        pmcsr: 0,
        chip: 0x162000a1,
        capacity: 0x2c9,
        wpr_hi: 0,
        gsp: 0x10,
        sec2: 0x10,
    }
}
fn put(d: &mut [u8], o: usize, v: u32) {
    d[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn synthetic() -> Vec<u8> {
    let mut d = vec![0; 1024];
    for base in [0, 512] {
        d[base..base + 2].copy_from_slice(&0xaa55u16.to_le_bytes());
        d[base + 24] = 0x20;
        d[base + 0x20..base + 0x24].copy_from_slice(b"PCIR");
        d[base + 0x2a] = 24;
        d[base + 0x30] = 1;
    }
    d[0x234] = 0xe0;
    d[0x235] = 128;
    d[0x80..0x86].copy_from_slice(b"\xff\xb8BIT\0");
    d[0x88] = 12;
    d[0x89] = 6;
    d[0x8a] = 1;
    d[0x8b] = 0u8.wrapping_sub(d[0x80..0x8b].iter().fold(0u8, |a, b| a.wrapping_add(*b)));
    d[0x8c..0x92].copy_from_slice(&[0x70, 2, 4, 0, 0xa0, 0]);
    put(&mut d, 0xa0, 0x250);
    d[0x250..0x256].copy_from_slice(&[1, 6, 6, 1, 0, 0]);
    d[0x256] = 0x85;
    put(&mut d, 0x258, 0x280);
    for (o, v) in [
        (0, 0x3c0201),
        (4, 128),
        (8, 128),
        (24, 64),
        (32, 32),
        (36, 32),
        (40, 64),
        (48, 64),
    ] {
        put(&mut d, 0x280 + o, v);
    }
    d
}
#[test]
fn parses_synthetic_geometry() {
    let p = parse(&synthetic()).unwrap();
    assert_eq!(p.view, 0..1024);
    assert_eq!(p.image, 0x2bc..0x33c);
    assert_eq!(p.dmem_size, 64);
}
#[test]
fn every_truncation_rejected() {
    let d = synthetic();
    for n in 0..d.len() {
        assert!(parse(&d[..n]).is_err(), "{n}");
    }
}
#[test]
fn wrong_hash_rejected_after_parse() {
    assert_eq!(validate_rom(&synthetic()), Err(Error::Hash));
}
#[test]
fn pointer_overflow_rejected() {
    let mut d = synthetic();
    put(&mut d, 0x258, u32::MAX);
    assert!(parse(&d).is_err());
}
#[test]
fn malformed_descriptor_rejected() {
    for (o, v) in [
        (0, 0x3c0301),
        (4, 129),
        (24, 0),
        (36, u32::MAX),
        (40, 0),
        (48, 0),
    ] {
        let mut d = synthetic();
        put(&mut d, 0x280 + o, v);
        assert!(parse(&d).is_err());
    }
}
#[test]
fn duplicate_production_rejected() {
    let mut d = synthetic();
    d[0x253] = 2;
    d[0x25c] = 0x85;
    put(&mut d, 0x25e, 0x280);
    assert_eq!(parse(&d), Err(Error::Ambiguous));
}
#[test]
fn no_heuristic_nvgi_scan() {
    let mut d = synthetic();
    d[..4].copy_from_slice(b"NVGI");
    put(&mut d, 4, 0x300);
    put(&mut d, 8, u32::MAX);
    assert!(parse(&d).is_err());
}
#[test]
fn bounded_random_input_no_panics() {
    let mut seed = 1u32;
    for n in 0..2048 {
        let mut d = vec![0; n];
        for b in &mut d {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *b = (seed >> 24) as u8;
        }
        assert!(parse(&d).is_err());
    }
}
#[test]
fn identity_gate() {
    assert!(validate_identity(&identity()).is_ok());
    let mut variants = Vec::new();
    let mut i = identity();
    i.location[1] += 1;
    variants.push(i);
    let mut i = identity();
    i.device += 1;
    variants.push(i);
    let mut i = identity();
    i.subsystem += 1;
    variants.push(i);
    let mut i = identity();
    i.capacity = 0x11;
    variants.push(i);
    let mut i = identity();
    i.command = 0;
    variants.push(i);
    let mut i = identity();
    i.chip = 0;
    variants.push(i);
    for i in variants {
        assert_eq!(validate_identity(&i), Err(Error::Identity));
    }
}
#[test]
fn warm_and_failed_reads_rejected() {
    for v in [0, 0xffffffff, 0xbadf0010] {
        let mut i = identity();
        i.gsp = v;
        assert_eq!(validate_identity(&i), Err(Error::Warm));
    }
    let mut i = identity();
    i.pmcsr = 3;
    assert_eq!(validate_identity(&i), Err(Error::Warm));
    i = identity();
    i.wpr_hi = 1;
    assert_eq!(validate_identity(&i), Err(Error::Warm));
}
struct Fake {
    id: Result<Identity, Error>,
    rom_reads: usize,
}
impl ReadOnlyPreflight for Fake {
    fn identity(&mut self) -> Result<Identity, Error> {
        self.id
    }
    fn prom(&mut self) -> Result<Vec<u8>, Error> {
        self.rom_reads += 1;
        Ok(synthetic())
    }
}
#[test]
fn preflight_rejects_before_rom_on_identity_failure() {
    let mut f = Fake {
        id: Err(Error::Read),
        rom_reads: 0,
    };
    assert!(preflight(&mut f).is_err());
    assert_eq!(f.rom_reads, 0);
    let mut i = identity();
    i.subsystem = 0;
    f.id = Ok(i);
    assert!(preflight(&mut f).is_err());
    assert_eq!(f.rom_reads, 0);
}
#[test]
fn preflight_rejects_rom_hash() {
    let mut f = Fake {
        id: Ok(identity()),
        rom_reads: 0,
    };
    assert!(matches!(preflight(&mut f), Err(Error::Hash)));
    assert_eq!(f.rom_reads, 1);
}
#[test]
fn explicit_policy_required() {
    assert!(validate_policy(Some(RUN_CONTENT), true, false).is_ok());
    for (run, c, bad) in [
        (None, true, false),
        (Some(&b"wrong"[..]), true, false),
        (Some(RUN_CONTENT), false, false),
        (Some(RUN_CONTENT), true, true),
    ] {
        assert_eq!(validate_policy(run, c, bad), Err(Error::Policy));
    }
}
#[test]
fn assets_missing_truncated_or_corrupted_rejected() {
    assert_eq!(validate_assets(&[]), Err(Error::Asset));
    let d = vec![&b""[..]; 7];
    assert_eq!(validate_assets(&d), Err(Error::Asset));
    assert!(!hash_matches(b"x", PCI_SHA));
}
struct Poll {
    frames: Vec<[Option<u32>; 3]>,
    at: usize,
}
impl CompletionIo for Poll {
    fn read(&mut self, o: u32) -> Option<u32> {
        self.frames[self.at.min(self.frames.len() - 1)][match o {
            0x1fa824 => 0,
            0x1fa828 => 1,
            0x110100 => 2,
            _ => panic!(),
        }]
    }
    fn wait_ms(&mut self) {
        self.at += 1;
    }
}
const F: u64 = 0x57fe00000;
fn frame(cpu: Option<u32>) -> [Option<u32>; 3] {
    [
        Some((F >> 8) as u32),
        Some(((F + 0xe0000) >> 8) as u32),
        cpu,
    ]
}
#[test]
fn wpr_before_halt_retains_dma() {
    let mut p = Poll {
        frames: vec![frame(Some(0))],
        at: 0,
    };
    assert_eq!(
        wait_fwsec(&mut p, F, 3),
        Completion {
            ready: false,
            stopped: false
        }
    );
}
#[test]
fn delayed_halt_allows_release() {
    let mut p = Poll {
        frames: vec![frame(Some(0)), frame(Some(0x10))],
        at: 0,
    };
    assert_eq!(
        wait_fwsec(&mut p, F, 3),
        Completion {
            ready: true,
            stopped: true
        }
    );
    assert_eq!(p.at, 1);
}
#[test]
fn halt_without_matching_wpr_is_failure() {
    let mut p = Poll {
        frames: vec![[Some(0), Some(0), Some(0x10)]],
        at: 0,
    };
    assert_eq!(
        wait_fwsec(&mut p, F, 3),
        Completion {
            ready: false,
            stopped: true
        }
    );
}
#[test]
fn read_errors_and_poison_never_release_dma() {
    for cpu in [None, Some(u32::MAX), Some(0xbadf0010)] {
        let mut p = Poll {
            frames: vec![frame(cpu)],
            at: 0,
        };
        assert_eq!(
            wait_fwsec(&mut p, F, 2),
            Completion {
                ready: false,
                stopped: false
            }
        );
    }
}
#[test]
fn invalid_address_and_zero_budget_fail_closed() {
    let mut p = Poll {
        frames: vec![frame(Some(0x10))],
        at: 0,
    };
    for f in [0, F + 1, u64::MAX] {
        assert!(!wait_fwsec(&mut p, f, 3).stopped);
    }
    assert!(!wait_fwsec(&mut p, F, 0).stopped);
}
#[test]
#[ignore = "requires user's private ROM via TU102_ROM; never committed"]
fn exported_rom_fixture() {
    let raw = std::fs::read(std::env::var("TU102_ROM").expect("TU102_ROM")).unwrap();
    let p = validate_rom(&raw).unwrap();
    assert_eq!(p.image.len(), 0x9df0);
    assert_eq!(p.dmem_size, 0x3f0);
    let normalized = &raw[p.view.clone()];
    let n = validate_rom(normalized).unwrap();
    assert_eq!(&raw[p.image], &normalized[n.image]);
    let mut corrupt = raw.clone();
    corrupt[p.view.start + 0x3e4c4] ^= 1;
    assert_eq!(validate_rom(&corrupt), Err(Error::Hash));
}

#[test]
#[ignore = "requires local regenerated firmware via TU102_ASSETS; never committed"]
fn regenerated_asset_fixtures() {
    let dir = std::path::PathBuf::from(std::env::var("TU102_ASSETS").expect("TU102_ASSETS"));
    let mut blobs: Vec<Vec<u8>> = ASSETS
        .iter()
        .map(|(n, _, _)| std::fs::read(dir.join(n)).unwrap())
        .collect();
    assert!(validate_assets(&blobs.iter().map(Vec::as_slice).collect::<Vec<_>>()).is_ok());
    for i in 0..blobs.len() {
        blobs[i][0] ^= 1;
        assert_eq!(
            validate_assets(&blobs.iter().map(Vec::as_slice).collect::<Vec<_>>()),
            Err(Error::Asset)
        );
        blobs[i][0] ^= 1;
    }
    blobs.swap(0, 1);
    assert_eq!(
        validate_assets(&blobs.iter().map(Vec::as_slice).collect::<Vec<_>>()),
        Err(Error::Asset)
    );
}
