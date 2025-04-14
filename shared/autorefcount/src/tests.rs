use super::*;

#[test]
fn initial_count() {
    let rc = AutoRefCount::new(1);
    assert_eq!(rc.count(), 1);
}

#[test]
fn increment_increases_count() {
    let rc = AutoRefCount::new(0);
    {
        let _guard = rc.increment();
        assert_eq!(rc.count(), 1);
    }
    assert_eq!(rc.count(), 0);
}

#[test]
fn multiple_increments() {
    let rc = AutoRefCount::new(0);
    {
        let _guard1 = rc.increment();
        assert_eq!(rc.count(), 1);

        {
            let _guard2 = rc.increment();
            assert_eq!(rc.count(), 2);
        }

        assert_eq!(rc.count(), 1);
    }
    assert_eq!(rc.count(), 0);
}

#[test]
#[should_panic]
fn underflow_panic_on_decrement() {
    let rc = AutoRefCount::new(0);
    let _guard = rc.increment();
    rc.0.store(0, Ordering::SeqCst);
    drop(_guard);
}

#[test]
#[should_panic]
fn underflow_panic_on_increment_check() {
    let rc = AutoRefCount::new(usize::MAX);
    let _guard = rc.increment();
}
