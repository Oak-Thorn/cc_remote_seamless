use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;

pub struct PtySession {
    master_write: Arc<Mutex<Box<dyn Write + Send>>>,
    output_rx: mpsc::UnboundedReceiver<String>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtySession {
    pub fn spawn(cmd: &str, args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new(cmd);
        for arg in args {
            command.arg(arg);
        }

        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let (output_tx, output_rx) = mpsc::unbounded_channel();

        thread::spawn(move || {
            let buf = BufReader::new(reader);
            for line in buf.lines() {
                match line {
                    Ok(text) => {
                        if output_tx.send(text).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            master_write: Arc::new(Mutex::new(writer)),
            output_rx,
            child,
        })
    }

    pub fn inject_input(&self, data: &str) -> Result<(), std::io::Error> {
        let mut writer = self.master_write.lock().unwrap();
        writer.write_all(data.as_bytes())?;
        writer.flush()
    }

    pub async fn next_output(&mut self) -> Option<String> {
        self.output_rx.recv().await
    }

    pub fn wait(&mut self) -> Option<i32> {
        self.child.wait().ok().map(|s| {
            if s.success() { 0 } else { 1 }
        })
    }
}
