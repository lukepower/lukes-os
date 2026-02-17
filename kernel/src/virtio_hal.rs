use core::ptr::NonNull;
use core::sync::atomic::Ordering;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};
use x86_64::{structures::paging::FrameAllocator, VirtAddr};

use crate::memory::{FRAME_ALLOCATOR, PHYS_MEM_OFFSET};

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let mut allocator = FRAME_ALLOCATOR.lock();
        let allocator = allocator
            .as_mut()
            .expect("VirtIO HAL: frame allocator not initialized");

        let mut start_paddr = 0;

        for i in 0..pages {
            let frame = allocator
                .allocate_frame()
                .expect("VirtIO HAL: failed to allocate DMA memory");

            if i == 0 {
                start_paddr = frame.start_address().as_u64();
            } else {
                let expected_addr = start_paddr + (i as u64 * PAGE_SIZE as u64);
                if frame.start_address().as_u64() != expected_addr {
                    panic!("VirtIO HAL: allocator returned non-contiguous memory for multi-page allocation");
                }
            }
        }

        let phys_offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
        let virt_addr = VirtAddr::new(start_paddr + phys_offset);
        let ptr = NonNull::new(virt_addr.as_mut_ptr()).unwrap();

        // Zero the allocated memory
        unsafe {
            core::ptr::write_bytes(ptr.as_ptr(), 0, pages * PAGE_SIZE);
        }

        (start_paddr, ptr)
    }

    unsafe fn dma_dealloc(_paddr: PhysAddr, _vaddr: NonNull<u8>, _pages: usize) -> i32 {
        // Leaking memory for now (allocator doesn't support deallocation yet)
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
