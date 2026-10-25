//! `--text-websocket`: authored Higurashi messages reach a real WebSocket
//! client through the normal Runtime. Temporary progress only.
use super::*;
use std::sync::mpsc;

#[test]
#[ignore = "actual Runtime title/story/MSGSET text over a local WebSocket;real assets/offscreen Metal/the default audio output;temporary progress only"]
fn higurashi_runtime_streams_authored_messages_to_websocket_clients() {
    let local = crate::text_stream::start("127.0.0.1:0").unwrap();
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut client, _) = tungstenite::connect(format!("ws://{local}")).unwrap();
        while let Ok(tungstenite::Message::Text(text)) = client.read() {
            if sender.send(text.to_string()).is_err() {
                break;
            }
        }
    });
    let mut journey = Journey::new("text-stream", &SaveData::default());
    let viewport = viewport((1280, 720));
    let deadline = Instant::now() + Duration::from_secs(150);
    journey.boot_title(viewport, deadline);
    journey.start_story(viewport, deadline);
    let mut texts = Vec::new();
    while texts.len() < 3 {
        assert!(Instant::now() < deadline, "messages were not streamed: {texts:?}");
        journey.tap(KeyCode::Enter, viewport);
        texts.extend(received.try_iter());
    }
    for text in &texts {
        println!("HIGURASHI_TEXT_STREAM {text:?}");
        assert!(!text.is_empty() && !text.contains('@'), "{text:?}");
        assert!(!text.chars().any(|c| ('ｦ'..='ﾟ').contains(&c)), "undecoded fixup: {text:?}");
    }
    assert!(texts.windows(2).any(|w| w[0] != w[1]), "{texts:?}");
    // The third authored message is spoken by 大石; speaker names are not streamed.
    assert!(!texts.iter().any(|text| text.starts_with("大石")), "{texts:?}");
}
