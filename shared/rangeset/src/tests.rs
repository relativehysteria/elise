use super::*;

const DEFAULT_RS: RangeSet = RangeSet::new();

#[test]
fn range_new_valid() {
    let range = Range::new(5, 10);
    assert!(range.is_ok());
    let range = range.unwrap();
    assert_eq!(range.start, 5);
    assert_eq!(range.end, 10);
}

#[test]
fn range_new_invalid() {
    let range = Range::new(10, 5);
    assert_eq!(
        range.unwrap_err(), Error::InvalidRange(Range { start: 10, end: 5 }));
}

#[test]
fn range_contains() {
    let range1 = Range::new(5, 15).unwrap();
    let range2 = Range::new(7, 10).unwrap();
    assert_eq!(range1.contains(&range2), true);
}

#[test]
fn range_contains_edge_cases() {
    let range1 = Range::new(5, 15).unwrap();

    let range3 = Range::new(15, 15).unwrap();
    assert_eq!(range1.contains(&range3), true);

    let range4 = Range::new(16, 16).unwrap();
    assert_eq!(range1.contains(&range4), false);
}

#[test]
fn range_overlaps() {
    let range1 = Range::new(5, 15).unwrap();
    let range2 = Range::new(10, 20).unwrap();
    let overlap = range1.overlaps(&range2);
    assert!(overlap.is_some());
    let overlap = overlap.unwrap();
    assert_eq!(overlap.start, 10);
    assert_eq!(overlap.end, 15);
}

#[test]
fn range_no_overlap() {
    let range1 = Range::new(5, 15).unwrap();
    let range2 = Range::new(16, 20).unwrap();
    assert!(range1.overlaps(&range2).is_none());
}

#[test]
fn rangeset_new() {
    let rangeset = DEFAULT_RS.clone();
    assert!(rangeset.is_empty());
    assert_eq!(rangeset.entries().len(), 0);
}

#[test]
fn rangeset_insert() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 15).unwrap()).unwrap();
    rangeset.insert(Range::new(20, 30).unwrap()).unwrap();

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].start, 5);
    assert_eq!(entries[0].end, 15);
    assert_eq!(entries[1].start, 20);
    assert_eq!(entries[1].end, 30);
}

#[test]
fn rangeset_insert_ordering() {
    let mut rangeset = DEFAULT_RS.clone();
    assert_eq!(rangeset.insert(Range::new(0x1a, 0x9ffff).unwrap()), Ok(()));
    assert_eq!(rangeset.insert(Range::new(0x2, 0x9).unwrap()), Ok(()));

    let entries = rangeset.entries();
    assert_eq!(entries[0], Range { start: 0x2, end: 0x9 });
    assert_eq!(entries[1], Range { start: 0x1a, end: 0x9ffff});
}

#[test]
fn rangeset_insert_overlap() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 15).unwrap()).unwrap();
    rangeset.insert(Range::new(10, 20).unwrap()).unwrap();

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].start, 5);
    assert_eq!(entries[0].end, 20);
}

#[test]
fn rangeset_insert_touching() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 10).unwrap()).unwrap();
    rangeset.insert(Range::new(11, 15).unwrap()).unwrap();

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].start, 5);
    assert_eq!(entries[0].end, 15);
}

#[test]
fn rangeset_remove() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 15).unwrap()).unwrap();
    rangeset.insert(Range::new(20, 30).unwrap()).unwrap();
    let removed = rangeset.remove(Range::new(7, 10).unwrap()).unwrap();
    assert!(removed);

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].start, 5);
    assert_eq!(entries[0].end, 6);
    assert_eq!(entries[1].start, 11);
    assert_eq!(entries[1].end, 15);
}

#[test]
fn rangeset_remove_full_range() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 15).unwrap()).unwrap();
    let removed = rangeset.remove(Range::new(5, 15).unwrap()).unwrap();
    assert!(removed);
    assert!(rangeset.is_empty());
}

#[test]
fn rangeset_remove_noop() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(5, 15).unwrap()).unwrap();
    let removed = rangeset.remove(Range::new(16, 20).unwrap()).unwrap();
    assert!(!removed);

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(rangeset.in_use, entries.len());
    assert_eq!(entries[0].start, 5);
    assert_eq!(entries[0].end, 15);
}

#[test]
fn rangeset_remove_partial_overlap() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(10, 50).unwrap()).unwrap();
    rangeset.insert(Range::new(100, 150).unwrap()).unwrap();

    // Remove a part of the first range
    let removed = rangeset.remove(Range::new(30, 40).unwrap()).unwrap();
    assert!(removed);

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 3);  // Should now have three ranges
    assert_eq!(entries[0], Range { start: 10, end: 29 });
    assert_eq!(entries[1], Range { start: 41, end: 50 });
    assert_eq!(entries[2], Range { start: 100, end: 150 });
}

#[test]
fn rangeset_delete() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(10, 20).unwrap()).unwrap();

    assert_eq!(rangeset.delete(1).unwrap_err(), Error::IndexOutOfBounds(1));
    assert!(rangeset.delete(0).is_ok())
}

#[test]
fn rangeset_split_entry() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(10, 30).unwrap()).unwrap();
    rangeset.split_entry(0, Range::new(15, 20).unwrap()).unwrap();

    let entries = rangeset.entries();
    assert_eq!(rangeset.in_use, entries.len());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], Range { start: 10, end: 14 });
    assert_eq!(entries[1], Range { start: 21, end: 30 });
}

#[test]
fn rangeset_split_entry_complex() {
    let mut rangeset = DEFAULT_RS.clone();
    rangeset.insert(Range::new(100, 300).unwrap()).unwrap();
    rangeset.split_entry(0, Range::new(150, 250).unwrap()).unwrap();

    let entries = rangeset.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], Range { start: 100, end: 149 });
    assert_eq!(entries[1], Range { start: 251, end: 300 });

    rangeset.split_entry(1, Range::new(250, 250).unwrap()).unwrap();
    let entries = rangeset.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0], Range { start: 100, end: 149 });
    assert_eq!(entries[1], Range { start: 251, end: 300 });
}

#[test]
fn rangeset_len_edge_cases() {
    let mut rangeset = DEFAULT_RS.clone();

    // Test with no ranges (should return None)
    assert_eq!(rangeset.len(), Some(0));

    // Test with large range
    rangeset.insert(Range::new(0, usize::MAX - 1).unwrap()).unwrap();
    assert_eq!(rangeset.len(), Some(usize::MAX));

    // Test with overlapping range
    rangeset.insert(Range::new(usize::MAX / 2, usize::MAX - 1).unwrap()).unwrap();
    assert_eq!(rangeset.len(), Some(usize::MAX));  // Should remain the same, as the range overlaps
}
