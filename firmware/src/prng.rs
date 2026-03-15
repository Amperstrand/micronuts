//! Simple PRNG for demo purposes (NOT cryptographically secure)

pub struct Prng {
    state: u64,
}

impl Prng {
    pub fn new(seed: u32) -> Self {
        Self { state: seed as u64 }
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    pub fn fill_bytes(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let val = self.next_u64();
            for (i, byte) in chunk.iter_mut().enumerate() {
                *byte = (val >> (i * 8)) as u8;
            }
        }
    }
}
