use unicode_segmentation::UnicodeSegmentation;

use crate::search::search_common::MATCH_ALL_VALUE;

/// Parses a string, processing escape sequences \\, \xXX, \0, \n, \t, \r, and allows specifying a "match all" '.' for matching any byte value (can be escaped
/// as \.). Collects the resolved values, or 0x8000 in the case of '.'s, into a Vec<u16>.
///
/// Ignores any errors or unexpected values/conditions that occur, e.g. invalid escape sequences such as \i will be ignored.
pub fn parse_match_str(string: &str) -> Vec<u16> {
	let mut buf: Vec<u16> = Vec::new();

	let gcs: Vec<&str> = string.graphemes(true).collect();

	let mut escaped = false;

	let mut i = 0;
	while i < gcs.len() {
		if escaped {
			escaped = false;
			match gcs[i] {
				"\\" => {
					buf.push(b'\\' as u16);
				}
				"n" => {
					buf.push(b'\n' as u16);
				}
				"t" => {
					buf.push(b'\t' as u16);
				}
				"r" => {
					buf.push(b'\r' as u16);
				}
				"0" => {
					buf.push(b'\0' as u16);
				}
				"." => {
					buf.push(b'.' as u16);
				}
				"x" => {
					if (i + 2) < gcs.len() {
						let hex_str = &gcs[(i + 1)..=(i + 2)].join("");
						if let Ok(val) = u8::from_str_radix(&hex_str, 16) {
							buf.push(val as u16);
						}
					}

					i += 3;
					continue;
				}
				_ => ()
			}
		} else {
			match gcs[i] {
				"\\" => {
					escaped = true;
				}
				"." => {
					buf.push(MATCH_ALL_VALUE);
				}
				c => {
					for &b in c.as_bytes() {
						buf.push(b as u16);
					}
				}
			}
		}

		i += 1;
	}

	buf
}

#[cfg(test)]
mod test {
    use super::parse_match_str;

	#[test]
	fn test_parse_match_str() {
		let test_str = "\\x7f\\0\\r\\t\\s\\n\\xy1\\x9aPK..🤩\\.";

		let expected: &'static [u16] = &[
			0x007f, 0x0000, b'\r' as u16, b'\t' as u16, b'\n' as u16, 0x009a, b'P' as u16, b'K' as u16, 0x8000, 0x8000, 0xf0, 0x9f, 0xa4, 0xa9, b'.' as u16
		];

		let computed = parse_match_str(test_str);

		assert_eq!(expected, computed);
	}
}