use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTableFlags, Size4KiB,
};
use x86_64::VirtAddr;

use crate::memory;
use crate::serial_println;
use crate::vfs;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELF_CLASS_64: u8 = 2;
const ELF_DATA_2LSB: u8 = 1; // Little endian
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const PT_LOAD: u32 = 1;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ElfHeader64 {
    pub ident: [u8; 16],
    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub ph_offset: u64,
    pub sh_offset: u64,
    pub flags: u32,
    pub eh_size: u16,
    pub ph_ent_size: u16,
    pub ph_num: u16,
    pub sh_ent_size: u16,
    pub sh_num: u16,
    pub sh_str_ndx: u16,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ProgramHeader64 {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

#[derive(Debug)]
pub enum ElfError {
    InvalidMagic,
    Not64Bit,
    NotLittleEndian,
    UnsupportedType,
    MalformedHeader,
    AllocationFailed,
    IoError,
    MapFailed(Page<Size4KiB>),
    InvalidVaddr(u64),
}

/// Parsed ELF binary representation.
pub struct LoadedElf {
    pub entry_point: u64,
    pub user_stack_top: u64,
}

/// Parse and validate an ELF64 binary in memory, mapping its PT_LOAD segments.
pub fn load_elf(
    elf_bytes: &[u8],
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<LoadedElf, ElfError> {
    if elf_bytes.len() < core::mem::size_of::<ElfHeader64>() {
        return Err(ElfError::MalformedHeader);
    }

    // Verify Magic
    if elf_bytes[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidMagic);
    }

    if elf_bytes[4] != ELF_CLASS_64 {
        return Err(ElfError::Not64Bit);
    }

    if elf_bytes[5] != ELF_DATA_2LSB {
        return Err(ElfError::NotLittleEndian);
    }

    let header_ptr = elf_bytes.as_ptr() as *const ElfHeader64;
    let header = unsafe { core::ptr::read_unaligned(header_ptr) };

    if header.elf_type != ET_EXEC && header.elf_type != ET_DYN {
        return Err(ElfError::UnsupportedType);
    }

    let ph_offset = header.ph_offset as usize;
    let ph_num = header.ph_num as usize;
    let ph_size = header.ph_ent_size as usize;

    if ph_offset + (ph_num * ph_size) > elf_bytes.len() {
        return Err(ElfError::MalformedHeader);
    }

    // Process program headers (PT_LOAD segments)
    for i in 0..ph_num {
        let entry_offset = ph_offset + (i * ph_size);
        let ph_ptr = unsafe { elf_bytes.as_ptr().add(entry_offset) as *const ProgramHeader64 };
        let ph = unsafe { core::ptr::read_unaligned(ph_ptr) };

        if ph.p_type == PT_LOAD && ph.p_memsz > 0 {
            load_segment(&ph, elf_bytes, mapper, frame_allocator)?;
        }
    }

    // Allocate 32 KiB User Stack at 0x0000_7FFF_FFFF_0000
    let user_stack_top: u64 = 0x0000_7FFF_FFFF_0000;
    let stack_pages = 8;
    for page_idx in 0..stack_pages {
        let vaddr = user_stack_top - ((page_idx + 1) * 4096);
        let page = Page::containing_address(VirtAddr::new(vaddr));
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(ElfError::AllocationFailed)?;

        let flags = PageTableFlags::PRESENT
            | PageTableFlags::WRITABLE
            | PageTableFlags::USER_ACCESSIBLE;

        unsafe {
            if let Ok(map) = mapper.map_to(page, frame, flags, frame_allocator) {
                map.flush();
            }
        }
    }

    Ok(LoadedElf {
        entry_point: header.entry,
        user_stack_top,
    })
}

fn load_segment(
    ph: &ProgramHeader64,
    elf_bytes: &[u8],
    mapper: &mut OffsetPageTable<'static>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), ElfError> {
    let start_vaddr = ph.p_vaddr;
    let end_vaddr = start_vaddr + ph.p_memsz;

    // Reject segments with p_vaddr outside 0x0000_0000_0040_0000 .. 0x0000_7FFF_0000_0000
    if start_vaddr < 0x0040_0000 || end_vaddr > 0x0000_7FFF_0000_0000 {
        return Err(ElfError::InvalidVaddr(start_vaddr));
    }

    let start_page = Page::containing_address(VirtAddr::new(start_vaddr));
    let end_page = Page::containing_address(VirtAddr::new(end_vaddr.saturating_sub(1)));

    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if (ph.p_flags & 2) != 0 {
        // Writable
        flags |= PageTableFlags::WRITABLE;
    }
    if (ph.p_flags & 1) == 0 {
        // No execute
        flags |= PageTableFlags::NO_EXECUTE;
    }

    let file_start = ph.p_offset as usize;
    let file_end = file_start + (ph.p_filesz as usize);
    let segment_file_data = if file_end <= elf_bytes.len() {
        &elf_bytes[file_start..file_end]
    } else {
        return Err(ElfError::MalformedHeader);
    };

    let offset_phys = memory::PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(ElfError::AllocationFailed)?;

        // Zero out frame in physical memory offset
        let virt_frame = offset_phys + frame.start_address().as_u64();
        unsafe {
            core::ptr::write_bytes(virt_frame as *mut u8, 0, 4096);
        }

        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .map_err(|_| ElfError::MapFailed(page))?
                .flush();
        }

        // Copy matching segment data if within range
        let page_start_vaddr = page.start_address().as_u64();
        let page_end_vaddr = page_start_vaddr + 4096;

        if page_start_vaddr < (start_vaddr + ph.p_filesz) && page_end_vaddr > start_vaddr {
            let copy_start = core::cmp::max(page_start_vaddr, start_vaddr);
            let copy_end = core::cmp::min(page_end_vaddr, start_vaddr + ph.p_filesz);
            let copy_len = (copy_end - copy_start) as usize;

            let src_offset = (copy_start - start_vaddr) as usize;
            let dst_offset = (copy_start - page_start_vaddr) as usize;

            unsafe {
                core::ptr::copy_nonoverlapping(
                    segment_file_data[src_offset..src_offset + copy_len].as_ptr(),
                    (virt_frame + dst_offset as u64) as *mut u8,
                    copy_len,
                );
            }
        }
    }

    Ok(())
}

