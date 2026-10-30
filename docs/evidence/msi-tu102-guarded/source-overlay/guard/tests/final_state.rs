use tu102_guard::{feat_host_write_closed, CompletionIo, FinalState, PostBoot};

fn observed() -> FinalState {
    // hardware-attempt-002: post-booter CPU/WPR values and relock PLM/selectors.
    // The old final group did not log fresh CPU/WPR reads, so this regression
    // tests the observed values together, not a proven successful OS handoff.
    FinalState {
        gsp: Some(0x10),
        sec2: Some(0x10),
        plm: Some(0xffffffef),
        ss0: Some(0x88888888),
        ss1: Some(8),
        wpr_hi: Some(0),
    }
}

#[test]
fn observed_open_to_host_closed_transition_passes_both_gates() {
    let pre = PostBoot {
        gsp: Some(0x10),
        sec2: Some(0x10),
        plm: Some(0xffffffff),
        wpr_hi: Some(0),
    };
    assert_eq!(pre.failures(), 0);
    assert_eq!(observed().failures(), 0);
    let mut still_open = observed();
    still_open.plm = pre.plm;
    assert_eq!(still_open.failures(), 4);
}

#[test]
fn original_reference_mask_still_passes() {
    let mut s = observed();
    s.plm = Some(0xffffff8f);
    assert_eq!(s.failures(), 0);
}

#[test]
fn only_two_exact_plm_words_are_accepted() {
    for low in 0..=255 {
        assert_eq!(
            feat_host_write_closed(Some(0xffffff00 | low)),
            low == 0x8f || low == 0xef
        );
    }
    for accepted in [0xffffff8f, 0xffffffef] {
        for bit in 8..32 {
            assert!(!feat_host_write_closed(Some(accepted ^ (1 << bit))));
        }
    }
    for value in [
        None,
        Some(0),
        Some(0xbadf5040),
        Some(0xbadf0010),
        Some(0x8f),
        Some(0xef),
    ] {
        assert!(!feat_host_write_closed(value));
    }
}

#[test]
fn all_failure_combinations_report_every_failed_field() {
    for bits in 0u8..64 {
        let mut s = observed();
        if bits & 1 != 0 {
            s.gsp = Some(0);
        }
        if bits & 2 != 0 {
            s.sec2 = Some(0);
        }
        if bits & 4 != 0 {
            s.plm = Some(0xffffffff);
        }
        if bits & 8 != 0 {
            s.ss0 = Some(0x2000015);
        }
        if bits & 16 != 0 {
            s.ss1 = Some(3);
        }
        if bits & 32 != 0 {
            s.wpr_hi = Some(1);
        }
        assert_eq!(s.failures(), bits);
    }
}

#[test]
fn unreadable_or_poisoned_fields_never_allow_handoff() {
    for value in [None, Some(u32::MAX), Some(0xbadf0010), Some(0xbadf5040)] {
        let mut s = observed();
        s.gsp = value;
        assert_eq!(s.failures(), 1);
        let mut s = observed();
        s.sec2 = value;
        assert_eq!(s.failures(), 2);
        let mut s = observed();
        s.plm = value;
        assert_eq!(s.failures(), 4);
        let mut s = observed();
        s.ss0 = value;
        assert_eq!(s.failures(), 8);
        let mut s = observed();
        s.ss1 = value;
        assert_eq!(s.failures(), 16);
        let mut s = observed();
        s.wpr_hi = value;
        assert_eq!(s.failures(), 32);
    }
}

#[test]
fn selector_bit_changes_cannot_pass() {
    for bit in 0..32 {
        let mut s = observed();
        s.ss0 = Some(0x88888888 ^ (1 << bit));
        assert_eq!(s.failures(), 8);
        let mut s = observed();
        s.ss1 = Some(8 ^ (1 << bit));
        assert_eq!(s.failures(), 16);
    }
}

#[test]
fn capture_reads_every_register_once_even_after_error() {
    struct Io {
        values: [Option<u32>; 6],
        seen: Vec<u32>,
    }
    const OFFSETS: [u32; 6] = [0x110100, 0x840100, 0x409650, 0x409664, 0x40966c, 0x1fa828];
    impl CompletionIo for Io {
        fn read(&mut self, offset: u32) -> Option<u32> {
            assert_eq!(offset, OFFSETS[self.seen.len()]);
            let v = self.values[self.seen.len()];
            self.seen.push(offset);
            v
        }
        fn wait_ms(&mut self) {
            panic!("final snapshot must not poll or retry")
        }
    }
    let mut io = Io {
        values: [
            Some(0x10),
            Some(0x10),
            Some(0xffffffef),
            Some(0x88888888),
            Some(8),
            Some(0),
        ],
        seen: Vec::new(),
    };
    assert_eq!(FinalState::capture(&mut io), observed());
    assert_eq!(io.seen, OFFSETS);
    let mut io = Io {
        values: [None; 6],
        seen: Vec::new(),
    };
    assert_eq!(FinalState::capture(&mut io).failures(), 63);
    assert_eq!(io.seen, OFFSETS);
}
