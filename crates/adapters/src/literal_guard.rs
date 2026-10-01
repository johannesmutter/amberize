//! Reject excessive IMAP literals before the protocol parser allocates their announced size.
use futures_util::io::{AsyncRead, AsyncWrite};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
const MAX_LITERAL: usize = 50 * 1024 * 1024;
const MAX_STAGE_BYTES: usize = 60 * 1024 * 1024;
#[derive(Debug)]
pub struct LiteralGuard<T> {
    inner: T,
    literal_remaining: usize,
    line: Vec<u8>,
    stage_bytes: usize,
}
impl<T> LiteralGuard<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            literal_remaining: 0,
            line: Vec::with_capacity(128),
            stage_bytes: 0,
        }
    }
    pub fn reset_budget(&mut self) {
        self.stage_bytes = 0;
    }
    fn inspect(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stage_bytes = self.stage_bytes.saturating_add(bytes.len());
        if self.stage_bytes > MAX_STAGE_BYTES {
            return Err(io::Error::other(
                "IMAP response exceeded the 60 MiB stage budget; cursor retained for retry",
            ));
        }
        let mut position = 0;
        while position < bytes.len() {
            if self.literal_remaining > 0 {
                let skip = self.literal_remaining.min(bytes.len() - position);
                self.literal_remaining -= skip;
                position += skip;
                continue;
            }
            let byte = bytes[position];
            position += 1;
            if self.line.len() == 128 {
                self.line.remove(0);
            }
            self.line.push(byte);
            if byte == b'\n' {
                if self.line.ends_with(b"}\r\n") {
                    if let Some(start) = self.line.iter().rposition(|b| *b == b'{') {
                        let digits = &self.line[start + 1..self.line.len() - 3];
                        let digits = digits.strip_suffix(b"+").unwrap_or(digits);
                        if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) {
                            let length = std::str::from_utf8(digits)
                                .ok()
                                .and_then(|v| v.parse::<usize>().ok())
                                .ok_or_else(|| io::Error::other("Invalid IMAP literal length"))?;
                            if length > MAX_LITERAL {
                                return Err(io::Error::other("IMAP literal exceeds the 50 MiB archive limit; cursor retained for retry"));
                            }
                            self.literal_remaining = length;
                        }
                    }
                }
                self.line.clear();
            }
        }
        Ok(())
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for LiteralGuard<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(n)) => match self.inspect(&buf[..n]) {
                Ok(()) => Poll::Ready(Ok(n)),
                Err(e) => Poll::Ready(Err(e)),
            },
            other => other,
        }
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for LiteralGuard<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_announced_giant_literal_before_any_body_allocation() {
        let mut guard = LiteralGuard::new(());
        assert!(guard.inspect(b"* 1 FETCH (BODY[] {999999999}\r\n").is_err());
    }
    #[test]
    fn split_header_is_checked_and_literal_contents_are_not_mistaken_for_headers() {
        let mut guard = LiteralGuard::new(());
        guard.inspect(b"* 1 FETCH (BODY[] {999999").unwrap();
        assert!(guard.inspect(b"999}\r\n").is_err());
        let body = b"{999999999}\r\n";
        let mut guard = LiteralGuard::new(());
        guard
            .inspect(format!("* 1 FETCH (BODY[] {{{}}}\r\n", body.len()).as_bytes())
            .unwrap();
        guard.inspect(body).unwrap();
        assert_eq!(guard.literal_remaining, 0);
        guard.inspect(b")\r\n").unwrap();
    }
}
