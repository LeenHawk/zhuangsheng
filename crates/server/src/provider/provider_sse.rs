const MAX_LINE_BYTES: usize = 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct SseDataDecoder {
    pending: Vec<u8>,
    data: Vec<u8>,
}

#[derive(Debug)]
pub(super) struct SseDecodeError;

impl SseDataDecoder {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, SseDecodeError> {
        if self.pending.len().saturating_add(bytes.len()) > MAX_LINE_BYTES {
            return Err(SseDecodeError);
        }
        self.pending.extend_from_slice(bytes);
        let mut frames = Vec::new();
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<_> = self.pending.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            self.line(&line, &mut frames)?;
        }
        Ok(frames)
    }

    pub(super) fn finish(mut self) -> Result<Vec<Vec<u8>>, SseDecodeError> {
        let mut frames = Vec::new();
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.line(&line, &mut frames)?;
        }
        self.dispatch(&mut frames);
        Ok(frames)
    }

    fn line(&mut self, line: &[u8], frames: &mut Vec<Vec<u8>>) -> Result<(), SseDecodeError> {
        if line.is_empty() {
            self.dispatch(frames);
            return Ok(());
        }
        if line[0] == b':' {
            return Ok(());
        }
        let Some((field, mut value)) = line
            .iter()
            .position(|byte| *byte == b':')
            .map(|index| (&line[..index], &line[index + 1..]))
        else {
            return Ok(());
        };
        if field != b"data" {
            return Ok(());
        }
        if value.first() == Some(&b' ') {
            value = &value[1..];
        }
        let separator = usize::from(!self.data.is_empty());
        if self
            .data
            .len()
            .saturating_add(separator)
            .saturating_add(value.len())
            > MAX_EVENT_BYTES
        {
            return Err(SseDecodeError);
        }
        if separator == 1 {
            self.data.push(b'\n');
        }
        self.data.extend_from_slice(value);
        Ok(())
    }

    fn dispatch(&mut self, frames: &mut Vec<Vec<u8>>) {
        if !self.data.is_empty() {
            frames.push(std::mem::take(&mut self.data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragmented_sse_is_reassembled_and_comments_are_ignored() {
        let mut decoder = SseDataDecoder::default();
        assert!(decoder.push(b": ping\r\ndata: {\"a\":").unwrap().is_empty());
        let frames = decoder
            .push(b"1}\r\n\r\ndata: first\ndata: second\n\n")
            .unwrap();
        assert_eq!(
            frames,
            vec![b"{\"a\":1}".to_vec(), b"first\nsecond".to_vec()]
        );
        assert!(decoder.finish().unwrap().is_empty());
    }

    #[test]
    fn unterminated_last_event_is_dispatched_at_eof() {
        let mut decoder = SseDataDecoder::default();
        assert!(
            decoder
                .push(b"event: message\ndata: final")
                .unwrap()
                .is_empty()
        );
        assert_eq!(decoder.finish().unwrap(), vec![b"final".to_vec()]);
    }
}
