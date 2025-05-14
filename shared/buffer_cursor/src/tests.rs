use super::*;

#[test]
fn write_and_positioning() {
    let mut data = [0u8; 10];
    let mut cursor = Cursor::new(&mut data);

    let result = cursor.write(&[1, 2, 3]).unwrap();
    assert_eq!(result, (0, 3));
    assert_eq!(cursor.pos, 3);
    assert_eq!(cursor.total_pos, 3);
    assert_eq!(&data[..3], &[1, 2, 3]);
}

#[test]
fn try_set_position_within_bounds() {
    let mut data = [0u8; 10];
    let mut cursor = Cursor::new(&mut data);

    assert_eq!(cursor.try_set_position(5), Some(5));
    assert_eq!(cursor.pos, 5);
    assert_eq!(cursor.total_pos, 5);
}

#[test]
fn try_set_position_over_limit() {
    let mut data = [0u8; 10];
    let mut cursor = Cursor::new_with_limit(&mut data, 5);

    assert_eq!(cursor.try_set_position(6), None);
}

#[test]
fn try_set_position_backward() {
    let mut data = [0u8; 10];
    let mut cursor = Cursor::new(&mut data);

    cursor.try_set_position(6).unwrap();
    assert_eq!(cursor.try_set_position(3), Some(3));
    assert_eq!(cursor.pos, 3);
    assert_eq!(cursor.total_pos, 3);
}

#[test]
fn split_at_mut_basic() {
    let mut data = [1, 2, 3, 4, 5];
    let cursor = Cursor::new(&mut data);

    let (left, right_cursor) = cursor.split_at_mut(3);
    assert_eq!(left, &[1, 2, 3]);
    assert_eq!(right_cursor.inner, &[4, 5]);
    assert_eq!(right_cursor.pos, 0);
    assert_eq!(right_cursor.total_pos, 3);
}

#[test]
fn split_at_mut_checked_success() {
    let mut data = [0u8; 8];
    let mut cursor = Cursor::new(&mut data);
    cursor.try_set_position(2).unwrap();

    let (left, right_cursor) = cursor.split_at_mut_checked(5).unwrap();
    assert_eq!(left.len(), 5);
    assert_eq!(right_cursor.inner.len(), 3);
    assert_eq!(right_cursor.pos, 0);
    assert_eq!(right_cursor.total_pos, 5);
}

#[test]
fn split_at_mut_checked_same_position() {
    let mut data = [0u8; 4];
    let cursor = Cursor::new(&mut data);

    let (left, right_cursor) = cursor.split_at_mut_checked(0).unwrap();
    assert_eq!(left.len(), 0);
    assert_eq!(right_cursor.inner.len(), 4);
    assert_eq!(right_cursor.pos, 0);
    assert_eq!(right_cursor.total_pos, 0);
}

#[test]
fn split_at_mut_checked_out_of_bounds() {
    let mut data = [0u8; 5];
    let cursor = Cursor::new(&mut data);

    assert!(cursor.split_at_mut_checked(6).is_none());
}

#[test]
fn split_at_mut_checked_overflow_limit() {
    let mut data = [0u8; 10];
    let mut cursor = Cursor::new_with_limit(&mut data, 5);
    cursor.try_set_position(3).unwrap();

    // Would push total_pos to 6 (over limit)
    assert!(cursor.split_at_mut_checked(6).is_none());
}

#[test]
#[should_panic(expected = "Attempted to split cursor with overflow")]
fn split_at_mut_panics_on_overflow() {
    let mut data = [0u8; 5];
    let cursor = Cursor::new(&mut data);

    // Should panic due to out-of-bounds
    let _ = cursor.split_at_mut(6);
}

#[test]
fn split_cursor_write_on_right() {
    let mut data = [0u8; 6];
    let cursor = Cursor::new(&mut data);
    let (_left, mut right) = cursor.split_at_mut(3);

    right.write(&[9, 9, 9]).unwrap();
    assert_eq!(right.pos, 3);
    assert_eq!(right.total_pos, 6);
    assert_eq!(&data[3..], &[9, 9, 9]);
}

