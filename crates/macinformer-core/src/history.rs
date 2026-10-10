//! Fixed-capacity ring buffer backing the System Information graphs.

#[derive(Clone, Debug)]
pub struct HistoryRing<T: Copy + Default> {
    buf: Vec<T>,
    head: usize,
    len: usize,
}

impl<T: Copy + Default> HistoryRing<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![T::default(); capacity.max(1)],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, v: T) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % self.buf.len();
        self.len = (self.len + 1).min(self.buf.len());
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Oldest → newest iteration over the last `n` samples.
    pub fn iter_last(&self, n: usize) -> impl Iterator<Item = T> + '_ {
        let n = n.min(self.len);
        let cap = self.buf.len();
        let start = (self.head + cap - n) % cap;
        (0..n).map(move |i| self.buf[(start + i) % cap])
    }

    pub fn latest(&self) -> Option<T> {
        if self.len == 0 {
            None
        } else {
            let idx = (self.head + self.buf.len() - 1) % self.buf.len();
            Some(self.buf[idx])
        }
    }
}

impl HistoryRing<f32> {
    pub fn max_last(&self, n: usize) -> f32 {
        self.iter_last(n).fold(0.0_f32, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_wraps() {
        let mut r = HistoryRing::new(4);
        for i in 0..10 {
            r.push(i);
        }
        let v: Vec<i32> = r.iter_last(4).collect();
        assert_eq!(v, vec![6, 7, 8, 9]);
        assert_eq!(r.latest(), Some(9));
        assert_eq!(r.len(), 4);
    }

    #[test]
    fn ring_short() {
        let mut r = HistoryRing::new(8);
        r.push(1.0_f32);
        r.push(3.0);
        assert_eq!(r.max_last(8), 3.0);
        let v: Vec<f32> = r.iter_last(8).collect();
        assert_eq!(v, vec![1.0, 3.0]);
    }
}
