use alloc::vec::Vec;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_id: u8,
    pub subclass_id: u8,
    pub prog_if: u8,
    pub header_type: u8,
}

impl PciDevice {
    pub fn read_bar(&self, index: u8) -> u32 {
        unsafe { config_read_u32(self.bus, self.device, self.function, 0x10 + (index * 4)) }
    }
}

pub unsafe fn config_read_u32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address = (1 << 31)
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);

    let mut port_addr = Port::<u32>::new(CONFIG_ADDRESS);
    let mut port_data = Port::<u32>::new(CONFIG_DATA);

    port_addr.write(address);
    port_data.read()
}

pub unsafe fn config_read_u16(bus: u8, device: u8, func: u8, offset: u8) -> u16 {
    let dword = config_read_u32(bus, device, func, offset);
    if (offset & 2) != 0 {
        (dword >> 16) as u16
    } else {
        (dword & 0xFFFF) as u16
    }
}

pub unsafe fn config_read_u8(bus: u8, device: u8, func: u8, offset: u8) -> u8 {
    let dword = config_read_u32(bus, device, func, offset);
    let shift = (offset & 3) * 8;
    ((dword >> shift) & 0xFF) as u8
}

pub unsafe fn config_write_u32(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
    let address = (1 << 31)
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);

    let mut port_addr = Port::<u32>::new(CONFIG_ADDRESS);
    let mut port_data = Port::<u32>::new(CONFIG_DATA);

    port_addr.write(address);
    port_data.write(value);
}

pub unsafe fn config_write_u16(bus: u8, device: u8, func: u8, offset: u8, value: u16) {
    let mut current = config_read_u32(bus, device, func, offset);
    let shift = (offset & 2) * 8;
    let mask = 0xFFFF << shift;
    current &= !mask;
    current |= (value as u32) << shift;
    config_write_u32(bus, device, func, offset, current);
}

pub unsafe fn config_write_u8(bus: u8, device: u8, func: u8, offset: u8, value: u8) {
    let mut current = config_read_u32(bus, device, func, offset);
    let shift = (offset & 3) * 8;
    let mask = 0xFF << shift;
    current &= !mask;
    current |= (value as u32) << shift;
    config_write_u32(bus, device, func, offset, current);
}

pub fn scan_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    // Bruteforce scan bus 0 (expand to 255 if needed, but QEMU puts everything on bus 0 mostly)
    for bus in 0..=0 {
        for device in 0..32 {
            if let Some(dev) = check_function(bus, device, 0) {
                devices.push(dev);

                // Check for multi-function device
                if (dev.header_type & 0x80) != 0 {
                    for func in 1..8 {
                        if let Some(fdev) = check_function(bus, device, func) {
                            devices.push(fdev);
                        }
                    }
                }
            }
        }
    }
    devices
}

fn check_function(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let vendor_id = unsafe { config_read_u16(bus, device, function, 0) };
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = unsafe { config_read_u16(bus, device, function, 2) };
    let class_id = unsafe { config_read_u8(bus, device, function, 0x0B) };
    let subclass_id = unsafe { config_read_u8(bus, device, function, 0x0A) };
    let prog_if = unsafe { config_read_u8(bus, device, function, 0x09) };
    let header_type = unsafe { config_read_u8(bus, device, function, 0x0E) };

    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id,
        class_id,
        subclass_id,
        prog_if,
        header_type,
    })
}