/// Low-level trampoline to execute `iretq` with user segment registers.
#[unsafe(naked)]
unsafe extern "C" fn user_jump_trampoline(
    _rip: u64,
    _cs: u64,
    _rflags: u64,
    _rsp: u64,
    _ss: u64,
) -> ! {
    // Calling convention (System V):
    // rdi = rip, rsi = cs, rdx = rflags, rcx = rsp, r8 = ss
    core::arch::naked_asm!(
        "push r8",  // ss
        "push rcx", // rsp
        "push rdx", // rflags
        "push rsi", // cs
        "push rdi", // rip
        "swapgs",
        "iretq",
    );
}

/// Transition to User Space (Ring 3) via sysretq / iretq.
/// Sets CS=0x20|3 (User Code), SS=0x18|3 (User Data), RFLAGS=0x202 (IF=1).
pub unsafe fn enter_user_mode(entry_point: u64, user_rsp: u64) -> ! {
    let core_id = crate::smp::PerCpu::current().core_id;
    let selectors = crate::gdt::selectors(core_id);

    // Ensure kernel privilege stack (RSP0) is set for returns from Ring 3
    let kstack = crate::smp::PerCpu::current().kernel_syscall_stack;
    crate::gdt::set_rsp0(core_id, VirtAddr::new(kstack));

    let user_cs = (selectors.user_code_selector.0 | 3) as u64;
    let user_ss = (selectors.user_data_selector.0 | 3) as u64;
    let rflags: u64 = 0x202; // IF=1, reserved=1

    serial_println!(
        "[USER] Dropping to Ring 3: RIP=0x{:X}, RSP=0x{:X}, CS=0x{:X}, SS=0x{:X}",
        entry_point, user_rsp, user_cs, user_ss
    );

    user_jump_trampoline(entry_point, user_cs, rflags, user_rsp, user_ss);
}

/// Load an ELF binary by VFS path and execute in Ring 3 as a scheduler thread.
pub fn spawn_user_process(path: &str) -> Result<crate::thread::ThreadId, &'static str> {
    let mut handle = vfs::open(path, vfs::OpenFlags::READ).map_err(|_| "Failed to open file")?;
    let meta = handle.metadata().map_err(|_| "Failed to read metadata")?;
    let mut buffer = alloc::vec![0u8; meta.size as usize];
    handle.read(&mut buffer).map_err(|_| "Failed to read file")?;

    let offset = memory::PHYS_MEM_OFFSET.load(core::sync::atomic::Ordering::Relaxed);
    let mut mapper = unsafe { memory::init(VirtAddr::new(offset)) };
    let mut frame_guard = memory::FRAME_ALLOCATOR.lock();
    let frame_allocator = frame_guard.as_mut().ok_or("No frame allocator")?;

    let loaded = load_elf(&buffer, &mut mapper, frame_allocator).map_err(|_| "ELF loading failed")?;

    drop(frame_guard);

    let tid = crate::scheduler::spawn_user("user_proc", loaded.entry_point, loaded.user_stack_top);
    serial_println!(
        "[USER] Spawned user thread TID={} for '{}': RIP=0x{:X}, RSP=0x{:X}",
        tid.0, path, loaded.entry_point, loaded.user_stack_top
    );
    Ok(tid)
}
