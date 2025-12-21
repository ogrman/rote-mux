use std::{mem::MaybeUninit, sync::atomic::AtomicUsize};

pub struct RingBuffer<T> {
    buffer: Box<[MaybeUninit<T>]>,
    capacity: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    count: AtomicUsize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            buffer: std::iter::repeat_with(MaybeUninit::uninit)
                .take(capacity)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            capacity,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, item: T) -> Result<(), T> {
        let cap = self.capacity;
        let tail = self.tail.load(std::sync::atomic::Ordering::Relaxed);
        let count = self.count.load(std::sync::atomic::Ordering::Acquire);
        if count == cap {
            // Buffer is full
            return Err(item);
        }
        unsafe {
            let ptr = self.buffer.as_ptr() as *mut MaybeUninit<T>;
            ptr.add(tail).write(MaybeUninit::new(item));
        }
        self.tail.store((tail + 1) % cap, std::sync::atomic::Ordering::Release);
        self.count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Ok(())
    }

    /// Pushes an item, overwriting the oldest entry if the buffer is full.
    pub fn force_push(&self, item: T) {
        let cap = self.capacity;
        let tail = self.tail.load(std::sync::atomic::Ordering::Relaxed);
        let count = self.count.load(std::sync::atomic::Ordering::Acquire);
        if count == cap {
            // Buffer is full, advance head to overwrite oldest
            let head = self.head.load(std::sync::atomic::Ordering::Acquire);
            self.head.store((head + 1) % cap, std::sync::atomic::Ordering::Release);
        } else {
            self.count.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        }
        unsafe {
            let ptr = self.buffer.as_ptr() as *mut MaybeUninit<T>;
            ptr.add(tail).write(MaybeUninit::new(item));
        }
        self.tail.store((tail + 1) % cap, std::sync::atomic::Ordering::Release);
    }

    pub fn pop(&self) -> Option<T> {
        let cap = self.capacity;
        let head = self.head.load(std::sync::atomic::Ordering::Relaxed);
        let count = self.count.load(std::sync::atomic::Ordering::Acquire);
        if count == 0 {
            // Buffer is empty
            return None;
        }
        let item = unsafe {
            let ptr = self.buffer.as_ptr() as *mut MaybeUninit<T>;
            ptr.add(head).read().assume_init()
        };
        self.head.store((head + 1) % cap, std::sync::atomic::Ordering::Release);
        self.count.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
        Some(item)
    }

    pub fn is_empty(&self) -> bool {
        self.count.load(std::sync::atomic::Ordering::Acquire) == 0
    }

    pub fn is_full(&self) -> bool {
        self.count.load(std::sync::atomic::Ordering::Acquire) == self.capacity
    }

    pub fn len(&self) -> usize {
        self.count.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
        #[test]
        fn test_force_push_overwrites() {
            let buf = RingBuffer::new(2);
            assert!(buf.push(1).is_ok());
            assert!(buf.push(2).is_ok());
            // Buffer is now full, force_push should overwrite the oldest (1)
            buf.force_push(3);
            assert!(buf.is_full());
            assert_eq!(buf.len(), 2);
            // 2 is now the oldest, then 3
            assert_eq!(buf.pop(), Some(2));
            assert_eq!(buf.pop(), Some(3));
            assert_eq!(buf.pop(), None);
        }
    use super::*;

    #[test]
    fn test_new_buffer_is_empty() {
        let buf: RingBuffer<i32> = RingBuffer::new(3);
        assert!(buf.is_empty());
        assert!(!buf.is_full());
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.capacity(), 3);
    }

    #[test]
    fn test_push_and_pop() {
        let buf = RingBuffer::new(2);
        assert!(buf.push(1).is_ok());
        assert!(buf.push(2).is_ok());
        assert!(buf.is_full());
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.pop(), Some(1));
        assert_eq!(buf.pop(), Some(2));
        assert!(buf.is_empty());
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn test_overwrite_when_full() {
        let buf = RingBuffer::new(2);
        assert!(buf.push(1).is_ok());
        assert!(buf.push(2).is_ok());
        assert!(buf.push(3).is_err()); // Should fail, buffer full
        buf.force_push(4); // Overwrite oldest (1)
        assert!(buf.is_full());
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.pop(), Some(2));
        assert_eq!(buf.pop(), Some(4));
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn test_mixed_operations() {
        let buf = RingBuffer::new(3);
        assert!(buf.push(1).is_ok());
        assert!(buf.push(2).is_ok());
        assert_eq!(buf.pop(), Some(1));
        assert!(buf.push(3).is_ok());
        assert!(buf.push(4).is_ok());
        assert!(buf.is_full());
        assert!(buf.push(5).is_err()); // Should fail, buffer full
        buf.force_push(6); // Overwrite oldest (2)
        assert_eq!(buf.pop(), Some(3));
        assert_eq!(buf.pop(), Some(4));
        assert_eq!(buf.pop(), Some(6));
        assert_eq!(buf.pop(), None);
    }
}
