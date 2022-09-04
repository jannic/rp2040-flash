#![no_std]
#![no_main]

mod blink;
mod conf;
mod flash_sector;

use conf::Conf;
use flash_sector::{FlashSector, FLASH_ORIGIN};

use core::mem::size_of;
use defmt::{assert, *};
use defmt_rtt as _;
use panic_probe as _;

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

const FLASH_END: usize = FLASH_ORIGIN + 2 * 1024 * 1024;
/// Place the configuration data at the very end of the flash memory,
/// so that it doesn't get overwritten by normal firmware upgrades.
const FLASH_CONF_ADDR: usize = FLASH_END - size_of::<FlashSector<Conf>>();

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

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());
    // add some delay to give an attached debug probe time to parse the
    // defmt RTT header. Reading that header might touch flash memory, which
    // interferes with flash write operations.
    // https://github.com/knurling-rs/defmt/pull/683
    delay.delay_ms(10);

    let pins = bsp::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    let conf = unsafe {
        FlashSector::<Conf>::read(FLASH_CONF_ADDR)
            .value()
            .assume_init()
    }
    .valid();

    let psm = pac.PSM;

    // Reset core1 so it's guaranteed to be running
    // ROM code, waiting for the wakeup sequence
    psm.frce_off.modify(|_, w| w.proc1().set_bit());
    while !psm.frce_off.read().proc1().bit_is_set() {
        cortex_m::asm::nop();
    }
    psm.frce_off.modify(|_, w| w.proc1().clear_bit());

    info!("Addr of flash block is {:x}", FLASH_CONF_ADDR);

    let new_conf = Conf::new(conf.boot_counter() + 1);

    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    unsafe { FlashSector::new(new_conf).write(FLASH_CONF_ADDR) };
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    let updated_conf = {
        let conf = unsafe {
            FlashSector::<Conf>::read(FLASH_CONF_ADDR)
                .value()
                .assume_init()
        };

        assert!(
            conf.is_valid(),
            "a valid configuration was written to flash, but an invalid is read back"
        );

        conf
    };

    let led_pin = pins.led.into_push_pull_output();
    let mut blink = blink::Blink::new(led_pin, delay);

    loop {
        blink.times(updated_conf.boot_counter()).unwrap();
        blink.pause();
    }
}
