use core::{
    mem::{size_of, MaybeUninit},
    ptr::read_volatile,
};

use rp2040_flash::flash;

/// XIP base address (see `XIP_BASE` in RP2040 datasheet).
pub const FLASH_ORIGIN: usize = 0x10000000;
/// RP2040 supports maximum 16 MiB of QSPI flash memory.
pub const FLASH_END_MAX: usize = FLASH_ORIGIN + 16 * 1024 * 1024;
/// The erasable sector size.
const FLASH_SECTOR_SIZE: usize = 4096;
/// The value an erased sector is filled with. This is typically 0xff.
const FLASH_ERASED_VALUE: u8 = 0xff;

/// The payload type `T` must fit into a single flash sector.
///
/// The payload type should be `repr(C)` to have a stable layout,
/// because the flash-stored payload can survive firmware upgrades.
pub union FlashSector<T>
where
    T: Copy,
{
    data: [u8; FLASH_SECTOR_SIZE],
    value: MaybeUninit<T>,
}

impl<T> Default for FlashSector<T>
where
    T: Copy,
{
    fn default() -> Self {
        Self {
            data: [FLASH_ERASED_VALUE; FLASH_SECTOR_SIZE],
        }
    }
}

impl<T> FlashSector<T>
where
    T: Copy,
{
    pub fn new(value: T) -> Self {
        assert!(
            size_of::<T>() <= FLASH_SECTOR_SIZE,
            "`T` must fit into a single sector size"
        );

        let mut instance = Self::default();
        instance.value = MaybeUninit::new(value);
        instance
    }

    pub unsafe fn read(mem_addr: usize) -> Self {
        assert!(
            size_of::<T>() <= FLASH_SECTOR_SIZE,
            "`T` must fit into a single sector size"
        );
        assert!(mem_addr >= FLASH_ORIGIN);
        assert!(mem_addr <= FLASH_END_MAX - FLASH_SECTOR_SIZE);
        // The read address must be sector-aligned, because the write function
        // only ever allows writing at sector-aligned addresses.
        assert!(is_aligned(mem_addr, FLASH_SECTOR_SIZE));

        let mut flash_sector = FlashSector::default();
        flash_sector.value = unsafe { read_volatile(mem_addr as *const _) };
        flash_sector
    }

    pub unsafe fn write(&self, mem_addr: usize) {
        assert!(mem_addr >= FLASH_ORIGIN);
        assert!(mem_addr <= FLASH_END_MAX - FLASH_SECTOR_SIZE);

        let flash_addr = mem_addr - FLASH_ORIGIN;
        assert!(is_aligned(flash_addr, FLASH_SECTOR_SIZE));

        flash::flash_range_erase_and_program(flash_addr as u32, &self.data, true);
    }

    pub fn value(&self) -> MaybeUninit<T> {
        unsafe { self.value }
    }
}

const fn is_aligned(addr: usize, alignment: usize) -> bool {
    assert!(alignment.is_power_of_two());
    addr & (alignment - 1) == 0
}
