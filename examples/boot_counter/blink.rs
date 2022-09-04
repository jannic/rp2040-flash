use embedded_hal::{blocking::delay::DelayMs, digital::v2::OutputPin};

const DURATION_SHORT_MS: u32 = 200;
const DURATION_LONG_MS: u32 = 1000;

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
            self.delay.delay_ms(DURATION_SHORT_MS);
            self.pin.set_low()?;
            if i != n {
                self.delay.delay_ms(DURATION_SHORT_MS)
            }
        }
        Ok(())
    }

    pub fn pause(&mut self) {
        self.delay.delay_ms(DURATION_LONG_MS);
    }
}
