use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

pub struct Pty {
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub output: Arc<Mutex<Vec<u8>>>,
}

impl Pty {
    pub fn spawn(shell: &str, cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let _child = pair.slave.spawn_command(cmd)?;

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let output_clone = Arc::clone(&output);

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut lock = output_clone.lock().unwrap();
                        lock.extend_from_slice(&buf[..n]);
                    }
                }
            }
        });

        Ok(Self { master: pair.master, writer, output })
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    pub fn write_bytes(&mut self, data: &[u8]) {
        let _ = self.writer.write_all(data);
        let _ = self.writer.flush();
    }

    /// Drain bytes accumulated by the reader thread since last call.
    pub fn drain(&self) -> Vec<u8> {
        let mut lock = self.output.lock().unwrap();
        std::mem::take(&mut *lock)
    }
}
