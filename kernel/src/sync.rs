use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use alloc::collections::VecDeque;

/// A fair FIFO Ticket Spinlock to prevent core starvation on SMP.
pub struct TicketLock<T> {
    next_ticket: AtomicUsize,
    now_serving: AtomicUsize,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for TicketLock<T> {}
unsafe impl<T: Send> Send for TicketLock<T> {}

pub struct TicketLockGuard<'a, T> {
    lock: &'a TicketLock<T>,
}

impl<T> TicketLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            next_ticket: AtomicUsize::new(0),
            now_serving: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> TicketLockGuard<'_, T> {
        let ticket = self.next_ticket.fetch_add(1, Ordering::Relaxed);
        while self.now_serving.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
        }
        TicketLockGuard { lock: self }
    }

    pub fn try_lock(&self) -> Option<TicketLockGuard<'_, T>> {
        let current = self.now_serving.load(Ordering::Acquire);
        if self.next_ticket.compare_exchange(
            current,
            current + 1,
            Ordering::Acquire,
            Ordering::Relaxed,
        ).is_ok() {
            Some(TicketLockGuard { lock: self })
        } else {
            None
        }
    }

    fn unlock(&self) {
        self.now_serving.fetch_add(1, Ordering::Release);
    }
}

impl<'a, T> Deref for TicketLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for TicketLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for TicketLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock.unlock();
    }
}

/// A wait queue that tracks threads blocked on a synchronization condition.
pub struct WaitQueue {
    waiters: TicketLock<VecDeque<crate::thread::ThreadId>>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            waiters: TicketLock::new(VecDeque::new()),
        }
    }

    pub fn sleep(&self) {
        let current_id = crate::scheduler::current_thread_id();
        self.waiters.lock().push_back(current_id);
        crate::scheduler::block_current_thread();
    }

    pub fn wake_one(&self) {
        if let Some(id) = self.waiters.lock().pop_front() {
            crate::scheduler::unblock_thread(id);
        }
    }

    pub fn wake_all(&self) {
        let mut guard = self.waiters.lock();
        while let Some(id) = guard.pop_front() {
            crate::scheduler::unblock_thread(id);
        }
    }
}

/// A sleeping mutex that deschedules threads on contention instead of burning CPU cycles.
pub struct SleepingMutex<T> {
    locked: AtomicBool,
    wait_queue: WaitQueue,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SleepingMutex<T> {}
unsafe impl<T: Send> Send for SleepingMutex<T> {}

pub struct SleepingMutexGuard<'a, T> {
    mutex: &'a SleepingMutex<T>,
}

impl<T> SleepingMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            wait_queue: WaitQueue::new(),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SleepingMutexGuard<'_, T> {
        while self.locked.swap(true, Ordering::Acquire) {
            // Contended: put current thread to sleep
            self.wait_queue.sleep();
        }
        SleepingMutexGuard { mutex: self }
    }

    pub fn try_lock(&self) -> Option<SleepingMutexGuard<'_, T>> {
        if !self.locked.swap(true, Ordering::Acquire) {
            Some(SleepingMutexGuard { mutex: self })
        } else {
            None
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
        self.wait_queue.wake_one();
    }
}

impl<'a, T> Deref for SleepingMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> DerefMut for SleepingMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T> Drop for SleepingMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.unlock();
    }
}
