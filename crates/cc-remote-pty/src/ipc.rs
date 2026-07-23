use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tracing::{info, error};

use crate::protocol::{self, PtyMessage};

pub fn socket_path(agent: &str, session_id: &str) -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from(format!("/tmp/cc-remote-{}-{}.sock", agent, session_id))
    }
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\cc-remote-{}-{}", agent, session_id))
    }
}

pub struct IpcServer {
    input_rx: mpsc::UnboundedReceiver<String>,
    output_tx: mpsc::UnboundedSender<String>,
}

impl IpcServer {
    #[cfg(unix)]
    pub async fn listen(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use tokio::net::UnixListener;

        if path.exists() {
            std::fs::remove_file(path)?;
        }

        let listener = UnixListener::bind(path)?;
        info!("IPC listening on {:?}", path);

        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let (output_tx, output_rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            Self::accept_loop(listener, input_tx, output_rx).await;
        });

        Ok(Self { input_rx, output_tx })
    }

    #[cfg(unix)]
    async fn accept_loop(
        listener: tokio::net::UnixListener,
        input_tx: mpsc::UnboundedSender<String>,
        mut output_rx: mpsc::UnboundedReceiver<String>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let (reader, mut writer) = stream.into_split();
                    let mut buf_reader = BufReader::new(reader);
                    let tx = input_tx.clone();

                    // Read the client's input lines into the input channel.
                    let read_fut = async move {
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match buf_reader.read_line(&mut line).await {
                                Ok(0) => break,
                                Ok(_) => {
                                    if let Some(msg) = protocol::decode(&line) {
                                        if let PtyMessage::Input { data } = msg {
                                            let _ = tx.send(data);
                                        }
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    };

                    // Pump PTY output to the client. Borrow `output_rx` (rather
                    // than move it) so the single receiver survives across
                    // connections — otherwise the second `accept()` iteration
                    // would try to reuse a moved value.
                    let out_rx = &mut output_rx;
                    let write_fut = async move {
                        while let Some(text) = out_rx.recv().await {
                            let msg = protocol::encode(&PtyMessage::Output { data: text });
                            if writer.write_all(msg.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    };

                    // Whichever side finishes first ends the connection: a
                    // closed client ends the read side, a closed PTY the write
                    // side. Dropping the other future releases the `output_rx`
                    // borrow so the next client can connect.
                    tokio::select! {
                        _ = read_fut => {}
                        _ = write_fut => {}
                    }
                }
                Err(e) => {
                    error!("IPC accept error: {}", e);
                    break;
                }
            }
        }
    }

    pub async fn next_input(&mut self) -> Option<String> {
        self.input_rx.recv().await
    }

    pub fn send_output(&self, text: String) {
        let _ = self.output_tx.send(text);
    }
}
