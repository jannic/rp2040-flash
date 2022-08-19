#![no_std]
#![no_main]

use core::mem::{size_of, MaybeUninit};
use defmt::{assert, *};
use defmt_rtt as _;
use embedded_time::fixed_point::FixedPoint;
use panic_probe as _;
use rp2040_flash::flash;

// Provide an alias for our BSP so we can switch targets quickly.
// Uncomment the BSP you included in Cargo.toml, the rest of the code does not need to change.
use rp_pico as bsp;
// use sparkfun_pro_micro_rp2040 as bsp;

use bsp::entry;
use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
    pac,
    watchdog::Watchdog,
    Sio,
};

const FLASH_ORIGIN: usize = 0x10000000;
/// The erasable sector size.
const FLASH_SECTOR_SIZE: usize = 4096;

/// The payload type `T` must fit into a single flash sector.
///
/// If the flash-stored data is expected to survive firmware upgrades,
/// `T` should be `repr(C)` to have a stable layout.
///
/// This data type itself should be flash sector aligned.
#[repr(C, align(4096))]
pub union FlashSector<T>
where
    T: Copy,
{
    data: [u8; FLASH_SECTOR_SIZE],
    value: MaybeUninit<T>,
}

impl<T> FlashSector<T>
where
    T: Copy,
{
    pub const fn uninit() -> Self {
        core::debug_assert!(
            size_of::<T>() <= FLASH_SECTOR_SIZE,
            "`T` must fit into a single sector size"
        );
        Self {
            value: MaybeUninit::uninit(),
        }
    }

    const fn new(value: T) -> Self {
        Self {
            value: MaybeUninit::new(value),
        }
    }

    fn mem_addr(&self) -> usize {
        unsafe { &self.data as *const _ as usize }
    }

    pub fn read(&self) -> MaybeUninit<T> {
        unsafe { self.value }
    }

    pub unsafe fn write(&mut self, value: T) {
        let tmp_flash_block = FlashSector::new(value);
        self.write_flash(&tmp_flash_block.data)
    }

    unsafe fn write_flash(&self, data: &[u8; FLASH_SECTOR_SIZE]) {
        let flash_addr = self.mem_addr() - FLASH_ORIGIN;
        assert!(Self::is_aligned(flash_addr, FLASH_SECTOR_SIZE));

        cortex_m::interrupt::free(|_cs| {
            flash::flash_range_erase_and_program(flash_addr as u32, data, true);
        });
    }

    const fn is_aligned(addr: usize, alignment: usize) -> bool {
        core::assert!(Self::is_pwr_of_two(alignment));
        addr & (alignment - 1) == 0
    }

    const fn is_pwr_of_two(n: usize) -> bool {
        n != 0 && (n & (n - 1) == 0)
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Conf {
    validity: u8,
    boot_counter: u32,
}

impl Conf {
    const fn initial() -> Self {
        Self {
            validity: Self::VALIDITY_MARKER,
            boot_counter: 0,
        }
    }

    fn new(counter: u32) -> Self {
        Self {
            validity: Self::VALIDITY_MARKER,
            boot_counter: counter,
        }
    }

    fn is_valid(&self) -> bool {
        self.validity == Self::VALIDITY_MARKER
    }

    const VALIDITY_MARKER: u8 = 0x55;
}

#[link_section = ".rodata"]
static mut CONF_FLASH_SECTOR: FlashSector<Conf> = FlashSector::uninit();

#[entry]
fn main() -> ! {
    info!("Program start");
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // External high-speed crystal on the Pico board is 12 Mhz
    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().integer());

    let pins = bsp::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // add some delay to give an attached debug probe time to parse the
    // defmt RTT header. Reading that header might touch flash memory, which
    // interferes with flash write operations.
    // https://github.com/knurling-rs/defmt/pull/683
    delay.delay_ms(10);

    let conf = {
        let conf = unsafe { CONF_FLASH_SECTOR.read().assume_init() };

        if conf.is_valid() {
            conf
        } else {
            Conf::initial()
        }
    };

    let psm = pac.PSM;

    // Reset core1 so it's guaranteed to be running
    // ROM code, waiting for the wakeup sequence
    psm.frce_off.modify(|_, w| w.proc1().set_bit());
    while !psm.frce_off.read().proc1().bit_is_set() {
        cortex_m::asm::nop();
    }
    psm.frce_off.modify(|_, w| w.proc1().clear_bit());

    info!("Addr of flash block is {:x}", unsafe {
        CONF_FLASH_SECTOR.mem_addr()
    });
    info!("Contents start with {=[u8]}", unsafe {
        &CONF_FLASH_SECTOR.data[0..4]
    });

    let new_conf = Conf::new(conf.boot_counter + 1);

    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    unsafe { CONF_FLASH_SECTOR.write(new_conf) };
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    let updated_conf = {
        let conf = unsafe { CONF_FLASH_SECTOR.read().assume_init() };
        assert!(
            conf.is_valid(),
            "a valid configuration was written to flash, but an invalid is read back"
        );
        conf
    };

    info!("Contents start with {=[u8]}", unsafe {
        &CONF_FLASH_SECTOR.data[0..4]
    });

    let led_pin = pins.led.into_push_pull_output();
    let mut blink = blink::Blink::new(led_pin, delay);

    loop {
        blink.times(updated_conf.boot_counter).unwrap();
        blink.pause();
    }
}

mod blink {
    use embedded_hal::{blocking::delay::DelayMs, digital::v2::OutputPin};

    pub struct Blink<Pin, Delay>
    where
        Pin: OutputPin,
        Delay: DelayMs<u32>,
    {
        pin: Pin,
        delay: Delay,
    }

    impl<Pin, Delay> Blink<Pin, Delay>
    where
        Pin: OutputPin,
        Delay: DelayMs<u32>,
    {
        pub fn new(pin: Pin, delay: Delay) -> Self {
            Self { pin, delay }
        }

        pub fn times(&mut self, n: u32) -> Result<(), Pin::Error> {
            for i in 1..=n {
                self.pin.set_high()?;
                self.delay.delay_ms(Self::DURATION_SHORT_MS);
                self.pin.set_low()?;
                if i != n {
                    self.delay.delay_ms(Self::DURATION_SHORT_MS)
                }
            }
            Ok(())
        }

        pub fn pause(&mut self) {
            self.delay.delay_ms(Self::DURATION_LONG_MS);
        }

        const DURATION_SHORT_MS: u32 = 200;
        const DURATION_LONG_MS: u32 = 1000;
    }
}
