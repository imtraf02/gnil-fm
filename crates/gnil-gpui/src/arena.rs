#![allow(unsafe_code)]

use std::{
    alloc::{self, handle_alloc_error},
    cell::Cell,
    mem::align_of,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    ptr::{self, NonNull},
    rc::Rc,
};

struct ArenaElement {
    value: *mut u8,
    drop: unsafe fn(*mut u8),
}

impl Drop for ArenaElement {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe { (self.drop)(self.value) };
    }
}

struct Chunk {
    start: *mut u8,
    end: *mut u8,
    offset: *mut u8,
    layout: alloc::Layout,
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            // SAFETY: This succeeded during allocation.
            alloc::dealloc(self.start, self.layout);
        }
    }
}

impl Chunk {
    fn new(chunk_size: NonZeroUsize, alignment: usize) -> Self {
        // this only fails if chunk_size is unreasonably huge
        let layout = alloc::Layout::from_size_align(chunk_size.get(), alignment).unwrap();
        let start = unsafe { alloc::alloc(layout) };
        if start.is_null() {
            handle_alloc_error(layout);
        }
        let end = unsafe { start.add(chunk_size.get()) };
        Self {
            start,
            end,
            offset: start,
            layout,
        }
    }

    fn allocate(&mut self, layout: alloc::Layout) -> Option<NonNull<u8>> {
        let current = self.offset.addr();
        let aligned = current.checked_add(layout.align() - 1)? & !(layout.align() - 1);
        let padding = aligned.checked_sub(current)?;
        let required = padding.checked_add(layout.size())?;
        let remaining = unsafe { self.end.offset_from_unsigned(self.offset) };
        if required > remaining {
            return None;
        }

        // SAFETY: `required <= remaining`, so both pointers stay within this allocation or
        // exactly one byte past it.
        let aligned = unsafe { self.offset.add(padding) };
        self.offset = unsafe { aligned.add(layout.size()) };
        NonNull::new(aligned)
    }

    fn reset(&mut self) {
        self.offset = self.start;
    }
}

pub struct Arena {
    chunks: Vec<Chunk>,
    elements: Vec<ArenaElement>,
    valid: Rc<Cell<bool>>,
    current_chunk_index: usize,
    chunk_size: NonZeroUsize,
}

impl Drop for Arena {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Arena {
    pub fn new(chunk_size: usize) -> Self {
        let chunk_size = NonZeroUsize::try_from(chunk_size).unwrap();
        Self {
            chunks: vec![Chunk::new(chunk_size, align_of::<u128>())],
            elements: Vec::new(),
            valid: Rc::new(Cell::new(true)),
            current_chunk_index: 0,
            chunk_size,
        }
    }

    pub fn capacity(&self) -> usize {
        self.chunks.iter().map(|chunk| chunk.layout.size()).sum()
    }

    pub fn clear(&mut self) {
        self.valid.set(false);
        self.valid = Rc::new(Cell::new(true));
        self.elements.clear();
        for chunk in &mut self.chunks {
            chunk.reset();
        }
        self.current_chunk_index = 0;
    }

    #[inline(always)]
    pub fn alloc<T>(&mut self, f: impl FnOnce() -> T) -> ArenaBox<T> {
        #[inline(always)]
        unsafe fn inner_writer<T, F>(ptr: *mut T, f: F)
        where
            F: FnOnce() -> T,
        {
            unsafe { ptr::write(ptr, f()) };
        }

        unsafe fn drop<T>(ptr: *mut u8) {
            unsafe { std::ptr::drop_in_place(ptr.cast::<T>()) };
        }

        let layout = alloc::Layout::new::<T>();
        let ptr = if layout.size() == 0 {
            NonNull::<T>::dangling().cast::<u8>().as_ptr()
        } else if let Some(ptr) = self.chunks[self.current_chunk_index].allocate(layout) {
            ptr.as_ptr()
        } else {
            let existing = (self.current_chunk_index + 1..self.chunks.len())
                .find_map(|index| self.chunks[index].allocate(layout).map(|ptr| (index, ptr)));
            if let Some((index, ptr)) = existing {
                self.current_chunk_index = index;
                ptr.as_ptr()
            } else {
                let chunk_size = NonZeroUsize::new(layout.size().max(self.chunk_size.get()))
                    .expect("non-zero layout");
                self.chunks.push(Chunk::new(
                    chunk_size,
                    layout.align().max(align_of::<u128>()),
                ));
                self.current_chunk_index = self.chunks.len() - 1;
                log::trace!(
                    "increased element arena capacity to {}kb",
                    self.capacity() / 1024,
                );
                self.chunks[self.current_chunk_index]
                    .allocate(layout)
                    .expect("new chunk was sized for this allocation")
                    .as_ptr()
            }
        };

        unsafe { inner_writer(ptr.cast(), f) };
        self.elements.push(ArenaElement {
            value: ptr,
            drop: drop::<T>,
        });

        ArenaBox {
            ptr: ptr.cast(),
            valid: self.valid.clone(),
        }
    }
}

pub struct ArenaBox<T: ?Sized> {
    ptr: *mut T,
    valid: Rc<Cell<bool>>,
}

impl<T: ?Sized> ArenaBox<T> {
    #[inline(always)]
    pub fn map<U: ?Sized>(mut self, f: impl FnOnce(&mut T) -> &mut U) -> ArenaBox<U> {
        ArenaBox {
            ptr: f(&mut self),
            valid: self.valid,
        }
    }

