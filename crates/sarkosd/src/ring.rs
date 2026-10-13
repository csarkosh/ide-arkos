//! A bounded byte ring: the replay buffer for one session (spec 2.1). Pushing past capacity drops
//! the oldest bytes. `snapshot` returns the bytes oldest first.

pub struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
    start: usize,
    len: usize,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "replay capacity must be positive");
        Self { buf: vec![0; cap], cap, start: 0, len: 0 }
    }

    #[allow(dead_code)] // part of the ring API; only tests read it so far
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, bytes: &[u8]) {
        let bytes = if bytes.len() > self.cap { &bytes[bytes.len() - self.cap..] } else { bytes };
        for &b in bytes {
            let end = (self.start + self.len) % self.cap;
            self.buf[end] = b;
            if self.len == self.cap {
                self.start = (self.start + 1) % self.cap;
            } else {
                self.len += 1;
            }
        }
    }

    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.len);
        for i in 0..self.len {
            out.push(self.buf[(self.start + i) % self.cap]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_everything_under_capacity() {
        let mut r = RingBuffer::new(8);
        r.push(b"abc");
        r.push(b"de");
        assert_eq!(r.snapshot(), b"abcde");
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn drops_oldest_when_full() {
        let mut r = RingBuffer::new(5);
        r.push(b"abcdef");
        assert_eq!(r.snapshot(), b"bcdef");
        r.push(b"gh");
        assert_eq!(r.snapshot(), b"defgh");
    }

    #[test]
    fn push_larger_than_capacity_keeps_the_tail() {
        let mut r = RingBuffer::new(3);
        r.push(b"0123456789");
        assert_eq!(r.snapshot(), b"789");
        assert_eq!(r.len(), 3);
    }

    #[test]
    fn wraps_correctly_many_times() {
        let mut r = RingBuffer::new(4);
        for i in 0..100u8 {
            r.push(&[i]);
        }
        assert_eq!(r.snapshot(), vec![96, 97, 98, 99]);
    }
}
