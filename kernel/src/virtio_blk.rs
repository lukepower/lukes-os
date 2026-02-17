use alloc::boxed::Box;
use alloc::sync::Arc;
use core::ptr::NonNull;
use spin::Mutex;
use virtio_drivers::{
    device::blk::VirtIOBlk as VirtIOBlkDriver,
    transport::pci::{bus::{ConfigurationAccess, DeviceFunction, PciRoot}, PciTransport},
};

use crate::block::{BlockDevice, BlockError, Result};
use crate::pci;
use crate::virtio_hal::VirtioHal;
use crate::serial_println;

// Global instance of the VirtIO block device
pub static RAW_VIRTIO_BLK: Mutex<Option<VirtIOBlkDriver<VirtioHal, PciTransport>>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct PciConfig;

impl ConfigurationAccess for PciConfig {
    unsafe fn unsafe_clone(&self) -> Self {
        *self
    }

    unsafe fn read_word(&self, dev: DeviceFunction, offset: u8) -> u32 {
        pci::config_read_u32(dev.bus, dev.device, dev.function, offset)
    }

    unsafe fn write_word(&mut self, dev: DeviceFunction, offset: u8, value: u32) {
        pci::config_write_u32(dev.bus, dev.device, dev.function, offset, value)
    }
}

pub fn init() {
    let mut devices = pci::scan_bus();
    let mut virtio_dev = None;

    for dev in devices {
        if dev.vendor_id == 0x1AF4 {
            // Found a VirtIO device. We need to check if it's a block device.
            // On PCI, the device ID identifies the type (Legacy: 0x1000+ID, Modern: 0x1040+ID)
            // But virtio-drivers PciTransport checks this.
            // We just need to pass it to PciTransport.
            serial_println!("Attempting to initialize VirtIO device at {:02x}:{:02x}.{:02x}", 
                dev.bus, dev.device, dev.function);
            
            // Create the PciRoot and leak it to get 'static lifetime
            let root = Box::leak(Box::new(PciRoot::new(PciConfig)));
            let device_function = DeviceFunction {
                bus: dev.bus,
                device: dev.device,
                function: dev.function,
            };

            match PciTransport::new::<VirtioHal, PciConfig>(root, device_function) {
                Ok(transport) => {
                     // check device type, 2 = block
                     // But PciTransport doesn't expose device type directly? 
                     // The driver check will fail if mismatch.
                     match VirtIOBlkDriver::<VirtioHal, PciTransport>::new(transport) {
                         Ok(driver) => {
                             serial_println!("[OK] VirtIO-Blk driver initialized!");
                             *RAW_VIRTIO_BLK.lock() = Some(driver);
                             virtio_dev = Some(dev);
                             break;
                         }
                         Err(e) => {
                             serial_println!("[SKIP] Not a block device or init failed: {:?}", e);
                         }
                     }
                }
                Err(e) => {
                    serial_println!("[ERR] Failed to create PciTransport: {:?}", e);
                }
            }
        }
    }
}

pub struct VirtIoBlockDevice;

impl BlockDevice for VirtIoBlockDevice {
    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<()> {
        let mut guard = RAW_VIRTIO_BLK.lock();
        if let Some(driver) = guard.as_mut() {
            driver.read_blocks(lba as usize, buf).map_err(|_| BlockError::ReadError)
        } else {
            Err(BlockError::IoError)
        }
    }

    fn write_block(&self, lba: u64, buf: &[u8]) -> Result<()> {
        let mut guard = RAW_VIRTIO_BLK.lock();
        if let Some(driver) = guard.as_mut() {
            driver.write_blocks(lba as usize, buf).map_err(|_| BlockError::WriteError)
        } else {
            Err(BlockError::IoError)
        }
    }

    fn block_size(&self) -> usize {
        512 // VirtIO standard
    }

    fn block_count(&self) -> u64 {
        let mut guard = RAW_VIRTIO_BLK.lock();
        if let Some(driver) = guard.as_mut() {
            driver.capacity()
        } else {
            0
        }
    }
}
