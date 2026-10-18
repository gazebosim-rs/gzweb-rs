// Modified for gzweb-rs; see `gz-collada:` markers (OBJ/STL-only code removed).

pub(crate) mod bytes;
pub mod float;
pub(crate) mod hex;
pub mod int;
pub(crate) mod xml;

pub(crate) mod utf16 {
    use std::{borrow::Cow, io};

    use crate::vendor::error;

    const UTF32BE_BOM: &[u8] = &[0xFF, 0xFE, 00, 00];
    const UTF32LE_BOM: &[u8] = &[00, 00, 0xFE, 0xFF];
    const UTF16BE_BOM: &[u8] = &[0xFE, 0xFF];
    const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];
    const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

    /// Converts bytes to a string. Converts to UTF-8 if bytes are UTF-16 and have BOM.
    pub(crate) fn decode_string(bytes: &[u8]) -> io::Result<Cow<'_, str>> {
        if bytes.starts_with(UTF8_BOM) {
            std::str::from_utf8(&bytes[UTF8_BOM.len()..])
                .map(Cow::Borrowed)
                .map_err(error::invalid_data)
        } else if bytes.starts_with(UTF32BE_BOM) || bytes.starts_with(UTF32LE_BOM) {
            return Err(error::invalid_data("utf-32 is not supported"));
        } else if bytes.starts_with(UTF16BE_BOM) {
            from_utf16be(&bytes[UTF16BE_BOM.len()..]).map(Into::into)
        } else if bytes.starts_with(UTF16LE_BOM) {
            from_utf16le(&bytes[UTF16BE_BOM.len()..]).map(Into::into)
        } else {
            // UTF-16/UTF-32 without BOM will get an error here.
            std::str::from_utf8(bytes)
                .map(Cow::Borrowed)
                .map_err(error::invalid_data)
        }
    }

    #[cold]
    #[inline(never)]
    fn from_utf16be(bytes: &[u8]) -> io::Result<String> {
        if bytes.len() % 2 != 0 {
            return Err(error::invalid_data("invalid utf-16: lone surrogate found"));
        }
        char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes(b.try_into().unwrap())),
        )
        .collect::<Result<String, _>>()
        .map_err(error::invalid_data)
    }

    #[cold]
    #[inline(never)]
    fn from_utf16le(bytes: &[u8]) -> io::Result<String> {
        if bytes.len() % 2 != 0 {
            return Err(error::invalid_data("invalid utf-16: lone surrogate found"));
        }
        char::decode_utf16(
            bytes
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes(b.try_into().unwrap())),
        )
        .collect::<Result<String, _>>()
        .map_err(error::invalid_data)
    }
}
