//! Split a concatenated MJPEG byte stream (image2pipe) into JPEG frames.

/// Incremental splitter based on SOI (FFD8) / EOI (FFD9) markers.
#[derive(Debug, Default)]
pub struct JpegSplitter {
    buf: Vec<u8>,
}

const MAX_FRAME: usize = 8 * 1024 * 1024;

impl JpegSplitter {
    /// Feed bytes; returns all complete frames.
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        loop {
            let Some(start) = find(&self.buf, &[0xFF, 0xD8], 0) else {
                self.buf.clear();
                break;
            };
            match find(&self.buf, &[0xFF, 0xD9], start + 2) {
                Some(end) => {
                    frames.push(self.buf[start..end + 2].to_vec());
                    self.buf.drain(..end + 2);
                }
                None => {
                    if start > 0 {
                        self.buf.drain(..start);
                    }
                    if self.buf.len() > MAX_FRAME {
                        self.buf.clear();
                    }
                    break;
                }
            }
        }
        frames
    }
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if h.len() < n.len() || from > h.len() - n.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|i| &h[*i..*i + n.len()] == n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_across_chunks() {
        let mut s = JpegSplitter::default();
        let f1 = [0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9];
        let f2 = [0xFF, 0xD8, 9, 0xFF, 0xD9];
        let mut all = f1.to_vec();
        all.extend_from_slice(&f2);
        let (a, b) = all.split_at(9);
        let mut got = s.push(a);
        got.extend(s.push(b));
        assert_eq!(got, vec![f1.to_vec(), f2.to_vec()]);
    }
}
