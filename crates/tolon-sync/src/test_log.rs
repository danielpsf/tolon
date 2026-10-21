//! Every log line of this crate's tests, for the redaction tests. One global subscriber: a
//! per-thread one races with callsite interest caching in parallel tests.

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

#[derive(Clone, Default)]
pub struct Buf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buf {
    type Writer = Buf;
    fn make_writer(&'a self) -> Buf {
        self.clone()
    }
}

/// Everything logged so far by any test in this binary, debug level included.
pub fn captured() -> String {
    static LOG: OnceLock<Buf> = OnceLock::new();
    let buf = LOG.get_or_init(|| {
        let buf = Buf::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _ = tracing::subscriber::set_global_default(sub);
        buf
    });
    String::from_utf8_lossy(&buf.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
}
