
// See W5100 datasheet section 6.3.2; each read/write is a 32-bit stream
// containing a 1-byte opcode, 2-byte address, and 1-byte data.
#[repr(transparent)]
pub(super) struct SpiStream([u8; 4]);

impl SpiStream {
    const OP_WRITE: u8 = 0xf0;
    const OP_READ: u8 = 0x0f;

    pub(super) fn write(addr: u16, data: u8) -> Self {
        Self([Self::OP_WRITE, (addr >> 8) as u8, addr as u8, data])
    }

    pub(super) fn read(addr: u16) -> Self {
        Self([Self::OP_READ, (addr >> 8) as u8, addr as u8, 0])
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(super) fn set_data(&mut self, val: u8) {
        self.0[3] = val;
    }

    // panics if our address is currently 0xffff
    pub(super) fn increment_addr(&mut self) {
        self.0[2] = self.0[2].wrapping_add(1);
        if self.0[2] == 0 {
            self.0[1] += 1; // TODO confirm this panics on overflow?
        }
    }
}
