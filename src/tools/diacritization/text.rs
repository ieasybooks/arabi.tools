//! Bounded UTF-8 reads for TXT files, retaining every source byte between calls.

use std::collections::VecDeque;
use std::io::{self, BufReader, Read};

pub(super) const CHUNK_SCALARS: usize = 500;

pub(super) struct TextChunks<R> {
    bytes: io::Bytes<BufReader<R>>,
    pending: VecDeque<char>,
}

impl<R: Read> TextChunks<R> {
    pub fn new(reader: R) -> Self {
        Self {
            bytes: BufReader::new(reader).bytes(),
            pending: VecDeque::with_capacity(CHUNK_SCALARS + 1),
        }
    }

    pub fn next_chunk(&mut self) -> io::Result<Option<String>> {
        // One lookahead scalar distinguishes a complete final chunk from a long word.
        for _ in self.pending.len()..=CHUNK_SCALARS {
            match next_character(&mut self.bytes)? {
                Some(character) => self.pending.push_back(character),
                None => break,
            }
        }
        if self.pending.is_empty() {
            return Ok(None);
        }
        let mut end = 0;
        let mut word_boundary = 0;
        let mut sentence_boundary = false;
        for (offset, &character) in self.pending.iter().take(CHUNK_SCALARS).enumerate() {
            end = offset + 1;
            if character.is_whitespace() {
                word_boundary = end;
            }
            if matches!(
                character,
                '\n' | '\r' | '\u{2028}' | '\u{2029}' | '.' | '!' | '?' | '؟' | '؛'
            ) {
                sentence_boundary = true;
                break;
            }
        }
        if !sentence_boundary && end < self.pending.len() && word_boundary > 0 {
            end = word_boundary;
        }
        Ok(Some(self.pending.drain(..end).collect()))
    }
}

fn next_character(bytes: &mut impl Iterator<Item = io::Result<u8>>) -> io::Result<Option<char>> {
    let Some(first) = bytes.next().transpose()? else {
        return Ok(None);
    };
    let length = match first {
        0..=0x7f => return Ok(Some(char::from(first))),
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Err(invalid_utf8()),
    };
    let mut encoded = [first, 0, 0, 0];
    for byte in &mut encoded[1..length] {
        *byte = bytes.next().transpose()?.ok_or_else(invalid_utf8)?;
    }
    std::str::from_utf8(&encoded[..length])
        .map_err(|_| invalid_utf8())?
        .chars()
        .next()
        .map(Some)
        .ok_or_else(invalid_utf8)
}

fn invalid_utf8() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "TXT is not valid UTF-8")
}
