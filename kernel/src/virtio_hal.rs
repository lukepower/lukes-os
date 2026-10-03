use core::ptr::NonNull;
use core::sync::atomic::Ordering;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};
use x86_64::VirtAddr;

use crate::memory::{FRAME_ALLOCATOR, PHYS_MEM_OFFSET};

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let mut allocator_guard = FRAME_ALLOCATOR.lock();
        let allocator = allocator_guard
            .as_mut()
            .expect("VirtIO HAL: frame allocator not initialized");

        let frame = allocator
            .allocate_contiguous(pages)
            .expect("VirtIO HAL: failed to allocate contiguous DMA memory");

        let start_paddr = frame.start_address().as_u64();
        let phys_offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
        let virt_addr = VirtAddr::new(start_paddr + phys_offset);
        let ptr = NonNull::new(virt_addr.as_mut_ptr()).unwrap();

        // Zero the allocated memory
        unsafe {
            core::ptr::write_bytes(ptr.as_ptr(), 0, pages * PAGE_SIZE);
        }

        (start_paddr, ptr)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let mut allocator_guard = FRAME_ALLOCATOR.lock();
        if let Some(allocator) = allocator_guard.as_mut() {
            let frame = x86_64::structures::paging::PhysFrame::containing_address(
                x86_64::PhysAddr::new(paddr),
            );
            allocator.deallocate_contiguous(frame, pages);
        }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        let phys_offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
        let virt_addr = VirtAddr::new(paddr + phys_offset);
        NonNull::new(virt_addr.as_mut_ptr()).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let phys_offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
        let vaddr = VirtAddr::from_ptr(buffer.as_ptr() as *mut u8);
        vaddr.as_u64() - phys_offset
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}
