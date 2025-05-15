//! A cursor abstraction over a mutable byte slice buffer.
//!
//! This cursor differs from `std::io::Cursor` in that it supports additional
//! functionality needed for fine-grained memory operations, such as write limits
//! and safe splitting of the buffer while maintaining consistent cursor state.

#[cfg(test)] mod tests;

/// A cursor over a mutable byte slice that supports write limiting and accurate
/// position tracking even across buffer splits.
///
/// Unlike `std::io::Cursor`, this implementation allows:
/// - Enforcing a strict write limit that, when exceeded, causes writes to fail.
/// - Splitting the buffer using `split_at()` methods while maintaining a correct
///   `total_pos`, representing the absolute position across the original buffer.
pub struct Cursor<'a, T: Copy> {
    /// The current mutable view of the buffer slice
    inner: &'a mut [T],

    /// Maximum number of bytes that can be written through this cursor over all
    /// splits
    limit: usize,

    /// Position within the current buffer slice
    pos: usize,

    /// Absolute position within the original unsplit buffer
    total_pos: usize,
}

impl<'a, T: Copy> Cursor<'a, T> {
    /// Creates a new cursor wrapping the provided underlying in-memory buffer.
    pub fn new(inner: &'a mut [T]) -> Self {
        Self {
            inner,
            pos: 0,
            total_pos: 0,
            limit: usize::MAX,
        }
    }

    /// Creates a new cursor wrapping the provided underlying in-memory buffer,
    /// setting the maximum write limit of the buffer to `limit`
    pub fn new_with_limit(inner: &'a mut [T], limit: usize) -> Self {
        Self {
            inner,
            pos: 0,
            total_pos: 0,
            limit,
        }
    }

    /// Consumes this cursor and return the underlying buffer
    pub fn into_inner(self) -> &'a mut [T] {
        self.inner
    }

    /// Gets a reference to the underlying buffer
    pub const fn get_ref(&self) -> &[T] {
        &*self.inner
    }

    /// Gets a mutable reference to the underlying buffer
    pub const fn get_mut(&mut self) -> &mut [T] {
        self.inner
    }

    /// Gets the current position of this cursor into the underlying buffer
    pub const fn current_position(&self) -> usize {
        self.pos
    }

    /// Gets the current position of the cursor over all splits
    pub const fn overall_position(&self) -> usize {
        self.total_pos
    }

    /// Sets the position of the cursor over all splits, panicking if the
    /// position goes over the limit
    pub const fn set_position(&mut self, pos: usize) {
        self.try_set_position(pos)
            .expect("Attempted to set impossible position");
    }

    /// Attempts to set the raw position of the current cursor, returning the
    /// new total position over all cursors
    ///
    /// This will fail if the total cursor position over all splits goes over
    /// the cursor's limit, or if `pos` goes over the length of the underlying
    /// buffer.
    pub const fn try_set_position(&mut self, pos: usize) -> Option<usize> {
        // If we're not changing the position, that's a successful noop
        if pos == self.pos {
            Some(self.total_pos)

        // If we're setting the position further than it is, make sure we're
        // not going over the limit
        } else if pos > self.pos {
            let total = self.total_pos + (pos - self.pos);
            if total > self.limit {
                return None;
            }

            self.total_pos = total;
            self.pos = pos;
            Some(self.total_pos)

        // If we're truncating the current buffer, adjust the positions
        } else {
            let delta = self.pos - pos;
            self.total_pos -= delta;
            self.pos = pos;
            Some(self.total_pos)
        }
    }

    /// Gets the current size limit for the underlying buffer
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Sets the current size limit for the underlying buffer, panicking if the
    /// limit is lower than the current position
    pub const fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
    }

    /// Attempts to set the limit of this cursor
    ///
    /// Returns `None` if the limit is lower than the current position
    pub const fn try_set_limit(&mut self, limit: usize) -> Option<()> {
        if limit < self.pos {
            None
        } else {
            Some(())
        }
    }

    /// Append the contents `buf` to the end of the underlying buffer
    ///
    /// On success, returns the position before the write and after the write
    pub fn write(&mut self, buf: &[T]) -> Option<(usize, usize)> {
        // Set the new position
        let cur_pos = self.pos;
        let new_pos = cur_pos.checked_add(buf.len())?;
        self.try_set_position(new_pos)?;

        // Copy the buffer contents
        self.inner[cur_pos..new_pos].copy_from_slice(buf);

        Some((cur_pos, new_pos))
    }

    /// Splits the current buffer at the given index, returning a mutable slice
    /// up to that point and a new cursor over the remaining buffer.
    ///
    /// * The cursor state is preserved such that the total position and
    ///   relative position get updated correctly.
    /// * If `raw_idx > pos`, the region between the cursor's current position
    ///   and `raw_idx` is considered initialized
    /// * The new cursor's position will be reset relative to the new buffer,
    ///   while the total position is mainained across the split.
    ///
    /// Will panic if the split would result in an out-of-bounds access or if
    /// the total position would overflow its limit.
    pub fn split_at(self, raw_idx: usize) -> (&'a mut [T], Self) {
        self.split_at_checked(raw_idx)
            .expect("Attempted to split cursor with overflow")
    }

    /// Non-panic version of `split_at()`
    pub fn split_at_checked(mut self, raw_idx: usize)
            -> Option<(&'a mut [T], Self)> {
        // Don't overflow the buffer length
        if raw_idx > self.inner.len() {
            return None;
        }

        // If we're splitting past the current position, we'll need to assume
        // that the left side of the split is "initialized" and as such will
        // write the whole of its length as so
        let (new_total, new_cur) = if raw_idx > self.pos {
            (self.total_pos.checked_add(raw_idx - self.pos)?, 0)
        } else {
            (self.total_pos, self.pos - raw_idx)
        };

        // Don't overflow the limit
        if new_total > self.limit {
            return None;
        }

        // Split and adjust the inner variables
        let (left, right) = self.inner.split_at_mut(raw_idx);
        self.inner = right;
        self.total_pos = new_total;
        self.pos = new_cur;

        Some((left, self))
    }
}
