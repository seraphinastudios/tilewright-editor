// Copyright 2026 Seraphina Studios
// SPDX-License-Identifier: MIT

//! Bounded reading of newline-delimited messages, for the servers that
//! read one JSON message per line on standard input: the read-only MCP
//! server and the sandbox runner. A line over the cap is skipped to its
//! newline without being kept, so no line can make a reader hold more than
//! the cap.
use std::io::{self, BufRead};

/// What [`read_line`] found.
#[derive(Debug, Eq, PartialEq)]
pub enum Line {
    /// A message, in the buffer, without its newline.
    Message,
    /// A line longer than the cap, which was skipped to its newline.
    TooLong,
    /// The end of the input.
    End,
}

/// Reads the next line into `line`, keeping at most `maximum` bytes, not
/// counting a CR that ends it: a longer line is read on to its newline
/// without being kept. The last line may lack a newline.
pub fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>, maximum: usize) -> io::Result<Line> {
    line.clear();
    // One byte more is kept, in case it is the CR of a CR LF.
    let kept = maximum + 1;
    let fits = |line: &Vec<u8>| line.len() <= maximum || line.last() == Some(&b'\r');
    let mut too_long = false;
    loop {
        let available = match input.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if too_long || !fits(line) {
                Line::TooLong
            } else if line.is_empty() {
                Line::End
            } else {
                Line::Message
            });
        }
        let (part, end) = match available.iter().position(|byte| *byte == b'\n') {
            Some(newline) => (&available[..newline], Some(newline + 1)),
            None => (available, None),
        };
        if !too_long {
            if line.len() + part.len() > kept {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(part);
            }
        }
        let used = end.unwrap_or(part.len());
        input.consume(used);
        if end.is_some() {
            return Ok(if too_long || !fits(line) {
                Line::TooLong
            } else {
                Line::Message
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Line, read_line};
    use std::io::Cursor;

    fn lines(input: &[u8], maximum: usize) -> Vec<(Line, Vec<u8>)> {
        let mut input = Cursor::new(input);
        let mut line = Vec::new();
        let mut found = Vec::new();
        loop {
            let kind = read_line(&mut input, &mut line, maximum).unwrap();
            if kind == Line::End {
                return found;
            }
            // A line too long is not used, whatever is left of it.
            let kept = if kind == Line::Message {
                line.clone()
            } else {
                Vec::new()
            };
            found.push((kind, kept));
        }
    }

    #[test]
    fn lines_are_capped_without_being_kept_and_reading_goes_on() {
        let found = lines(b"abc\r\nabcd\nab\r\r\nxy", 3);
        assert_eq!(
            found,
            vec![
                (Line::Message, b"abc\r".to_vec()),
                (Line::TooLong, Vec::new()),
                // Three bytes before the CR that ends the line: at the cap.
                (Line::Message, b"ab\r\r".to_vec()),
                (Line::Message, b"xy".to_vec()),
            ]
        );
        assert_eq!(lines(b"abcd", 3), vec![(Line::TooLong, Vec::new())]);
        assert_eq!(lines(b"", 3), vec![]);
        assert_eq!(lines(b"\n", 3), vec![(Line::Message, Vec::new())]);
    }
}
