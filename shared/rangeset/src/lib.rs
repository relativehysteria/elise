//! Non-overlapping sets of inclusive ranges. Useful for physical memory
//! management.

#![no_std]

#[cfg(test)]
mod tests;

use core::cmp;

/// Errors returned by the range-based routines
#[derive(Debug, PartialEq)]
pub enum Error {
    /// An attempt was made to perform an operation on an invalid [`Range`],
    /// i.e. `range.start > range.end`.
    InvalidRange(Range),

    /// An attempt was made to index into a [`RangeSet`] out of its bounds.
    IndexOutOfBounds(usize),

    /// An attempt was made to insert an entry into a [`RangeSet`] that would
    /// overflow.
    RangeSetOverflow,

    /// An attempt was made to allocate 0 bytes of memory.
    ZeroSizedAllocation,
}

/// An inclusive range. `RangeInclusive` doesn't implement `Copy`, so it's not
/// used here.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Range {
    /// Start of the range (inclusive)
    start: usize,

    /// End of the range (inclusive)
    end: usize,
}

impl Range {
    /// Returns a new range.
    ///
    /// Returns an error if the range is invalid (i.e. `start > end`).
    pub fn new(start: usize, end: usize) -> Result<Self, Error> {
        unsafe {
        (start <= end)
            .then_some(Self::new_unchecked(start, end))
            .ok_or(Error::InvalidRange(Self::new_unchecked(start, end)))
        }
    }

    /// Returns a new possibly incorrect range.
    #[inline(always)]
    unsafe fn new_unchecked(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Check whether `other` is completely contained withing this range.
    pub fn contains(&self, other: &Range) -> bool {
        // Check if `other` is completely contained within this range
        self.start <= other.start && self.end >= other.end
    }

    /// Check whether this range overlaps with another range.
    /// If it does, returns the overlap between the two ranges.
    pub fn overlaps(&self, other: &Range) -> Option<Range> {
        // Check if there is overlap
        (self.start <= other.end && other.start <= self.end)
            .then_some(unsafe { Range::new_unchecked(
                    core::cmp::max(self.start, other.start),
                    core::cmp::min(self.end, other.end)) })
    }
}

/// A set of non-overlapping inclusive `Range`s.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RangeSet<const N: usize> {
    /// Array of ranges in the set
    ranges: [Range; N],

    /// Number of range entries in use.
    in_use: usize,
}

impl<const N: usize> RangeSet<N> {
    /// Returns a new empty `RangeSet`
    pub const fn new() -> Self {
        RangeSet {
            ranges:  [Range { start: 0, end: 0 }; N],
            in_use: 0,
        }
    }

    /// Returns all the used entries in a `RangeSet`
    pub fn entries(&self) -> &[Range] {
        &self.ranges[..self.in_use]
    }

    /// Compute the size of the range covered by this rangeset
    pub fn len(&self) -> Option<usize> {
        self.entries().iter().try_fold(0usize, |acc, x| {
            Some(acc + (x.end - x.start).checked_add(1)?)
        })
    }

    /// Checks whether there are range entries in use
    pub fn is_empty(&self) -> bool {
        self.in_use == 0
    }

    /// Delete the range at `idx`
    fn delete(&mut self, idx: usize) -> Result<(), Error> {
        // Make sure we don't index out of bounds
        if idx >= self.in_use { return Err(Error::IndexOutOfBounds(idx)); }

        // Put the delete range to the end
        for i in idx..self.in_use - 1 {
            self.ranges.swap(i, i + 1);
        }

        // Decrement the number of valid ranges
        self.in_use -= 1;
        Ok(())
    }

