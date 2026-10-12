//! Admin commands typed into the server's terminal.

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::{Log, Request};
use crate::game::Ask;

/// Reads command lines from standard input until it closes, printing each
/// answer, and prints the admin log (except admin commands) to standard
/// error as it happens when given one.
pub async fn run(game: mpsc::UnboundedSender<Ask>, log: Option<Log>) {
    if let Some(log) = log {
        let mut rx = log.subscribe();
        tokio::spawn(async move {
            while let Ok(l) = rx.recv().await {
                if l.kind != "admin" {
                    eprintln!("[{}] {}: {}", l.room, l.kind, l.text);
                }
            }
        });
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut room: Option<String> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (tx, rx) = oneshot::channel();
        let req = Request {
            line: line.to_owned(),
            room: room.clone(),
            reply: tx,
        };
        if game.send(Ask::Admin(req)).is_err() {
            return;
        }
        let Ok(reply) = rx.await else { return };
        if reply.room.is_some() {
            room.clone_from(&reply.room);
        }
        if reply.ok {
            println!("{}", reply.text);
        } else {
            println!("error: {}", reply.text);
        }
    }
}
