//! A bounded byte ring: the replay buffer for one session (spec 2.1). Pushing past capacity drops
//! the oldest bytes. `snapshot` returns the bytes oldest first.

pub struct RingBuffer {
    buf: Vec<u8>,
    cap: usize,
    start: usize,
    len: usize,
    /// Bytes ever pushed, so a reader can name a position in the stream the ring may no longer hold.
    total: u64,
}

impl RingBuffer {
    pub fn new(cap: usize) -> Self {
        assert!(cap > 0, "replay capacity must be positive");
        Self { buf: vec![0; cap], cap, start: 0, len: 0, total: 0 }
    }

    /// A ring holding `bytes` (the tail of them when longer than `cap`): the replay carried
    /// across a handover.
    pub fn from_bytes(cap: usize, bytes: &[u8]) -> Self {
        let mut r = Self::new(cap);
        r.push(bytes);
        r
    }

    #[allow(dead_code)] // part of the ring API; only tests read it so far
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
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

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// The bytes at stream positions `from..total`, when the ring still holds them all; `None`
    /// when some were dropped (the caller writes the snapshot instead) or `from` is past the end.
    pub fn since(&self, from: u64) -> Option<Vec<u8>> {
        let oldest_kept = self.total - self.len as u64;
        if from < oldest_kept || from > self.total {
            return None;
        }
        let skip = (from - oldest_kept) as usize;
        Some(self.snapshot()[skip..].to_vec())
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

    #[test]
    fn from_bytes_restores_a_snapshot_and_keeps_the_tail_when_too_long() {
        let r = RingBuffer::from_bytes(8, b"abcde");
        assert_eq!(r.snapshot(), b"abcde");
        assert_eq!(r.len(), 5);
        let r = RingBuffer::from_bytes(3, b"0123456789");
        assert_eq!(r.snapshot(), b"789");
        let mut r = RingBuffer::from_bytes(4, b"ab");
        r.push(b"cde");
        assert_eq!(r.snapshot(), b"bcde");
    }

    #[test]
    fn total_counts_every_byte_pushed_and_since_gives_the_unflushed_tail() {
        let mut r = RingBuffer::new(4);
        assert_eq!((r.total(), r.capacity()), (0, 4));
        r.push(b"ab");
        assert_eq!(r.total(), 2);
        assert_eq!(r.since(0), Some(b"ab".to_vec()));
        assert_eq!(r.since(1), Some(b"b".to_vec()));
        assert_eq!(r.since(2), Some(vec![]), "nothing new");
        r.push(b"cde");
        assert_eq!(r.total(), 5, "the dropped byte still counts");
        assert_eq!(r.since(2), Some(b"cde".to_vec()));
        assert_eq!(r.since(1), Some(b"bcde".to_vec()), "offset 1 is the oldest byte kept");
        assert_eq!(r.since(0), None, "offset 0 was dropped: rewrite from the snapshot");
        assert_eq!(r.since(9), None, "past the end is not a position in the stream");
        r.push(b"0123456789");
        assert_eq!(r.total(), 15);
        assert_eq!(r.since(11), Some(b"6789".to_vec()));
        let f = RingBuffer::from_bytes(8, b"abc");
        assert_eq!(f.total(), 3);
        assert_eq!(f.since(0), Some(b"abc".to_vec()));
    }
}
