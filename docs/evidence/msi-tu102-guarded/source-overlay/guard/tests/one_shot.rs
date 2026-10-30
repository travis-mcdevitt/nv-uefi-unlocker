use tu102_guard::{consume_once, OneShotPermission};

struct Permit {
    present: bool,
    valid: bool,
    flush_ok: bool,
    verify_ok: bool,
    deletes: usize,
}
impl OneShotPermission for Permit {
    fn valid(&mut self) -> bool {
        self.present && self.valid
    }
    fn delete_and_flush(&mut self) -> bool {
        self.deletes += 1;
        self.present = false;
        self.flush_ok
    }
    fn absent(&mut self) -> bool {
        self.verify_ok && !self.present
    }
}
fn permit() -> Permit {
    Permit {
        present: true,
        valid: true,
        flush_ok: true,
        verify_ok: true,
        deletes: 0,
    }
}
#[test]
fn cannot_authorize_twice() {
    let mut p = permit();
    assert!(consume_once(&mut p));
    assert!(!consume_once(&mut p));
    assert_eq!(p.deletes, 1);
}
#[test]
fn invalid_or_missing_permission_is_not_consumed() {
    for present in [true, false] {
        let mut p = permit();
        p.present = present;
        p.valid = false;
        assert!(!consume_once(&mut p));
        assert_eq!(p.deletes, 0);
    }
}
#[test]
fn failed_durable_delete_denies_transaction() {
    let mut p = permit();
    p.flush_ok = false;
    assert!(!consume_once(&mut p));
}
#[test]
fn failed_absence_verification_denies_transaction() {
    let mut p = permit();
    p.verify_ok = false;
    assert!(!consume_once(&mut p));
}
