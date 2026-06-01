mod protocol;
mod pty;
mod ipc;

use clap::Parser;
use tracing::info;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "cc-remote-pty")]
#[command(about = "PTY proxy for remote Agent interaction")]
struct Cli {
    #[arg(long, default_value = "claude")]
    agent: String,

    #[arg(long)]
    session_id: Option<String>,

    #[arg(last = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let session_id = cli.session_id.unwrap_or_else(|| Uuid::new_v4().to_string()[..8].to_string());

    let (cmd, args) = if cli.command.is_empty() {
        ("claude".to_string(), vec![])
    } else {
        (cli.command[0].clone(), cli.command[1..].to_vec())
    };

    info!("Starting PTY proxy: agent={}, session={}, cmd={}", cli.agent, session_id, cmd);

    let mut pty_session = pty::PtySession::spawn(&cmd, &args)?;
    let sock_path = ipc::socket_path(&cli.agent, &session_id);
    let mut ipc = ipc::IpcServer::listen(&sock_path).await?;

    let idle_threshold = std::time::Duration::from_millis(500);
    let mut last_output = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(input) = ipc.next_input() => {
                pty_session.inject_input(&input)?;
            }
            Some(output) = pty_session.next_output() => {
                last_output = std::time::Instant::now();
                ipc.send_output(output);
            }
            _ = tokio::time::sleep(idle_threshold) => {
                if last_output.elapsed() >= idle_threshold {
                    let state_msg = protocol::encode(&protocol::PtyMessage::State { waiting: true });
                    ipc.send_output(state_msg);
                }
            }
        }
    }
}
