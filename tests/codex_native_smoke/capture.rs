//! Match the native ownership fixture: drain independently of API waits and
//! retain bounded bytes. Keep draining during teardown, including on failure.
use super::*;

pub(super) struct Output {
    pub raw: Vec<u8>,
    pub screen: complete::NativeScreen,
    pub error: Option<&'static str>,
}

pub(super) struct Drainer {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drainer {
    pub fn start(mut master: File) -> (Arc<Mutex<Output>>, Self) {
        let output = Arc::new(Mutex::new(Output {
            raw: Vec::new(),
            screen: complete::NativeScreen::new(),
            error: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let sink = output.clone();
        let cancelled = stop.clone();
        let worker = thread::Builder::new()
            .name("codex-outer-pty".into())
            .spawn(move || {
                let mut buffer = [0u8; 16384];
                let mut cursor_query = 0;
                const QUERY: &[u8] = b"\x1b[6n";
                while !cancelled.load(Ordering::Acquire) {
                    match master.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            {
                                let mut output = sink.lock();
                                if output.raw.len().saturating_add(count) <= 32 * 1024 * 1024 {
                                    output.raw.extend_from_slice(&buffer[..count]);
                                    output.screen.advance(&buffer[..count]);
                                } else {
                                    output.error =
                                        Some("native evidence exceeded 32 MiB; draining continues");
                                }
                            }
                            for byte in &buffer[..count] {
                                if *byte == QUERY[cursor_query] {
                                    cursor_query += 1;
                                    if cursor_query == QUERY.len() {
                                        cursor_query = 0;
                                        if master.write_all(b"\x1b[1;1R").is_err() {
                                            sink.lock().error =
                                                Some("outer cursor response failed");
                                        }
                                    }
                                } else {
                                    cursor_query = usize::from(*byte == QUERY[0]);
                                }
                            }
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                        Err(_) => {
                            sink.lock().error = Some("outer native PTY read failed");
                            break;
                        }
                    }
                }
            })
            .expect("cannot start owned outer-PTY reader");
        (
            output,
            Self {
                stop,
                worker: Some(worker),
            },
        )
    }

    pub fn finish(&mut self) -> bool {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return true;
        };
        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        worker.join().is_ok()
    }
}

impl Drop for Drainer {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
