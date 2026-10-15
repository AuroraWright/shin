use super::*;

fn encoded_sound(channels: ChannelCount, sample_rate: u16, payload: &[u8]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    SoundHeader {
        file_size: 16 + payload.len() as u32,
        channel_count: channels,
        sample_rate,
        sample_count: 2430,
    }
    .write_le(&mut cursor)
    .unwrap();
    let mut bytes = cursor.into_inner();
    bytes.extend_from_slice(payload);
    bytes
}

#[test]
fn invalid_adpcm_headers_are_rejected_before_source_construction() {
    for channels in [ChannelCount::Mono, ChannelCount::Stereo] {
        let count = channels as usize;
        for channel in 0..count {
            for block in [0, 1, 80] {
                for filter in 5..=15 {
                    let mut payload = vec![0; 81 * count * 16];
                    payload[block * count * 16 + channel * 16] = filter << 4;
                    let message = match Sound::new(&encoded_sound(channels, 48000, &payload)) {
                        Ok(_) => panic!("Unsupported filter {filter} was accepted"),
                        Err(error) => error.to_string(),
                    };
                    assert!(message.contains(&format!("filter {filter}")));
                    assert!(message.contains(&format!("block {block}, channel {channel}")));
                }
            }
        }
        assert!(Sound::new(&encoded_sound(channels, 0, &[0; 32])).is_err());
    }
}

#[test]
fn valid_adpcm_headers_and_partial_tails_keep_their_decoded_samples() {
    use crate::format::audio::AudioSource;
    for channels in [ChannelCount::Mono, ChannelCount::Stereo] {
        let count = channels as usize;
        for filter in 0..=4 {
            for shift in 0..=15 {
                let mut payload = vec![0x12; count * 16];
                for channel in 0..count {
                    payload[channel * 16] = filter << 4 | shift;
                }
                let sound = Sound::new(&encoded_sound(channels, 48000, &payload)).unwrap();
                let mut source = AudioSource::new(sound.decode());
                let expected: Vec<_> = std::iter::from_fn(|| source.read_sample()).collect();
                assert_eq!(expected.len(), 30);
                for tail in [1, count * 16 - 1] {
                    let mut partial = payload.clone();
                    // A partial channel frame must still be ignored, even if
                    // its apparent header requests an unsupported coefficient.
                    partial.extend(std::iter::repeat_n(0xf0, tail));
                    let sound = Sound::new(&encoded_sound(channels, 48000, &partial)).unwrap();
                    let mut source = AudioSource::new(sound.decode());
                    let actual: Vec<_> = std::iter::from_fn(|| source.read_sample()).collect();
                    assert_eq!(actual, expected);
                    assert_eq!(source.current_samples_position(), 30);
                }
            }
        }
    }
}
