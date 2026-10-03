use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use crate::memory::PHYS_MEM_OFFSET;
use crate::serial_println;

#[derive(Debug, Clone)]
pub struct CpuInfo {
    pub processor_id: u8,
    pub apic_id: u8,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AcpiInfo {
    pub lapic_addr: u64,
    pub cpus: Vec<CpuInfo>,
    pub ioapic_addr: Option<u32>,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct RsdpDescriptor {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct RsdpDescriptorV2 {
    first_part: RsdpDescriptor,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct AcpiHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

fn phys_to_virt<T>(phys: u64) -> *const T {
    let offset = PHYS_MEM_OFFSET.load(Ordering::Relaxed);
    (offset + phys) as *const T
}

unsafe fn read_u32_unaligned(ptr: *const u8) -> u32 {
    let p = ptr as *const [u8; 4];
    u32::from_le_bytes(core::ptr::read_unaligned(p))
}

unsafe fn read_u64_unaligned(ptr: *const u8) -> u64 {
    let p = ptr as *const [u8; 8];
    u64::from_le_bytes(core::ptr::read_unaligned(p))
}

pub fn parse_acpi(rsdp_phys: u64) -> Option<AcpiInfo> {
    let rsdp_ptr = phys_to_virt::<RsdpDescriptor>(rsdp_phys);
    let rsdp = unsafe { core::ptr::read_unaligned(rsdp_ptr) };

    if &rsdp.signature != b"RSD PTR " {
        serial_println!("[ACPI] Invalid RSDP signature");
        return None;
    }

    let revision = rsdp.revision;
    serial_println!("[ACPI] RSDP revision: {}", revision);

    let mut madt_phys: Option<u64> = None;

    if revision >= 2 {
        let rsdp2_ptr = phys_to_virt::<RsdpDescriptorV2>(rsdp_phys);
        let rsdp2 = unsafe { core::ptr::read_unaligned(rsdp2_ptr) };
        let xsdt_phys = rsdp2.xsdt_address;
        serial_println!("[ACPI] XSDT physical address: 0x{:X}", xsdt_phys);

        let xsdt_header_ptr = phys_to_virt::<AcpiHeader>(xsdt_phys);
        let xsdt_header = unsafe { core::ptr::read_unaligned(xsdt_header_ptr) };
        let total_len = xsdt_header.length as usize;
        let header_len = core::mem::size_of::<AcpiHeader>();
        let num_entries = (total_len.saturating_sub(header_len)) / 8;

        for i in 0..num_entries {
            let entry_offset = header_len + i * 8;
            let entry_ptr = unsafe { (xsdt_header_ptr as *const u8).add(entry_offset) };
            let entry_phys = unsafe { read_u64_unaligned(entry_ptr) };
            let entry_hdr = unsafe { core::ptr::read_unaligned(phys_to_virt::<AcpiHeader>(entry_phys)) };
            if &entry_hdr.signature == b"APIC" {
                madt_phys = Some(entry_phys);
                break;
            }
        }
    }

    // Fallback to RSDT if not found in XSDT or revision == 0
    if madt_phys.is_none() && rsdp.rsdt_address != 0 {
        let rsdt_phys = rsdp.rsdt_address as u64;
        serial_println!("[ACPI] RSDT physical address: 0x{:X}", rsdt_phys);

        let rsdt_header_ptr = phys_to_virt::<AcpiHeader>(rsdt_phys);
        let rsdt_header = unsafe { core::ptr::read_unaligned(rsdt_header_ptr) };
        let total_len = rsdt_header.length as usize;
        let header_len = core::mem::size_of::<AcpiHeader>();
        let num_entries = (total_len.saturating_sub(header_len)) / 4;

        for i in 0..num_entries {
            let entry_offset = header_len + i * 4;
            let entry_ptr = unsafe { (rsdt_header_ptr as *const u8).add(entry_offset) };
            let entry_phys = unsafe { read_u32_unaligned(entry_ptr) } as u64;
            let entry_hdr = unsafe { core::ptr::read_unaligned(phys_to_virt::<AcpiHeader>(entry_phys)) };
            if &entry_hdr.signature == b"APIC" {
                madt_phys = Some(entry_phys);
                break;
            }
        }
    }

    let madt_addr = match madt_phys {
        Some(addr) => addr,
        None => {
            serial_println!("[ACPI] MADT (APIC) table not found");
            return None;
        }
    };

    serial_println!("[ACPI] Found MADT at physical: 0x{:X}", madt_addr);
    parse_madt(madt_addr)
}

fn parse_madt(madt_phys: u64) -> Option<AcpiInfo> {
    let header_ptr = phys_to_virt::<AcpiHeader>(madt_phys);
    let header = unsafe { core::ptr::read_unaligned(header_ptr) };
    let total_len = header.length as usize;

    // MADT header has lapic_address (u32) at offset 36, flags (u32) at offset 40
    let lapic_addr_ptr = unsafe { (header_ptr as *const u8).add(36) };
    let mut default_lapic = unsafe { read_u32_unaligned(lapic_addr_ptr) } as u64;

    let mut cpus = Vec::new();
    let mut ioapic_addr = None;

    let mut offset = 44usize;
    while offset < total_len {
        let entry_ptr = unsafe { (header_ptr as *const u8).add(offset) };
        let entry_type = unsafe { *entry_ptr };
        let entry_len = unsafe { *entry_ptr.add(1) } as usize;

        if entry_len == 0 {
            break;
        }

        match entry_type {
            0 => {
                // Processor Local APIC
                let processor_id = unsafe { *entry_ptr.add(2) };
                let apic_id = unsafe { *entry_ptr.add(3) };
                let flags = unsafe { read_u32_unaligned(entry_ptr.add(4)) };
                let enabled = (flags & 1) != 0 || (flags & 2) != 0;

                serial_println!(
                    "[ACPI] CPU core found: proc_id={}, apic_id={}, enabled={}",
                    processor_id, apic_id, enabled
                );

                cpus.push(CpuInfo {
                    processor_id,
                    apic_id,
                    enabled,
                });
            }
            1 => {
                // I/O APIC
                let io_id = unsafe { *entry_ptr.add(2) };
                let addr = unsafe { read_u32_unaligned(entry_ptr.add(4)) };
                serial_println!("[ACPI] I/O APIC found: id={}, addr=0x{:X}", io_id, addr);
                ioapic_addr = Some(addr);
            }
            5 => {
                // 64-bit Local APIC Address Override
                let addr_override = unsafe { read_u64_unaligned(entry_ptr.add(4)) };
                serial_println!("[ACPI] 64-bit LAPIC address override: 0x{:X}", addr_override);
                default_lapic = addr_override;
            }
            _ => {}
        }

        offset += entry_len;
    }

    Some(AcpiInfo {
        lapic_addr: default_lapic,
        cpus,
        ioapic_addr,
    })
}
