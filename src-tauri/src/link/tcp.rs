//! NBP over TCP: one connection carries both streams.

use super::{Input, Writer};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub(crate) async fn open(
    host: &str,
    port: u16,
    tx: mpsc::Sender<Input>,
) -> Result<(Writer, Vec<JoinHandle<()>>, String), String> {
    let s = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect((host, port)))
        .await
        .map_err(|_| format!("{host}:{port}: timed out"))?
        .map_err(|e| format!("{host}:{port}: {e}"))?;
    s.set_nodelay(true).ok();
    let (mut r, w) = s.into_split();
    let reader = tokio::spawn(async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match r.read(&mut buf).await {
                Ok(0) => {
                    let _ = tx.send(Input::Closed("the device closed the connection".into())).await;
                    break;
                }
                Ok(n) => {
                    if tx.send(Input::Bytes(0, buf[..n].to_vec())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Input::Closed(e.to_string())).await;
                    break;
                }
            }
        }
    });
    Ok((Writer::Tcp(w), vec![reader], format!("TCP {host}:{port}")))
}
