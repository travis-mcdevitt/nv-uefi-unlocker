use tu102_guard::{CompletionIo, PostBoot};

fn success() -> PostBoot {
    PostBoot {
        gsp: Some(0x10),
        sec2: Some(0x10),
        plm: Some(0xffffffff),
        wpr_hi: Some(0),
    }
}

#[test]
fn open_plm_all_ones_is_a_valid_value() {
    // Regression: generic read32 rejected this intentional value before comparing it.
    assert_eq!(success().failures(), 0);
}

#[test]
fn each_combination_reports_all_failures() {
    for bits in 0u8..16 {
        let mut s = success();
        if bits & 1 != 0 {
            s.gsp = Some(0);
        }
        if bits & 2 != 0 {
            s.sec2 = Some(0);
        }
        if bits & 4 != 0 {
            s.plm = Some(0xffffff8f);
        }
        if bits & 8 != 0 {
            s.wpr_hi = Some(1);
        }
        assert_eq!(s.failures(), bits);
    }
}

#[test]
fn transport_failure_or_cpu_poison_cannot_be_success() {
    for value in [None, Some(0xffffffff), Some(0xbadf0010)] {
        let mut s = success();
        s.gsp = value;
        assert_eq!(s.failures(), 1);
        let mut s = success();
        s.sec2 = value;
        assert_eq!(s.failures(), 2);
    }
    for value in [None, Some(0xbadf5040), Some(0xffffff8f)] {
        let mut s = success();
        s.plm = value;
        assert_eq!(s.failures(), 4);
    }
    for value in [None, Some(0xffffffff), Some(0xbadf0000)] {
        let mut s = success();
        s.wpr_hi = value;
        assert_eq!(s.failures(), 8);
    }
}

#[test]
fn capture_reads_every_field_even_after_failed_read() {
    struct Io(Vec<u32>);
    impl CompletionIo for Io {
        fn read(&mut self, offset: u32) -> Option<u32> {
            self.0.push(offset);
            None
        }
        fn wait_ms(&mut self) {
            panic!("snapshot must not poll or retry")
        }
    }
    let mut io = Io(Vec::new());
    assert_eq!(PostBoot::capture(&mut io).failures(), 15);
    assert_eq!(io.0, [0x110100, 0x840100, 0x409650, 0x1fa828]);
}
