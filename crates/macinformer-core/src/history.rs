//! Bounded history for graphs (ring buffer).

#[derive(Clone, Debug)]
pub struct HistoryRing<T> {
    buf: std::collections::VecDeque<T>,
    cap: usize,
}

impl<T: Copy + Default> HistoryRing<T> {
    pub fn new(cap: usize) -> Self {
        HistoryRing { buf: std::collections::VecDeque::with_capacity(cap), cap }
    }
    pub fn push(&mut self, v: T) {
        if self.buf.len() >= self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }
    pub fn as_slice_padded(&self, w: usize) -> Vec<T> {
        let mut out = vec![T::default(); w.saturating_sub(self.buf.len())];
        out.extend(self.buf.iter().copied());
        out
    }
    pub fn len(&self) -> usize {
        self.buf.len()
    }
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
    pub fn latest(&self) -> Option<T> {
        self.buf.back().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_caps_and_pads() {
        let mut h = HistoryRing::new(3);
        for i in 0..5 {
            h.push(i);
        }
        assert_eq!(h.len(), 3);
        assert_eq!(h.as_slice_padded(5), vec![0, 0, 2, 3, 4]);
        assert_eq!(h.latest(), Some(4));
    }
}
