//! Optional WebSocket text stream for text hookers (`--text-websocket HOST:PORT`).
//! Each connected client receives every displayed text as one plain-text
//! message, with the engine's `@` markup removed. A client that connects late
//! first receives the most recent text.
use std::{
    io::ErrorKind,
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{Arc, Mutex, OnceLock, mpsc},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use shin_core::{
    format::text::decode_string_fixup,
    layout::{MessageTextParser, ParsedCommand},
};
use tracing::{info, warn};
use tungstenite::Message;

/// How often a client thread checks for queued text between reads.
const POLL: Duration = Duration::from_millis(50);
/// A client that stops reading is dropped instead of stalling its thread.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

static STREAM: OnceLock<Arc<TextStream>> = OnceLock::new();

/// Start the process-wide server and return its bound address. Text sent
/// before this is ignored.
pub fn start(address: &str) -> Result<SocketAddr> {
    let (stream, local) = TextStream::bind(address)?;
    STREAM
        .set(stream)
        .map_err(|_| anyhow!("The text WebSocket server is already running"))?;
    info!(%local, "Text WebSocket server listening");
    Ok(local)
}

/// Broadcast already plain text. Does nothing when the server is not running.
pub fn send(text: &str) {
    if let Some(stream) = STREAM.get() {
        stream.broadcast(None, text);
    }
}

fn running() -> bool {
    STREAM.get().is_some()
}

/// Umineko, Higurashi and Gerokasu2 message boxes. All three games author the
/// speaker name as the first line, which is never streamed. A message that is
/// installed again right after itself (a restore re-entering MSGSET) is sent
/// once.
pub fn message(text: &str, markup: Markup, message_id: u32) {
    if let Some(stream) = STREAM.get() {
        stream.broadcast(Some(message_id), &plain_text(text, markup, true));
    }
}

/// A text page in standard markup (Umineko purple statements).
pub fn page(text: &str) {
    if running() {
        send(&plain_text(text, Markup::Standard, false));
    }
}

/// A titled description (Umineko characters and tips).
pub fn description(title: &str, body: &str) {
    if running() {
        let title = plain_text(title, Markup::Standard, false);
        let body = plain_text(body, Markup::Standard, false);
        send(&[title, body].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n"));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Markup {
    /// Umineko and Gerokasu2.
    Standard,
    /// Higurashi: half-width kana fixups and no bold commands.
    Higurashi,
}

/// Displayed characters only: `@r` becomes a newline, ruby readings and
/// control commands are dropped, and surrounding blank lines are trimmed.
pub fn plain_text(text: &str, markup: Markup, hide_name: bool) -> String {
    let decoded;
    let parser = match markup {
        Markup::Standard => MessageTextParser::new(text),
        Markup::Higurashi => {
            decoded = decode_string_fixup(text);
            MessageTextParser::new_higurashi(&decoded)
        }
    };
    let mut output = String::new();
    let mut skipping = hide_name;
    for command in parser {
        match command {
            ParsedCommand::Newline if skipping => skipping = false,
            _ if skipping => {}
            ParsedCommand::Char(c) => output.push(c),
            ParsedCommand::Newline => output.push('\n'),
            _ => {}
        }
    }
    output.trim_matches(|c| c == '\n' || c == '\r').to_owned()
}

#[derive(Default)]
struct Clients {
    senders: Vec<mpsc::Sender<Arc<str>>>,
    last: Option<Arc<str>>,
    /// The message ID of `last`, when it was a message box text.
    last_message: Option<u32>,
}

#[derive(Default)]
pub struct TextStream {
    clients: Mutex<Clients>,
}

impl TextStream {
    fn bind(address: &str) -> Result<(Arc<Self>, SocketAddr)> {
        let listener = TcpListener::bind(address)
            .with_context(|| format!("Binding the text WebSocket server to {address}"))?;
        let local = listener.local_addr()?;
        let stream = Arc::new(Self::default());
        let accepting = stream.clone();
        std::thread::Builder::new()
            .name("text-websocket".into())
            .spawn(move || {
                for incoming in listener.incoming() {
                    let tcp = match incoming {
                        Ok(tcp) => tcp,
                        Err(error) => {
                            warn!(%error, "Text WebSocket accept failed");
                            continue;
                        }
                    };
                    let stream = accepting.clone();
                    let spawned = std::thread::Builder::new()
                        .name("text-websocket-client".into())
                        .spawn(move || {
                            let peer = tcp.peer_addr().ok();
                            match stream.serve(tcp) {
                                Ok(()) => info!(?peer, "Text WebSocket client disconnected"),
                                Err(error) => {
                                    info!(?peer, %error, "Text WebSocket client dropped")
                                }
                            }
                        });
                    if let Err(error) = spawned {
                        warn!(%error, "Text WebSocket client thread failed to start");
                    }
                }
            })?;
        Ok((stream, local))
    }

    fn broadcast(&self, message_id: Option<u32>, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut clients = self.clients.lock().unwrap();
        if message_id.is_some()
            && clients.last_message == message_id
            && clients.last.as_deref() == Some(text)
        {
            return;
        }
        let text: Arc<str> = text.into();
        clients.last = Some(text.clone());
        clients.last_message = message_id;
        clients.senders.retain(|sender| sender.send(text.clone()).is_ok());
    }

    fn serve(&self, tcp: TcpStream) -> Result<()> {
        tcp.set_write_timeout(Some(WRITE_TIMEOUT))?;
        let mut socket = tungstenite::accept(tcp).context("WebSocket handshake")?;
        info!(peer = ?socket.get_ref().peer_addr().ok(), "Text WebSocket client connected");
        socket.get_ref().set_read_timeout(Some(POLL))?;
        let (sender, receiver) = mpsc::channel();
        {
            let mut clients = self.clients.lock().unwrap();
            if let Some(last) = &clients.last {
                sender.send(last.clone()).unwrap();
            }
            clients.senders.push(sender);
        }
        loop {
            loop {
                match receiver.try_recv() {
                    Ok(text) => socket.send(Message::text(text.as_ref()))?,
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            }
            // Reads answer pings and notice closes; client text is ignored.
            match socket.read() {
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_drops_markup_ruby_and_hidden_names() {
        let text = "右代宮　戦人@r@v00/awase.「@c900.赤@c.い@b真実.@<字@>だ@k@|」";
        assert_eq!(
            plain_text(text, Markup::Standard, false),
            "右代宮　戦人\n「赤い字だ」"
        );
        assert_eq!(plain_text(text, Markup::Standard, true), "「赤い字だ」");
        assert_eq!(plain_text("@r地の文", Markup::Standard, false), "地の文");
        assert_eq!(plain_text("名前", Markup::Standard, true), "");
        // Higurashi's half-width fixups decode to the displayed kana.
        assert_eq!(plain_text("ｱｲ@rｳ", Markup::Higurashi, false), "あい\nう");
    }

    fn read(client: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>) -> String {
        match client.read().unwrap() {
            Message::Text(text) => text.to_string(),
            other => panic!("unexpected frame {other:?}"),
        }
    }

    #[test]
    fn every_client_receives_broadcasts_and_late_clients_get_the_last_text() {
        let (stream, local) = TextStream::bind("127.0.0.1:0").unwrap();
        stream.broadcast(None, "");
        stream.broadcast(None, "一行目");
        let url = format!("ws://{local}");
        let (mut first, _) = tungstenite::connect(&url).unwrap();
        assert_eq!(read(&mut first), "一行目");
        let (mut second, _) = tungstenite::connect(&url).unwrap();
        assert_eq!(read(&mut second), "一行目");
        // Wait until both clients are registered before broadcasting.
        while stream.clients.lock().unwrap().senders.len() < 2 {
            std::thread::sleep(Duration::from_millis(5));
        }
        stream.broadcast(Some(7), "二行目\n続き");
        // The same message installed again is not repeated.
        stream.broadcast(Some(7), "二行目\n続き");
        assert_eq!(read(&mut first), "二行目\n続き");
        assert_eq!(read(&mut second), "二行目\n続き");
        // A closed client is dropped; the other keeps receiving.
        first.close(None).unwrap();
        while stream.clients.lock().unwrap().senders.len() > 1 {
            stream.broadcast(None, "三行目");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(read(&mut second), "三行目");
    }
}
