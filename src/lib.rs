#![no_std]

pub mod flash {
    use core::marker::PhantomData;
    use rp2040_hal::rom_data;

    #[repr(C)]
    struct FlashFunctionPointers<'a> {
        connect_internal_flash: unsafe extern "C" fn() -> (),
        flash_exit_xip: unsafe extern "C" fn() -> (),
        flash_range_erase: Option<
            unsafe extern "C" fn(addr: u32, count: usize, block_size: u32, block_cmd: u8) -> (),
        >,
        flash_range_program:
            Option<unsafe extern "C" fn(addr: u32, data: *const u8, count: usize) -> ()>,
        flash_flush_cache: unsafe extern "C" fn() -> (),
        flash_enter_cmd_xip: unsafe extern "C" fn() -> (),
        phantom: PhantomData<&'a ()>,
    }

    #[allow(unused)]
    fn flash_function_pointers(erase: bool, write: bool) -> FlashFunctionPointers<'static> {
        FlashFunctionPointers {
            connect_internal_flash: rom_data::connect_internal_flash::ptr(),
            flash_exit_xip: rom_data::flash_exit_xip::ptr(),
            flash_range_erase: if erase {
                Some(rom_data::flash_range_erase::ptr())
            } else {
                None
            },
            flash_range_program: if write {
                Some(rom_data::flash_range_program::ptr())
            } else {
                None
            },
            flash_flush_cache: rom_data::flash_flush_cache::ptr(),
            flash_enter_cmd_xip: rom_data::flash_enter_cmd_xip::ptr(),
            phantom: PhantomData,
        }
    }

    #[allow(unused)]
    /// # Safety
    ///
    /// `boot2` must contain a valid 2nd stage boot loader which can be called to re-initialize XIP mode
    unsafe fn flash_function_pointers_with_boot2(
        erase: bool,
        write: bool,
        boot2: &[u32; 64],
    ) -> FlashFunctionPointers {
        let boot2_fn_ptr = (boot2 as *const u32 as *const u8).offset(1);
        let boot2_fn: unsafe extern "C" fn() -> () = core::mem::transmute(boot2_fn_ptr);
        FlashFunctionPointers {
            connect_internal_flash: rom_data::connect_internal_flash::ptr(),
            flash_exit_xip: rom_data::flash_exit_xip::ptr(),
            flash_range_erase: if erase {
                Some(rom_data::flash_range_erase::ptr())
            } else {
                None
            },
            flash_range_program: if write {
                Some(rom_data::flash_range_program::ptr())
            } else {
                None
            },
            flash_flush_cache: rom_data::flash_flush_cache::ptr(),
            flash_enter_cmd_xip: boot2_fn,
            phantom: PhantomData,
        }
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
    pub unsafe fn flash_range_erase(addr: u32, len: u32, use_boot2: bool) {
        let mut boot2 = [0u32; 256 / 4];
        let ptrs = if use_boot2 {
            rom_data::memcpy44(&mut boot2 as *mut _, 0x10000000 as *const _, 256);
            flash_function_pointers_with_boot2(true, false, &boot2)
        } else {
            flash_function_pointers(true, false)
        };
        write_flash_inner(addr, len, None, &ptrs as *const FlashFunctionPointers);
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
    pub unsafe fn flash_range_erase_and_program(addr: u32, data: &[u8], use_boot2: bool) {
        let mut boot2 = [0u32; 256 / 4];
        let ptrs = if use_boot2 {
            rom_data::memcpy44(&mut boot2 as *mut _, 0x10000000 as *const _, 256);
            flash_function_pointers_with_boot2(true, true, &boot2)
        } else {
            flash_function_pointers(true, true)
        };
        write_flash_inner(
            addr,
            data.len() as u32,
            Some(data),
            &ptrs as *const FlashFunctionPointers,
        );
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
    pub unsafe fn flash_range_program(addr: u32, data: &[u8], use_boot2: bool) {
        let mut boot2 = [0u32; 256 / 4];
        let ptrs = if use_boot2 {
            rom_data::memcpy44(&mut boot2 as *mut _, 0x10000000 as *const _, 256);
            flash_function_pointers_with_boot2(false, true, &boot2)
        } else {
            flash_function_pointers(false, true)
        };
        write_flash_inner(
            addr,
            data.len() as u32,
            Some(data),
            &ptrs as *const FlashFunctionPointers,
        );
    }

    /// # Safety
    ///
    /// Nothing must access flash while this is running.
    /// Usually this means:
    ///   - interrupts must be disabled
    ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
    ///   - DMA must not access flash memory
    /// Length of data must be a multiple of 4096
    /// addr must be aligned to 4096
    #[inline(never)]
    #[link_section = ".data.ram_func"]
    unsafe fn write_flash_inner(
        addr: u32,
        len: u32,
        data: Option<&[u8]>,
        ptrs: *const FlashFunctionPointers,
    ) {
        /*
         Should be equivalent to:
            rom_data::connect_internal_flash();
            rom_data::flash_exit_xip();
            rom_data::flash_range_erase(addr, len, 1 << 31, 0); // if selected
            rom_data::flash_range_program(addr, data as *const _, len); // if selected
            rom_data::flash_flush_cache();
            rom_data::flash_enter_cmd_xip();
        */
        core::arch::asm!(
            "mov r8, r0",
            "mov r9, r2",
            "mov r10, r1",
            "ldr r4, [{ptrs}, #0]",
            "blx r4", // connect_internal_flash()

            "ldr r4, [{ptrs}, #4]",
            "blx r4", // flash_exit_xip()

            "mov r0, r8", // r0 = addr
            "mov r1, r10", // r1 = len
            "movs r2, #1",
            "lsls r2, r2, #31", // r2 = 1 << 31
            "movs r3, #0", // r3 = 0
            "ldr r4, [{ptrs}, #8]",
            "cmp r4, #0",
            "beq 1f",
            "blx r4", // flash_range_erase(addr, len, 1 << 31, 0)
            "1:",

            "mov r0, r8", // r0 = addr
            "mov r1, r9", // r0 = data
            "mov r2, r10", // r2 = len
            "ldr r4, [{ptrs}, #12]",
            "cmp r4, #0",
            "beq 1f",
            "blx r4", // flash_range_program(addr, data, len);
            "1:",

            "ldr r4, [{ptrs}, #16]",
            "blx r4", // flash_flush_cache();

            "ldr r4, [{ptrs}, #20]",
            "blx r4", // flash_enter_cmd_xip();
            ptrs = in(reg) ptrs,
            in("r0") addr,
            in("r2") data.map(|d| d.as_ptr()).unwrap_or(core::ptr::null()),
            in("r1") len,
            out("r3") _,
            out("r4") _,
            // Registers r8-r10 are used to store values
            // from r0-r2 in registers not clobbered by
            // function calls.
            // The values can't be passed in using r8-r10 directly
            // due to https://github.com/rust-lang/rust/issues/99071
            out("r8") _,
            out("r9") _,
            out("r10") _,
            clobber_abi("C"),
        );
    }

    /// Return SPI flash unique ID
    ///
    /// Most SPI flash chips accept this command (the Winbond parts
    /// commonly seen on RP2040 devboards certainly do).
    ///
    /// The returned bytes are relatively predictable and should be
    /// salted and hashed before use if that is an issue (e.g. for MAC
    /// addresses).
    ///
    /// # Safety
    ///
    /// Nothing must access flash while this is running.
    /// Usually this means:
    ///   - interrupts must be disabled
    ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
    ///   - DMA must not access flash memory
    pub unsafe fn flash_unique_id(use_boot2: bool) -> u64 {
        let mut boot2 = [0u32; 256 / 4];
        let ptrs = if use_boot2 {
            rom_data::memcpy44(&mut boot2 as *mut _, 0x10000000 as *const _, 256);
            flash_function_pointers_with_boot2(true, true, &boot2)
        } else {
            flash_function_pointers(false, false)
        };
        let mut id = [0u8; 8];
        let p = id.as_mut_ptr();

        // 4B - read unique ID, +0x400 -> 4 dummy bytes
        read_flash_inner(0x44B, 8, p, &ptrs as *const FlashFunctionPointers);
        u64::from_be_bytes(id)
    }

    /// Return SPI flash JEDEC ID
    ///
    /// This is the three-byte manufacturer-and-model identifier
    /// commonly used to check before using manufacturer-specific SPI
    /// flash features, e.g. 0xEF7015 for Winbond W25Q16JV.
    ///
    /// # Safety
    ///
    /// Nothing must access flash while this is running.
    /// Usually this means:
    ///   - interrupts must be disabled
    ///   - 2nd core must be running code from RAM or ROM with interrupts disabled
    ///   - DMA must not access flash memory
    pub unsafe fn flash_jedec_id(use_boot2: bool) -> u32 {
        let mut boot2 = [0u32; 256 / 4];
        let ptrs = if use_boot2 {
            rom_data::memcpy44(&mut boot2 as *mut _, 0x10000000 as *const _, 256);
            flash_function_pointers_with_boot2(false, false, &boot2)
        } else {
            flash_function_pointers(false, false)
        };
        let mut id = [0u8; 4];
        let p = id.as_mut_ptr().add(1);

        // 9F - read JEDEC ID
        read_flash_inner(0x9F, 3, p, &ptrs as *const FlashFunctionPointers);
        u32::from_be_bytes(id)
    }

    /// Issue a generic SPI flash read command
    ///
    /// # Arguments
    ///
    /// * `cmd_skip` - SPI flash command (bits 0-7) plus dummy-byte count (bits 15-8)
    /// * `len` - Transfer length *excluding* any dummy bytes
    /// * `data` - Result buffer, must be at least `len` bytes long
    /// * `ptrs` - Flash function pointers as per
    #[inline(never)]
    #[link_section = ".data.ram_func"]
    unsafe fn read_flash_inner(
        cmd_skip: u16,
        len: u32,
        data: *mut u8,
        ptrs: *const FlashFunctionPointers,
    ) {
        core::arch::asm!(
            "mov r8, r0", // cmd+skip
            "mov r9, r1", // len
            "mov r10, r2", // data
            "mov r6, r3", // ptrs

            "ldr r4, [r6, #0]",
            "blx r4", // connect_internal_flash()

            "ldr r4, [r6, #4]",
            "blx r4", // flash_exit_xip()

            "movs r4, #0x18",
            "lsls r4, r4, #24", // 0x18000000, SSI, RP2040 datasheet 4.10.13

            // disable, write 0 to SSIENR
            "movs r0, #0",
            "str r0, [r4, #8]", // SSIENR

            // write ctrlr0
            "movs r0, #0x3",
            "lsls r0, r0, #8", // TMOD=0x300
            "ldr r1, [r4, #0]", // CTRLR0
            "orrs r1, r0",
            "str r1, [r4, #0]",

            // rx, so write ctrlr1 with len-1
            "mov r0, r9",
            "subs r0, #1",
            "mov r1, r8", // cmd+skip
            "asrs r1, #8", // skip
            "add r0, r0, r1", // total xfer
            "str r0, [r4, #0x04]", // CTRLR1

            // enable, write 1 to ssienr
            "movs r0, #1",
            "str r0, [r4, #8]", // SSIENR

            // write cmd to dr
            "mov r2, r4",
            "adds r2, 0x60", // &DR
            "mov r0, r8",
            "str r0, [r2]", // DR

            // Skip any dummy cycles
            "mov r1, r8", // cmd+skip
            "asrs r1, #8", // skip
            "beq 9f",
            "4:",
            "ldr r0, [r4, #0x28]", // SR
            "movs r2, #0x8",
            "tst r0, r2",
            "beq 4b",
            "mov r2, r4",
            "adds r2, 0x60", // &DR
            "ldrb r0, [r2]", // DR
            "subs r1, #1",
            "bne 4b",

            // Read RX fifo
            "9:",
            "mov r1, r9", // len
            "mov r5, r10", // data

            "2:",
            "ldr r0, [r4, #0x28]", // SR
            "movs r2, #0x8",
            "tst r0, r2", // SR.RFNE
            "beq 2b",

            "mov r2, r4",
            "adds r2, 0x60", // &DR
            "ldr r0, [r2]", // DR
            "strb r0, [r5]",
            "adds r5, #1",
            "subs r1, #1",
            "bne 2b",

            // Disable, write 0 to ssienr
            "movs r0, #0",
            "str r0, [r4, #8]", // SSIENR

            // Write 0 to CTRLR1 (returning to its default value)
            //
            // flash_enter_cmd_xip does NOT do this, and everything goes
            // wrong unless we do it here
            "str r0, [r4, #4]", // CTRLR1

            "ldr r4, [r6, #20]",
            "blx r4", // flash_enter_cmd_xip();

            in("r0") cmd_skip,
            in("r1") len,
            in("r2") data,
            in("r3") ptrs,
            out("r4") _,
            // Registers r8-r10 are used to store values
            // from r0-r2 in registers not clobbered by
            // function calls.
            // The values can't be passed in using r8-r10 directly
            // due to https://github.com/rust-lang/rust/issues/99071
            out("r8") _,
            out("r9") _,
            out("r10") _,
            clobber_abi("C"),
        );
    }
}
