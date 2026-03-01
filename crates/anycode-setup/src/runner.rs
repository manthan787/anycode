use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

/// A line of output from a subprocess.
#[derive(Debug, Clone)]
pub enum OutputLine {
    Stdout(String),
    Stderr(String),
    Finished(Option<i32>),
}

/// Runs a command asynchronously, streaming output lines via an mpsc channel.
/// Returns a receiver that yields OutputLine items.
pub fn run_command(
    program: &str,
    args: &[&str],
) -> anyhow::Result<mpsc::Receiver<OutputLine>> {
    let (tx, rx) = mpsc::channel();

    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let tx_out = tx.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                let _ = tx_out.send(OutputLine::Stdout(line));
            }
        }
    });

    let tx_err = tx.clone();
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                let _ = tx_err.send(OutputLine::Stderr(line));
            }
        }
    });

    thread::spawn(move || {
        let status = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(OutputLine::Finished(status));
    });

    Ok(rx)
}
