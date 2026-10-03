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

/// Create an OffsetPageTable for an arbitrary PML4 physical frame.
pub unsafe fn page_table_for_frame(
    pml4_frame: PhysFrame<Size4KiB>,
    physical_memory_offset: VirtAddr,
) -> OffsetPageTable<'static> {
    let phys = pml4_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { OffsetPageTable::new(&mut *page_table_ptr, physical_memory_offset) }
}

/// Allocate a new user PML4 table with lower-half (0..256) zeroed and upper-half (256..512)
/// mirrored from the kernel PML4 table.
pub fn new_user_page_table(
    frame_allocator: &mut BootInfoFrameAllocator,
) -> Option<PhysFrame<Size4KiB>> {
    let pml4_frame = frame_allocator.allocate_frame_internal()?;
    let offset = PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    let user_pml4_virt = (offset + pml4_frame.start_address().as_u64()) as *mut PageTable;

    use x86_64::registers::control::Cr3;
    let (kernel_pml4_frame, _) = Cr3::read();
    let kernel_pml4_virt = (offset + kernel_pml4_frame.start_address().as_u64()) as *const PageTable;

    unsafe {
        let user_table = &mut *user_pml4_virt;
        let kernel_table = &*kernel_pml4_virt;

        // Zero out lower half (entries 0..256: 0x0000_0000_0000_0000 .. 0x0000_7FFF_FFFF_FFFF)
        for i in 0..256 {
            user_table[i].set_unused();
        }

        // Copy kernel mappings in upper half (entries 256..512)
        for i in 256..512 {
            user_table[i] = kernel_table[i].clone();
        }

        // Copy lower-half kernel mappings (e.g. entry 32 = kernel text/data, entry 136 = heap/backbuffer)
        // User processes only occupy entry 0 (code/data/bss) and entry 255 (user stack).
        for i in 1..255 {
            if !kernel_table[i].is_unused() {
                user_table[i] = kernel_table[i].clone();
            }
        }
    }

    Some(pml4_frame)
}

/// Free all user frames mapped in the user entries of the given PML4 and deallocate the PML4 frame.
pub unsafe fn free_user_page_table(
    pml4_frame: PhysFrame<Size4KiB>,
    frame_allocator: &mut BootInfoFrameAllocator,
) {
    let offset = PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    let pml4_virt = (offset + pml4_frame.start_address().as_u64()) as *mut PageTable;
    let pml4 = &mut *pml4_virt;

    // Walk user-private entries 0 and 255 only (entries 1..255 and 256..512 are shared kernel mappings)
    for &i in &[0, 255] {
        if !pml4[i].is_unused() && pml4[i].flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
            let pdpt_frame = pml4[i].frame().unwrap();
            let pdpt_virt = (offset + pdpt_frame.start_address().as_u64()) as *mut PageTable;
            let pdpt = &mut *pdpt_virt;

            for j in 0..512 {
                if !pdpt[j].is_unused() && pdpt[j].flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
                    if pdpt[j].flags().contains(x86_64::structures::paging::PageTableFlags::HUGE_PAGE) {
                        // 1 GiB huge page
                        frame_allocator.deallocate_contiguous(pdpt[j].frame().unwrap(), 512 * 512);
                    } else {
                        let pd_frame = pdpt[j].frame().unwrap();
                        let pd_virt = (offset + pd_frame.start_address().as_u64()) as *mut PageTable;
                        let pd = &mut *pd_virt;

                        for k in 0..512 {
                            if !pd[k].is_unused() && pd[k].flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
                                if pd[k].flags().contains(x86_64::structures::paging::PageTableFlags::HUGE_PAGE) {
                                    // 2 MiB huge page
                                    frame_allocator.deallocate_contiguous(pd[k].frame().unwrap(), 512);
                                } else {
                                    let pt_frame = pd[k].frame().unwrap();
                                    let pt_virt = (offset + pt_frame.start_address().as_u64()) as *mut PageTable;
                                    let pt = &mut *pt_virt;

                                    for l in 0..512 {
                                        if !pt[l].is_unused() && pt[l].flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
                                            frame_allocator.deallocate_frame(pt[l].frame().unwrap());
                                        }
                                    }
                                    frame_allocator.deallocate_frame(pt_frame);
                                }
                            }
                        }
                        frame_allocator.deallocate_frame(pd_frame);
                    }
                }
            }
            frame_allocator.deallocate_frame(pdpt_frame);
        }
    }

    frame_allocator.deallocate_frame(pml4_frame);
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

/// Query current physical frame allocator metrics (total, allocated, free bytes).
pub fn physical_memory_stats() -> Option<(u64, u64, u64)> {
    let guard = FRAME_ALLOCATOR.lock();
    guard.as_ref().map(|alloc| {
        let total = alloc.total_memory_bytes();
        let free = alloc.free_memory_bytes();
        let used = total.saturating_sub(free);
        (total, used, free)
    })
}
