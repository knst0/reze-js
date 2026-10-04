#[derive(Clone, Copy)]
pub struct SiteId {
    pub ordinal: u32,
}

pub struct SiteRegistry {
    prefix: String,
    next: u32,
}

impl SiteRegistry {
    pub fn new(module_id: &str, source: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(module_id.as_bytes());
        hasher.update(&[0]);
        hasher.update(source.as_bytes());
        let digest = hasher.finalize();
        let mut prefix = String::with_capacity(16);
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in digest.as_bytes().iter().take(8) {
            prefix.push(HEX[(byte >> 4) as usize] as char);
            prefix.push(HEX[(byte & 15) as usize] as char);
        }
        Self { prefix, next: 0 }
    }

    pub fn assign(&mut self) -> SiteId {
        let ordinal = self.next;
        self.next += 1;
        SiteId { ordinal }
    }

    pub fn key(&self, site: SiteId) -> String {
        let mut key = String::with_capacity(self.prefix.len() + 8);
        key.push_str(&self.prefix);
        key.push('_');
        push_base36(&mut key, site.ordinal);
        key
    }
}

fn push_base36(out: &mut String, mut value: u32) {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut digits = [0; 7];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = DIGITS[(value % 36) as usize];
        value /= 36;
        if value == 0 {
            break;
        }
    }
    for &digit in &digits[start..] {
        out.push(char::from(digit));
    }
}
