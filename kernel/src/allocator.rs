use linked_list_allocator::LockedHeap;
use x86_64::structures::paging::{
    mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const INITIAL_HEAP_SIZE: usize = 4 * 1024 * 1024; // 4 MiB initial heap

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the kernel heap with the initial heap size.
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + INITIAL_HEAP_SIZE as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, INITIAL_HEAP_SIZE);
    }

    Ok(())
}

/// Dynamically extend the kernel heap by mapping additional memory.
///
/// # Safety
/// `mapper` and `frame_allocator` must be active and valid page structures.
pub unsafe fn extend_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    additional_bytes: usize,
) -> Result<(), MapToError<Size4KiB>> {
    let mut heap = ALLOCATOR.lock();
    let current_top = heap.top() as usize;
    let aligned_bytes = (additional_bytes + 4095) & !4095;

    let start_page = Page::containing_address(VirtAddr::new(current_top as u64));
    let end_page = Page::containing_address(VirtAddr::new((current_top + aligned_bytes - 1) as u64));

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        mapper.map_to(page, frame, flags, frame_allocator)?.flush();
    }

    heap.extend(aligned_bytes);
    Ok(())
}

/// Returns the total current size of the heap in bytes.
pub fn heap_size() -> usize {
    ALLOCATOR.lock().size()
}

/// Returns the currently allocated bytes in the heap.
pub fn heap_used() -> usize {
    ALLOCATOR.lock().used()
}

/// Returns the free bytes remaining in the heap.
pub fn heap_free() -> usize {
    ALLOCATOR.lock().free()
}
