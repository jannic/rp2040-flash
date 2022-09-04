const VALIDITY_MARKER: u8 = 0x55;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Conf {
    validity: u8,
    boot_counter: u32,
}

impl Default for Conf {
    fn default() -> Self {
        Self {
            validity: VALIDITY_MARKER,
            boot_counter: 0,
        }
    }
}

impl Conf {
    pub fn new(counter: u32) -> Self {
        Self {
            validity: VALIDITY_MARKER,
            boot_counter: counter,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.validity == VALIDITY_MARKER
    }

    pub fn valid(self) -> Self {
        if self.is_valid() {
            self
        } else {
            Default::default()
        }
    }

    pub fn boot_counter(&self) -> u32 {
        self.boot_counter
    }
}
