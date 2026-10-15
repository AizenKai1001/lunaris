//! Type-erased column of plain-data components, stored contiguously (structure of arrays per archetype).
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::ptr::{self, NonNull};

pub struct Column {
    ptr: NonNull<u8>,
    len: usize,
    cap: usize,
    size: usize,
    align: usize,
}

// SAFETY: a column owns its allocation and stores plain data only (components are `Copy`, no interior pointers).
unsafe impl Send for Column {}
unsafe impl Sync for Column {}

impl Column {
    pub fn new(size: usize, align: usize) -> Column {
        // Dangling but aligned pointer: valid for zero-sized reads and the base for zero-sized components.
        let dangling = NonNull::new(align as *mut u8).expect("alignment is non-zero");
        Column { ptr: dangling, len: 0, cap: if size == 0 { usize::MAX } else { 0 }, size, align }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Size in bytes of one element.
    pub fn elem_size(&self) -> usize {
        self.size
    }

    #[inline]
    pub fn base(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn ptr_at(&self, row: usize) -> *mut u8 {
        debug_assert!(row < self.len || self.size == 0);
        // SAFETY: row is in bounds (checked above in debug builds; callers keep rows valid).
        unsafe { self.ptr.as_ptr().add(row * self.size) }
    }

    fn grow(&mut self, min_cap: usize) {
        if self.size == 0 || min_cap <= self.cap {
            return;
        }
        let new_cap = min_cap.max(self.cap * 2).max(8);
        let new_bytes = new_cap.checked_mul(self.size).expect("column too large");
        let new_layout = Layout::from_size_align(new_bytes, self.align).expect("bad layout");
        // SAFETY: layouts are consistent between alloc/realloc/dealloc; non-zero size here.
        let p = unsafe {
            if self.cap == 0 {
                alloc(new_layout)
            } else {
                let old = Layout::from_size_align_unchecked(self.cap * self.size, self.align);
                realloc(self.ptr.as_ptr(), old, new_bytes)
            }
        };
        self.ptr = NonNull::new(p).unwrap_or_else(|| std::alloc::handle_alloc_error(new_layout));
        self.cap = new_cap;
    }

    pub fn reserve(&mut self, additional: usize) {
        self.grow(self.len + additional);
    }

    /// Append a copy of `size` bytes from `src`.
    ///
    /// # Safety
    /// `src` must point to `size` readable bytes forming a valid component value.
    pub unsafe fn push_copy(&mut self, src: *const u8) {
        self.grow(self.len + 1);
        // SAFETY: capacity ensured above; src does not overlap the destination row.
        unsafe { ptr::copy_nonoverlapping(src, self.ptr.as_ptr().add(self.len * self.size), self.size) };
        self.len += 1;
    }

    /// Remove `row` by moving the last row into its place. Returns true when a different row was moved.
    pub fn swap_remove(&mut self, row: usize) -> bool {
        assert!(row < self.len, "row out of bounds");
        let last = self.len - 1;
        let moved = row != last;
        if moved && self.size != 0 {
            // SAFETY: both rows are in bounds and distinct.
            unsafe { ptr::copy_nonoverlapping(self.ptr.as_ptr().add(last * self.size), self.ptr.as_ptr().add(row * self.size), self.size) };
        }
        self.len -= 1;
        moved
    }

    /// Drop the last row without moving anything.
    pub fn pop(&mut self) {
        assert!(self.len > 0);
        self.len -= 1;
    }
}

impl Clone for Column {
    fn clone(&self) -> Column {
        let mut c = Column::new(self.size, self.align);
        if self.size == 0 {
            c.len = self.len;
            return c;
        }
        c.grow(self.len.max(1));
        // SAFETY: both allocations hold at least `len` rows and do not overlap.
        unsafe { ptr::copy_nonoverlapping(self.ptr.as_ptr(), c.ptr.as_ptr(), self.len * self.size) };
        c.len = self.len;
        c
    }
}

impl Drop for Column {
    fn drop(&mut self) {
        if self.size != 0 && self.cap != 0 {
            // SAFETY: allocated with this exact layout in `grow`.
            unsafe { dealloc(self.ptr.as_ptr(), Layout::from_size_align_unchecked(self.cap * self.size, self.align)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_swap_remove_clone() {
        let mut c = Column::new(8, 8);
        for i in 0..100u64 {
            unsafe { c.push_copy((&i as *const u64).cast()) };
        }
        assert_eq!(c.len(), 100);
        assert!(c.swap_remove(3));
        assert_eq!(unsafe { c.ptr_at(3).cast::<u64>().read() }, 99);
        let d = c.clone();
        assert_eq!(d.len(), 99);
        assert_eq!(unsafe { d.ptr_at(98).cast::<u64>().read() }, 98);
        assert!(!c.swap_remove(98));
    }

    #[test]
    fn zero_sized_columns() {
        let mut c = Column::new(0, 1);
        unsafe { c.push_copy(1 as *const u8) };
        unsafe { c.push_copy(1 as *const u8) };
        assert_eq!(c.len(), 2);
        c.swap_remove(0);
        assert_eq!(c.clone().len(), 1);
    }
}