#[test]
fn write_fails_if_exceeds_limit() {
    let mut data = [0u8; 5];
    let mut cursor = Cursor::new_with_limit(&mut data, 5);

    // First write OK
    cursor.write(&[1, 2, 3]).unwrap();

    // Next write would exceed limit
    assert!(cursor.write(&[4, 5, 6]).is_none());
}

#[test]
fn write_then_split() {
    let mut data = [0u8; 8];
    let mut cursor = Cursor::new(&mut data);
    cursor.write(&[1, 2, 3, 4]).unwrap();

    let (left, right_cursor) = cursor.split_at_mut_checked(5).unwrap();

    assert_eq!(left, &[1, 2, 3, 4, 0]);
    assert_eq!(right_cursor.inner.len(), 3);
    assert_eq!(right_cursor.pos, 0);
    assert_eq!(right_cursor.total_pos, 5);
}

#[test]
fn test_multiple_sequential_splits() {
    let mut data = [0u8; 10];
    let cursor = Cursor::new(&mut data);

    let (left1, cursor) = cursor.split_at_mut(3);
    let (left2, cursor) = cursor.split_at_mut(2);
    let (left3, right)   = cursor.split_at_mut(2);

    assert_eq!(left1.len(), 3);
    assert_eq!(left2.len(), 2);
    assert_eq!(left3.len(), 2);
    assert_eq!(right.inner.len(), 3);

    assert_eq!(right.pos, 0);
    assert_eq!(right.total_pos, 7);
}

#[test]
fn test_split_then_write_on_each_segment() {
    let mut data = [0u8; 10];
    let cursor = Cursor::new(&mut data);

    let (seg1, cursor) = cursor.split_at_mut(4);
    let (seg2, mut cursor) = cursor.split_at_mut(3);

    seg1.copy_from_slice(&[1, 1, 1, 1]);
    seg2.copy_from_slice(&[2, 2, 2]);

    cursor.write(&[3]).unwrap();

    assert_eq!(data, [1, 1, 1, 1, 2, 2, 2, 3, 0, 0]);
}

#[test]
fn test_back_to_back_splits_exhausting_buffer() {
    let mut data = [0u8; 6];
    let cursor = Cursor::new(&mut data);

    let (a, cursor) = cursor.split_at_mut(2);
    let (b, cursor) = cursor.split_at_mut(2);
    let (c, cursor) = cursor.split_at_mut(2);

    assert_eq!(a.len(), 2);
    assert_eq!(b.len(), 2);
    assert_eq!(c.len(), 2);
    assert_eq!(cursor.inner.len(), 0);
    assert_eq!(cursor.total_pos, 6);
}

#[test]
fn test_split_at_cursor_position_then_write() {
    let mut data = [0u8; 8];
    let mut cursor = Cursor::new(&mut data);
    cursor.try_set_position(4).unwrap();

    let (left, mut right) = cursor.split_at_mut(4);

    assert_eq!(left.len(), 4);
    assert_eq!(right.inner.len(), 4);
    assert_eq!(right.pos, 0);
    assert_eq!(right.total_pos, 4);

    right.write(&[9, 9, 9]).unwrap();
    assert_eq!(&data[4..7], &[9, 9, 9]);
}

#[test]
fn test_split_with_zero_length_remaining() {
    let mut data = [1, 2, 3];
    let cursor = Cursor::new(&mut data);

    let (left, right) = cursor.split_at_mut(3);
    assert_eq!(left, &[1, 2, 3]);
    assert_eq!(right.inner.len(), 0);
    assert_eq!(right.total_pos, 3);
    assert_eq!(right.pos, 0);
}

#[test]
fn test_split_reuse_then_backtrack_position() {
    let mut data = [0u8; 6];
    let cursor = Cursor::new(&mut data);

    let (_a, mut cursor_b) = cursor.split_at_mut(3);
    cursor_b.write(&[4, 5, 6]).unwrap();

    // Backtrack
    cursor_b.try_set_position(1).unwrap();
    assert_eq!(cursor_b.pos, 1);
    assert_eq!(cursor_b.total_pos, 4);

    // Overwrite at earlier point
    cursor_b.write(&[7, 8]).unwrap();
    assert_eq!(&data[4..6], &[7, 8]);
}
