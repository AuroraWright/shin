//! Length-prefixed UTF-8 strings used by Gerokasu2. No Shift-JIS fixup.
use std::{
    io::{Read, Seek, Write},
    marker::PhantomData,
};

use binrw::{BinRead, BinResult, BinWrite, Endian};

use super::string::StringLengthDesc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utf8String<L>(pub String, PhantomData<L>);

pub type U16Utf8String = Utf8String<u16>;

impl<L> Utf8String<L> {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into(), PhantomData)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<L: StringLengthDesc> BinRead for Utf8String<L> {
    type Args<'a> = ();

    fn read_options<R: Read + Seek>(reader: &mut R, endian: Endian, _: ()) -> BinResult<Self> {
        let pos = reader.stream_position()?;
        let length = L::read_options(reader, endian, ())?
            .get_length()
            .ok_or_else(|| binrw::Error::AssertFail {
                pos,
                message: "UTF-8 string needs a length".into(),
            })?;
        let mut bytes = vec![0; length];
        reader.read_exact(&mut bytes)?;
        if bytes.pop() != Some(0) || bytes.contains(&0) {
            return Err(binrw::Error::AssertFail {
                pos,
                message: "Invalid UTF-8 string terminator".into(),
            });
        }
        let value = String::from_utf8(bytes).map_err(|error| binrw::Error::Custom {
            pos,
            err: Box::new(error),
        })?;
        Ok(Self::new(value))
    }
}

impl<L: StringLengthDesc> BinWrite for Utf8String<L> {
    type Args<'a> = ();

    fn write_options<W: Write + Seek>(
        &self,
        writer: &mut W,
        endian: Endian,
        _: (),
    ) -> BinResult<()> {
        let pos = writer.stream_position()?;
        let length = L::from_length(self.0.len() + 1)
            .filter(|_| !self.0.contains('\0'))
            .ok_or_else(|| binrw::Error::AssertFail {
                pos,
                message: "Invalid UTF-8 string length or embedded NUL".into(),
            })?;
        length.write_options(writer, endian, ())?;
        writer.write_all(self.0.as_bytes())?;
        writer.write_all(&[0])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn gerokasu2_utf8_lengths_count_bytes_and_preserve_hiragana() {
        let text = U16Utf8String::new("日本かわいい");
        let mut data = Cursor::new(Vec::new());
        text.write_le(&mut data).unwrap();
        assert_eq!(&data.get_ref()[..2], &19u16.to_le_bytes());
        data.set_position(0);
        assert_eq!(U16Utf8String::read_le(&mut data).unwrap(), text);
        for bytes in [
            &[0, 0][..],
            &[2, 0, 0xff, 0],
            &[2, 0, b'x', b'y'],
            &[3, 0, 0, 0, 0],
        ] {
            assert!(U16Utf8String::read_le(&mut Cursor::new(bytes)).is_err());
        }
    }
}