    /// Insert a new range into the `RangeSet` while keeping it sorted.
    ///
    /// If the range overlaps with an existing range, both ranges will be merged
    /// into one.
    pub fn insert(&mut self, mut range: Range) -> Result<(), Error> {
        let mut idx = 0;
        while idx < self.in_use {
            let entry = self.ranges[idx];

            // Calculate this entry's end to check for touching
            let eend = entry.end.checked_add(1).ok_or(Error::RangeSetOverflow)?;

            // If the range starts after the current entry, continue
            if range.start > eend {
                idx += 1;
                continue;
            }

            // If the ranges don't overlap/touch, break
            if range.end < entry.start { break; }

            // At this point, there is some overlap/touch: merge the ranges
            range.start = cmp::min(entry.start, range.start);
            range.end   = cmp::max(entry.end,   range.end);

            // And delete the old overlapping range
            self.delete(idx)?;
        }

        // Ensure that our ranges don't overflow
        if self.in_use >= self.ranges.len() {
            return Err(Error::RangeSetOverflow);
        }

        // Shift ranges if necessary
        if idx < self.in_use {
            self.ranges.copy_within(idx..self.in_use, idx + 1);
        }

        // Insert the range
        self.ranges[idx] = range;
        self.in_use += 1;
        Ok(())
    }

    /// Remove a `range` from this `RangeSet`.
    ///
    /// Any range overlapping with `range` will be trimmed. Any range that is
    /// completely contained within `range` will be entirely removed.
    ///
    /// Returns `Ok(true)` if a range was altered/removed by this function call,
    /// otherwise `Ok(false)` means this call was effectively a noop.
    pub fn remove(&mut self, range: Range) -> Result<bool, Error> {
        // Track whether we have removed/altered a range within this rangeset.
        // Essentially, this remains `false` if this function call was a noop
        let mut any_removed = false;

        // Go through each entry in our ranges
        let mut idx = 0;
        while idx < self.in_use {
            let entry = self.ranges[idx];

            // If there is no overlap with this range, skip to the next entry
            if entry.overlaps(&range).is_none() {
                idx += 1;
                continue;
            }

            // We are altering/removing a range, so this function is not a noop
            any_removed = true;

            // If the entry is completely contained in the range, delete it
            if range.contains(&entry) {
                self.delete(idx)?;
                // Idx not incremented, entry has shifted
                continue;
            }

            // Handle overlaps
            if range.start <= entry.start {
                // Overlap at the start: adjust the start
                self.ranges[idx].start = range.end.saturating_add(1);
            } else if range.end >= entry.end {
                // Overlap at the end: adjust the end
                self.ranges[idx].end = range.start.saturating_sub(1);
            } else {
                // The range is fully contained within this entry;
                // split the entry in two and skip the new entry
                idx += 1 * self.split_entry(idx, range)? as usize;
            }
            idx += 1;
        }
        Ok(any_removed)
    }

    /// Split an entry into two when the `range` is fully contained within the
    /// entry at `idx`, making sure there is enough space in the rangeset for
    /// both entries. Returns `true` if an entry was in fact split and another
    /// one created and `false` if nothing happened.
    #[inline(always)]
    fn split_entry(&mut self, idx: usize, range: Range) -> Result<bool, Error> {
        // Make sure we index in bounds
        if idx >= self.in_use {
            return Err(Error::IndexOutOfBounds(idx));
        }

        // Make sure we have space
        if self.in_use >= self.ranges.len() {
            return Err(Error::RangeSetOverflow);
        }

        let entry = self.ranges[idx];

        // Make sure the entry contains the range fully
        if !entry.contains(&range) {
            return Ok(false);
        }

        // First half of the range, ensure the range doesn't become invalid
        if range.start > entry.start {
            self.ranges[idx].end = range.start.saturating_sub(1);
        } else {
            // If the range.start is exactly the start of the entry, skip
            // modifying it
            self.ranges[idx].end = entry.start;
        }

        // Shift the remaining entries to the right by one to make space
        if idx + 1 < self.in_use {
            self.ranges.copy_within(idx + 1..self.in_use, idx + 2);
        }

        // Insert the second half in the correct position
        self.ranges[idx + 1] = Range::new(
            range.end.saturating_add(1), entry.end)?;
        self.in_use += 1;

        Ok(true)
    }
}
