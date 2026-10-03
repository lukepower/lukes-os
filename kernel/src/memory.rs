use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use spin::Mutex;
use x86_64::structures::paging::{
    FrameAllocator, FrameDeallocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};
use core::sync::atomic::AtomicU64;

pub static FRAME_ALLOCATOR: Mutex<Option<BootInfoFrameAllocator>> = Mutex::new(None);
pub static PHYS_MEM_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Initialize a new OffsetPageTable.
///
/// # Safety
/// The caller must guarantee that the complete physical memory is mapped
/// to virtual memory at the passed `physical_memory_offset`.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = unsafe { active_level_4_table(physical_memory_offset) };
    unsafe { OffsetPageTable::new(level_4_table, physical_memory_offset) }
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    unsafe { &mut *page_table_ptr }
}

/// A contiguous range of usable physical memory.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct UsableRegion {
    pub start: u64,
    pub end: u64,
    pub current: u64,
}

/// High-performance physical frame allocator with:
/// - O(1) single-frame allocation & deallocation (intrusive free-list)
/// - O(1) contiguous multi-frame allocation (for DMA / VirtIO)
/// - Memory tracking & diagnostics
pub struct BootInfoFrameAllocator {
    regions: [Option<UsableRegion>; 32],
    region_count: usize,
    active_idx: usize,
    free_list_head: u64, // Physical address of first freed frame, 0 = empty
    total_frames: usize,
    allocated_frames: usize,
    phys_mem_offset: u64,
}

impl BootInfoFrameAllocator {
    /// Create a FrameAllocator from the passed bootloader memory regions.
    ///
    /// # Safety
    /// The caller must guarantee that the passed memory map and physical memory offset are valid.
    pub unsafe fn init(memory_regions: &'static MemoryRegions, phys_mem_offset: u64) -> Self {
        let mut regions: [Option<UsableRegion>; 32] = [None; 32];
        let mut region_count = 0;
        let mut total_frames = 0;

        for r in memory_regions.iter() {
            if r.kind == MemoryRegionKind::Usable {
                // Align start up to 4096, end down to 4096
                let start = (r.start + 0xFFF) & !0xFFF;
                let end = r.end & !0xFFF;
                if start < end && region_count < 32 {
                    let frames_in_region = ((end - start) / 4096) as usize;
                    total_frames += frames_in_region;
                    regions[region_count] = Some(UsableRegion {
                        start,
                        end,
                        current: start,
                    });
                    region_count += 1;
                }
            }
        }

        BootInfoFrameAllocator {
            regions,
            region_count,
            active_idx: 0,
            free_list_head: 0,
            total_frames,
            allocated_frames: 0,
            phys_mem_offset,
        }
    }

    /// Internal O(1) frame allocator using intrusive free-list first, then region bump.
    pub fn allocate_frame_internal(&mut self) -> Option<PhysFrame<Size4KiB>> {
        // 1. Try intrusive free list
        if self.free_list_head != 0 {
            let paddr = self.free_list_head;
            let vaddr = paddr + self.phys_mem_offset;
            unsafe {
                let next_paddr = *(vaddr as *const u64);
                self.free_list_head = next_paddr;
            }
            self.allocated_frames += 1;
            return Some(PhysFrame::containing_address(PhysAddr::new(paddr)));
        }

        // 2. Allocate from bump regions
        while self.active_idx < self.region_count {
            if let Some(ref mut region) = self.regions[self.active_idx] {
                if region.current + 4096 <= region.end {
                    let paddr = region.current;
                    region.current += 4096;
                    self.allocated_frames += 1;
                    return Some(PhysFrame::containing_address(PhysAddr::new(paddr)));
                }
            }
            self.active_idx += 1;
        }

        None
    }

    /// Allocate multiple contiguous physical frames (essential for VirtIO / DMA).
    pub fn allocate_contiguous(&mut self, pages: usize) -> Option<PhysFrame<Size4KiB>> {
        if pages == 0 {
            return None;
        }
        if pages == 1 {
            return self.allocate_frame_internal();
        }

        let needed_bytes = (pages as u64) * 4096;

        while self.active_idx < self.region_count {
            if let Some(ref mut region) = self.regions[self.active_idx] {
                if region.current + needed_bytes <= region.end {
                    let start_paddr = region.current;
                    region.current += needed_bytes;
                    self.allocated_frames += pages;
                    return Some(PhysFrame::containing_address(PhysAddr::new(start_paddr)));
                }
            }
            self.active_idx += 1;
        }

        None
    }

    /// Deallocate a single physical frame, adding it to the intrusive free list.
    pub fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        let paddr = frame.start_address().as_u64();
        let vaddr = paddr + self.phys_mem_offset;
        unsafe {
            *(vaddr as *mut u64) = self.free_list_head;
        }
        self.free_list_head = paddr;
        if self.allocated_frames > 0 {
            self.allocated_frames -= 1;
        }
    }

    /// Deallocate multiple contiguous physical frames.
    pub fn deallocate_contiguous(&mut self, start_frame: PhysFrame<Size4KiB>, pages: usize) {
        let start_addr = start_frame.start_address().as_u64();
        for i in 0..pages {
            let paddr = start_addr + (i as u64 * 4096);
            let frame = PhysFrame::containing_address(PhysAddr::new(paddr));
            self.deallocate_frame(frame);
        }
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn allocated_frames(&self) -> usize {
        self.allocated_frames
    }

    pub fn free_frames(&self) -> usize {
        self.total_frames.saturating_sub(self.allocated_frames)
    }

    pub fn total_memory_bytes(&self) -> u64 {
        (self.total_frames as u64) * 4096
    }

    pub fn free_memory_bytes(&self) -> u64 {
        (self.free_frames() as u64) * 4096
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        self.allocate_frame_internal()
    }
}

impl FrameDeallocator<Size4KiB> for BootInfoFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.deallocate_frame(frame);
    }
}
