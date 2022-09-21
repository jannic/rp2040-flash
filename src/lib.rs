#![no_std]

pub mod flash {
    use rp2040_hal::rom_data;

    pub enum DataOrLen<'a> {
        Data(&'a [u8]),
        Len(usize),
    }

    #[repr(C)]
    struct FlashFunctionPointers {
        connect_internal_flash: unsafe extern "C" fn() -> (),
        flash_exit_xip: unsafe extern "C" fn() -> (),
        flash_range_erase:
            unsafe extern "C" fn(addr: u32, count: usize, block_size: u32, block_cmd: u8) -> (),
        flash_range_program: unsafe extern "C" fn(addr: u32, data: *const u8, count: usize) -> (),
        flash_flush_cache: unsafe extern "C" fn() -> (),
        flash_enter_cmd_xip: unsafe extern "C" fn() -> (),
    }

    pub struct FlashIAP {
        fn_ptrs: FlashFunctionPointers,
        boot2: Option<[u32; 256 / 4]>,
    }

    impl FlashIAP {
        pub fn new(use_boot2: bool) -> Self {
            let mut me = Self {
                fn_ptrs: FlashFunctionPointers {
                    connect_internal_flash: rom_data::connect_internal_flash::ptr(),
                    flash_exit_xip: rom_data::flash_exit_xip::ptr(),
                    flash_range_erase: rom_data::flash_range_erase::ptr(),
                    flash_range_program: rom_data::flash_range_program::ptr(),
                    flash_flush_cache: rom_data::flash_flush_cache::ptr(),
                    flash_enter_cmd_xip: rom_data::flash_enter_cmd_xip::ptr(),
                },
                boot2: None,
            };
            if use_boot2 {
                me.boot2 = Some([0u32; 256 / 4]);
                unsafe {
                    rom_data::memcpy44(
                        me.boot2.as_mut().unwrap_unchecked() as *mut _,
                        0x10000000 as *const _,
                        256,
                    );
                }
            }
            me
        }

        /// Erase a flash range starting at `addr` with length `len`.
        ///
        /// `addr` and `len` must be multiples of 4096
        ///
        /// If `use_boot2` is `true`, a copy of the 2nd stage boot loader
        /// is used to re-initialize the XIP engine after flashing.
        ///
        /// # Safety
        ///
        /// Nothing must access flash while this is running.
        /// Usually this means:
        ///   - interrupts must be disabled
        ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
        ///   - DMA must not access flash memory
        ///
        /// `addr` and `len` parameters must be valid and are not checked.
        pub unsafe fn flash_range_erase(&mut self, addr: u32, len: usize) {
            self.write_flash_inner(addr, DataOrLen::Len(len as usize), true);
        }

        /// Erase and rewrite a flash range starting at `addr` with data `data`.
        ///
        /// `addr` and `data.len()` must be multiples of 4096
        ///
        /// If `use_boot2` is `true`, a copy of the 2nd stage boot loader
        /// is used to re-initialize the XIP engine after flashing.
        ///
        /// # Safety
        ///
        /// Nothing must access flash while this is running.
        /// Usually this means:
        ///   - interrupts must be disabled
        ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
        ///   - DMA must not access flash memory
        ///
        /// `addr` and `len` parameters must be valid and are not checked.
        pub unsafe fn flash_range_erase_and_program(&mut self, addr: u32, data: &[u8]) {
            self.write_flash_inner(addr, DataOrLen::Data(data), true);
        }

        /// Write a flash range starting at `addr` with data `data`.
        ///
        /// `addr` and `data.len()` must be multiples of 256
        ///
        /// If `use_boot2` is `true`, a copy of the 2nd stage boot loader
        /// is used to re-initialize the XIP engine after flashing.
        ///
        /// # Safety
        ///
        /// Nothing must access flash while this is running.
        /// Usually this means:
        ///   - interrupts must be disabled
        ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
        ///   - DMA must not access flash memory
        ///
        /// `addr` and `len` parameters must be valid and are not checked.
        pub unsafe fn flash_range_program(&mut self, addr: u32, data: &[u8]) {
            self.write_flash_inner(addr, DataOrLen::Data(data), false);
        }

        unsafe fn write_flash_inner(&mut self, addr: u32, data: DataOrLen, erase: bool) {
            let (data_ptr, len) = match data {
                DataOrLen::Data(data) => (data.as_ptr(), data.len()),
                DataOrLen::Len(len) => (core::ptr::null(), len),
            };

            // this is done on every call in case Self was moved in the mean time.
            if let Some(boot2) = self.boot2.as_ref() {
                let boot2_fn_ptr = (boot2 as *const u32 as *const u8).offset(1);
                let boot2_fn: unsafe extern "C" fn() -> () = core::mem::transmute(boot2_fn_ptr);
                self.fn_ptrs.flash_enter_cmd_xip = boot2_fn;
            }
            self.write_flash_inner_in_ram(addr, data_ptr, len, erase)
        }

        #[inline(never)]
        #[link_section = ".data.ram_func"]
        unsafe fn write_flash_inner_in_ram(
            &mut self,
            addr: u32,
            data_ptr: *const u8,
            len: usize,
            erase: bool,
        ) {
            #[cfg(not(feature = "use-asm"))]
            {
                (self.fn_ptrs.connect_internal_flash)();
                (self.fn_ptrs.flash_exit_xip)();
                if erase {
                    (self.fn_ptrs.flash_range_erase)(addr, len, 1 << 31, 0);
                }
                if !data_ptr.is_null() {
                    (self.fn_ptrs.flash_range_program)(addr, data_ptr, len);
                }
                (self.fn_ptrs.flash_flush_cache)();
                (self.fn_ptrs.flash_enter_cmd_xip)();
            }
            #[cfg(feature = "use-asm")]
            {
                let stack = [addr, data_ptr as u32, len as u32];
                core::arch::asm!(
                    "ldr r0, [{ptrs}, #0]",
                    "blx r0", // connect_internal_flash()

                    "ldr r0, [{ptrs}, #4]",
                    "blx r0", // flash_exit_xip()

                    "cmp r4, #0", // erase
                    "beq 1f",
                    "ldr r0, [{stack}, #0]", // addr
                    "ldr r1, [{stack}, #8]", // len
                    "movs r2, #1",
                    "lsls r2, r2, #31", // r2 = 1 << 31
                    "movs r3, #0", // r3 = 0
                    "ldr r4, [{ptrs}, #8]",
                    "blx r4", // flash_range_erase(addr, len, 1 << 31, 0)
                    "1:",

                    "ldr r1, [{stack}, #4]", // data_ptr
                    "cmp r1, #0",
                    "beq 1f",
                    "ldr r0, [{stack}, #0]", // addr
                    "ldr r2, [{stack}, #8]", // len
                    "ldr r4, [{ptrs}, #12]",
                    "blx r4", // flash_range_program(addr, data, len);
                    "1:",

                    "ldr r4, [{ptrs}, #16]",
                    "blx r4", // flash_flush_cache();

                    "ldr r4, [{ptrs}, #20]",
                    "blx r4", // flash_enter_cmd_xip();
                    ptrs = in(reg) &self.fn_ptrs as *const FlashFunctionPointers,
                    stack = in(reg) &stack as *const u32,
                    out("r0") _,
                    out("r1") _,
                    out("r2") _,
                    out("r3") _,
                    inout("r4") erase as u32 => _,
                    clobber_abi("C"),
                );
            }
        }
    }
}