    #[track_caller]
    fn validate(&self) {
        assert!(
            self.valid.get(),
            "attempted to dereference an ArenaRef after its Arena was cleared"
        );
    }
}

impl<T: ?Sized> Deref for ArenaBox<T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.validate();
        unsafe { &*self.ptr }
    }
}

impl<T: ?Sized> DerefMut for ArenaBox<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.validate();
        unsafe { &mut *self.ptr }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use super::*;

    #[test]
    fn test_arena() {
        let mut arena = Arena::new(1024);
        let a = arena.alloc(|| 1u64);
        let b = arena.alloc(|| 2u32);
        let c = arena.alloc(|| 3u16);
        let d = arena.alloc(|| 4u8);
        assert_eq!(*a, 1);
        assert_eq!(*b, 2);
        assert_eq!(*c, 3);
        assert_eq!(*d, 4);

        arena.clear();
        let a = arena.alloc(|| 5u64);
        let b = arena.alloc(|| 6u32);
        let c = arena.alloc(|| 7u16);
        let d = arena.alloc(|| 8u8);
        assert_eq!(*a, 5);
        assert_eq!(*b, 6);
        assert_eq!(*c, 7);
        assert_eq!(*d, 8);

        // Ensure drop gets called.
        let dropped = Rc::new(Cell::new(false));
        struct DropGuard(Rc<Cell<bool>>);
        impl Drop for DropGuard {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        arena.alloc(|| DropGuard(dropped.clone()));
        arena.clear();
        assert!(dropped.get());
    }

    #[test]
    fn test_arena_grow() {
        let mut arena = Arena::new(8);
        arena.alloc(|| 1u64);
        arena.alloc(|| 2u64);

        assert_eq!(arena.capacity(), 16);

        arena.alloc(|| 3u32);
        arena.alloc(|| 4u32);

        assert_eq!(arena.capacity(), 24);
    }

    #[test]
    fn test_arena_alignment() {
        let mut arena = Arena::new(256);
        let x1 = arena.alloc(|| 1u8);
        let x2 = arena.alloc(|| 2u16);
        let x3 = arena.alloc(|| 3u32);
        let x4 = arena.alloc(|| 4u64);
        let x5 = arena.alloc(|| 5u64);

        assert_eq!(*x1, 1);
        assert_eq!(*x2, 2);
        assert_eq!(*x3, 3);
        assert_eq!(*x4, 4);
        assert_eq!(*x5, 5);

        assert_eq!(x1.ptr.align_offset(std::mem::align_of_val(&*x1)), 0);
        assert_eq!(x2.ptr.align_offset(std::mem::align_of_val(&*x2)), 0);
    }

    #[test]
    fn test_arena_overaligned_allocation_larger_than_chunk() {
        #[repr(align(4096))]
        struct Overaligned(u8);

        let mut arena = Arena::new(8);
        let value = arena.alloc(|| Overaligned(7));

        assert_eq!(value.0, 7);
        assert_eq!(value.ptr.align_offset(4096), 0);
        assert!(arena.capacity() >= 4096);
    }

    #[test]
    fn test_arena_zero_sized_value_is_dropped() {
        struct ZeroSized;

        static DROPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        impl Drop for ZeroSized {
            fn drop(&mut self) {
                DROPPED.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }

        DROPPED.store(false, std::sync::atomic::Ordering::Relaxed);
        let mut arena = Arena::new(8);
        arena.alloc(|| ZeroSized);
        arena.clear();
        assert!(DROPPED.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    #[should_panic(expected = "attempted to dereference an ArenaRef after its Arena was cleared")]
    fn test_arena_use_after_clear() {
        let mut arena = Arena::new(16);
        let value = arena.alloc(|| 1u64);

        arena.clear();
        let _read_value = *value;
    }
}
