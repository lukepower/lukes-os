use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::instructions::tables::load_tss;
use x86_64::instructions::segmentation::{CS, Segment, DS, ES, SS};

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const MAX_CPUS: usize = 8;
const STACK_SIZE: usize = 4096 * 4;

struct CpuGdtEntry {
    tss: TaskStateSegment,
    stack: [u8; STACK_SIZE],
    gdt: GlobalDescriptorTable,
    selectors: Selectors,
}

#[derive(Clone, Copy)]
pub struct Selectors {
    pub code_selector: SegmentSelector,
    pub data_selector: SegmentSelector,
    pub tss_selector: SegmentSelector,
}

static mut CPU_GDTS: [Option<CpuGdtEntry>; MAX_CPUS] = [
    None, None, None, None, None, None, None, None,
];

pub fn init_cpu(core_id: usize) {
    if core_id >= MAX_CPUS {
        panic!("core_id {} exceeds MAX_CPUS {}", core_id, MAX_CPUS);
    }

    unsafe {
        let tss = TaskStateSegment::new();
        let mut entry = CpuGdtEntry {
            tss,
            stack: [0; STACK_SIZE],
            gdt: GlobalDescriptorTable::new(),
            selectors: Selectors {
                code_selector: SegmentSelector(0),
                data_selector: SegmentSelector(0),
                tss_selector: SegmentSelector(0),
            },
        };

        let stack_start = VirtAddr::from_ptr(entry.stack.as_ptr());
        let stack_end = stack_start + STACK_SIZE as u64;
        entry.tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_end;

        CPU_GDTS[core_id] = Some(entry);
        let cpu_entry = CPU_GDTS[core_id].as_mut().unwrap();

        let code_selector = cpu_entry.gdt.append(Descriptor::kernel_code_segment());
        let data_selector = cpu_entry.gdt.append(Descriptor::kernel_data_segment());
        let tss_selector = cpu_entry.gdt.append(Descriptor::tss_segment(&cpu_entry.tss));

        cpu_entry.selectors = Selectors {
            code_selector,
            data_selector,
            tss_selector,
        };

        cpu_entry.gdt.load();
        CS::set_reg(code_selector);
        DS::set_reg(data_selector);
        ES::set_reg(data_selector);
        SS::set_reg(data_selector);
        load_tss(tss_selector);
    }
}

pub fn init() {
    init_cpu(0);
}
