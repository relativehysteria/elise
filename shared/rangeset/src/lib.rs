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

    /// An attempt was made to allocate memory not aligned to a power of 2
    WrongAlignment(u64),
}

/// An inclusive range. `RangeInclusive` doesn't implement `Copy`, so it's not
/// used here.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Range {
    /// Start of the range (inclusive)
    start: u64,

    /// End of the range (inclusive)
    end: u64,
}

impl Range {
    /// Returns a new range.
    ///
    /// Returns an error if the range is invalid (i.e. `start > end`).
    pub fn new(start: u64, end: u64) -> Result<Self, Error> {
        unsafe {
        (start <= end)
            .then_some(Self::new_unchecked(start, end))
            .ok_or(Error::InvalidRange(Self::new_unchecked(start, end)))
        }
    }

    /// Returns a new possibly incorrect range.
    #[inline(always)]
    unsafe fn new_unchecked(start: u64, end: u64) -> Self {
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

    /// Returns the start of this range
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Returns the inclusive end of this range
    pub fn end(&self) -> u64 {
        self.end
    }
}

/// A set of non-overlapping inclusive `Range`s.
#[derive(Clone, Debug)]
#[repr(C)]
pub struct RangeSet {
    /// Array of ranges in the set
    ranges: [Range; 256],

    /// Number of range entries in use.
    in_use: u32,
}

impl RangeSet {
    /// Returns a new empty `RangeSet`
    pub const fn new() -> Self {
        RangeSet {
            ranges:  [Range { start: 0, end: 0 }; 256],
            in_use: 0,
        }
    }

    /// Returns all the used entries in a `RangeSet`
    pub fn entries(&self) -> &[Range] {
        &self.ranges[..self.in_use as usize]
    }

    /// Compute the size of the range covered by this rangeset
    pub fn len(&self) -> Option<u64> {
        self.entries().iter().try_fold(0u64, |acc, x| {
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
        if idx >= self.in_use as usize {
            return Err(Error::IndexOutOfBounds(idx));
        }

        // Put the delete range to the end
        for i in idx..self.in_use as usize - 1 {
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
        while idx < self.in_use as usize {
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
        if self.in_use as usize >= self.ranges.len() {
            return Err(Error::RangeSetOverflow);
        }

        // Shift ranges if necessary
        if idx < self.in_use as usize {
            self.ranges.copy_within(idx..self.in_use as usize, idx + 1);
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
        while idx < self.in_use as usize {
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
        if idx >= self.in_use as usize {
            return Err(Error::IndexOutOfBounds(idx));
        }

        // Make sure we have space
        if self.in_use as usize >= self.ranges.len() {
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
        if idx + 1 < self.in_use as usize {
            self.ranges.copy_within(idx + 1..self.in_use as usize, idx + 2);
        }

        // Insert the second half in the correct position
        self.ranges[idx + 1] = Range::new(
            range.end.saturating_add(1), entry.end)?;
        self.in_use += 1;

        Ok(true)
    }

    /// Allocate `size` bytes of memory with `align` requirements, preferring to
    /// allocate from `regions`.
    ///
    /// Returns the pointer to the allocated memory.
    /// If the arguments to the function caused an unsatisfiable allocation,
    /// an error will be returned. If the allocation can't be satisfied for
    /// other reasons (i.e. there's not enough free memory), `Ok(None)` will be
    /// returned.
    pub fn allocate_prefer(
        &mut self,
        size: u64,
        align: u64,
        regions: Option<&RangeSet>
    ) -> Result<Option<u64>, Error> {
        // Don't allow 0-sized allocations
        if size == 0 { return Err(Error::ZeroSizedAllocation); }

        // Check that we have an alignment with a power of 2
        if align.count_ones() != 1 { return Err(Error::WrongAlignment(align)); }

        // Generate a mask for the alignment
        let align_mask = align - 1;

        // Go through each range and see if an allocation can fit into it
        let mut allocation = None;
        'search: for entry in self.entries() {
            // Calculate the padding
            let padding = (align - (entry.start & align_mask)) & align_mask;

            // Compute the inclusive start and end of the allocation
            let start = entry.start;
            let end   = start.checked_add(size - 1)
                .and_then(|e| e.checked_add(padding));

            // If the allocation couldn't be satisfied, stop trying
            let end = match end {
                None      => return Ok(None),
                Some(end) => end,
            };

            // Make sure this entry is large enough for the allocation
            if end > entry.end { continue; }

            // If there was a specific region the caller wanted to use,
            // check if there is overlap with this region
            if let Some(regions) = regions {
                for region in regions.entries() {
                    let overlap = match entry.overlaps(region) {
                        None    => continue,
                        Some(o) => o,
                    };

                    // Compute the rounded-up alignment from the
                    // overlapping region
                    let aligned = (overlap.start.wrapping_add(align_mask))
                        & !align_mask;

                    if aligned >= overlap.start &&
                       aligned <= overlap.end  &&
                       (overlap.end - aligned) >= (size - 1)
                    {
                        // Alignment did not cause an overflow AND
                        // Alignment did not cause exceeding the end AND
                        // Amount of aligned overlap can satisfy the
                        // allocation

                        // Compute the inclusive end of this proposed
                        // allocation
                        let alc_end = aligned + (size - 1);

                        // Make sure the allocation fits in the current
                        // addressable address space
                        let max_addr = core::usize::MAX as u64;
                        if aligned > max_addr || alc_end > max_addr {
                            continue 'search;
                        }

                        // We know the allocation can be satisfied starting
                        // at `aligned`
                        allocation = Some((aligned, alc_end, aligned));
                        break 'search;
                    }
                }
            }

            // Compute the "best" allocation size to date
            let prev_size = allocation.map(|(start, end, _)| end - start);

            if allocation.is_none() || prev_size.unwrap() > end - start {
                // Update the allocation to the new best size
                allocation = Some((start, end, start + padding));
            }
        }

        Ok(allocation.map(|(start, end, ptr)| {
            // Remove this range from the available set; it should be properly
            // validated at this point
            self.remove(Range { start, end }).unwrap();

            // Return out the pointer!
            ptr
        }))
    }

    /// Allocate `size` bytes of memory with `align` requirements.
    ///
    /// Returns the pointer to the allocated memory.
    /// If the arguments to the function caused an unsatisfiable allocation,
    /// an error will be returned. If the allocation can't be satisfied for
    /// other reasons (i.e. there's not enough free memory), `Ok(None)` will be
    /// returned.
    pub fn allocate(&mut self, size: u64, align: u64)
            -> Result<Option<u64>, Error> {
        self.allocate_prefer(size, align, None)
    }
}
