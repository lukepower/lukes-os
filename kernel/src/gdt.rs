use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::instructions::tables::load_tss;
use x86_64::instructions::segmentation::{CS, Segment};
use lazy_static::lazy_static;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const MAX_CPUS: usize = 8;
const STACK_SIZE: usize = 4096 * 5;

#[derive(Clone, Copy)]
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub user_data_selector: SegmentSelector,
    pub user_code_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut CPU_STACKS: [[u8; STACK_SIZE]; MAX_CPUS] = [[0; STACK_SIZE]; MAX_CPUS];
static mut RAW_TSS_ARRAY: [TaskStateSegment; MAX_CPUS] = [TaskStateSegment::new(); MAX_CPUS];

lazy_static! {
    static ref CPU_GDTS: [(GlobalDescriptorTable, Selectors); MAX_CPUS] = {
        unsafe {
            for (i, tss) in RAW_TSS_ARRAY.iter_mut().enumerate() {
                let stack_start = VirtAddr::from_ptr(CPU_STACKS[i].as_ptr());
                let stack_end = stack_start + STACK_SIZE as u64;
                tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;
            }
        }

        core::array::from_fn(|i| {
            let mut gdt = GlobalDescriptorTable::new();
            let code_selector = gdt.append(Descriptor::kernel_code_segment());
            let data_selector = gdt.append(Descriptor::kernel_data_segment());
            let user_data_selector = gdt.append(Descriptor::user_data_segment());
            let user_code_selector = gdt.append(Descriptor::user_code_segment());
            let tss_selector = gdt.append(Descriptor::tss_segment(unsafe { &RAW_TSS_ARRAY[i] }));
            (gdt, Selectors {
                code_selector,
                data_selector,
                user_data_selector,
                user_code_selector,
                tss_selector,
            })
        })
    };
}

/// Set the kernel privilege stack (RSP0) in the TSS for the given core upon user-to-kernel entry.
pub fn set_rsp0(core_id: usize, rsp0: VirtAddr) {
    if core_id < MAX_CPUS {
        unsafe {
            RAW_TSS_ARRAY[core_id].privilege_stack_table[0] = rsp0;
        }
    }
}

/// Retrieve the selectors for a specific CPU core.
pub fn selectors(core_id: usize) -> Selectors {
    CPU_GDTS[core_id % MAX_CPUS].1
}

pub fn init_cpu(core_id: usize) {
    if core_id >= MAX_CPUS {
        panic!("core_id {} exceeds MAX_CPUS", core_id);
    }

    let (gdt, selectors) = &CPU_GDTS[core_id];
    gdt.load();
    unsafe {
        CS::set_reg(selectors.code_selector);
        load_tss(selectors.tss_selector);
    }
}

pub fn init() {
    init_cpu(0);
}
