use core::fmt;
use std::collections::HashMap;

use log::warn;

use crate::searchlight::config::{FileType, PairingStrategy, SearchlightConfig};

use super::{Match, match_id_hash_slice};

#[derive(PartialEq)]
pub struct MatchPair<'a> {
	pub file_type: &'a FileType,
	pub start_idx: u64,
	pub end_idx: u64
}

impl fmt::Debug for MatchPair<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("MatchPair")/*.field("file_type", &self.file_type)*/.field("start_idx", &self.start_idx).field("end_idx", &self.end_idx).finish()
	}
}

impl<'a> MatchPair<'a> {
	pub fn new(file_type: &'a FileType, start: &Match, end: &Match) -> Self {
		MatchPair {
			file_type,
			start_idx: start.start_idx,
			end_idx: end.end_idx
		}
	}

	pub fn new_sized(file_type: &'a FileType, start: &Match, size: u64) -> Self {
		MatchPair {
			file_type,
			start_idx: start.start_idx,
			end_idx: start.start_idx + size
		}
	}
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MatchPart {
	Header,
	Footer
}

/// Processes the configured file types in `config` to produce a mapping from match ids to file types (preceded by the index of the file type into config) and match parts
pub fn preprocess_config<'a>(config: &'a SearchlightConfig) -> HashMap<u64, (usize, &'a FileType, MatchPart)> {
	let mut id_ftype_map: HashMap<u64, (usize, &'a FileType, MatchPart)> = HashMap::new();

	// Process the config to produce a mapping from match ids to indices of filetypes, with whether the match id corresponds to a header or footer
	for i in 0..(config.file_types.len()) {
		for header in &config.file_types[i].headers {
			let id = match_id_hash_slice(&header);
			if id_ftype_map.contains_key(&id) {
				warn!(
					"Collision detected, matches of this byte sequence may be misattributed (header: {:?} in type {}) - All byte sequences used in headers and footers should be unique",
					header,
					config.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
				);
			}
			id_ftype_map.insert(id, (i, &config.file_types[i], MatchPart::Header));
		}
		for footer in &config.file_types[i].footers {
			let id = match_id_hash_slice(&footer);
			if id_ftype_map.contains_key(&id) {
				warn!(
					"Collision detected, matches of this byte sequence may be misattributed (footer: {:?} in type {}) - All byte sequences used in headers and footers should be unique",
					footer,
					config.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
				);
			}
			id_ftype_map.insert(id, (i, &config.file_types[i], MatchPart::Footer));
		}
	}

	id_ftype_map
}

fn in_range(header: &Match, footer: &Match, max_size: Option<u64>) -> bool {
	assert!(footer.end_idx > header.start_idx);
	if (footer.end_idx - header.start_idx) <= max_size.unwrap_or(u64::MAX) {
		true
	} else {
		false
	}
}

/// Process the list of found matches (which should be sorted by match start index), identifying them with `id_ftype_map` (generated from `pairing::preprocess_config`)
/// and pairing headers up with footers (or, if no footer exists for that file type, returns a `MatchPair` for a range
/// max_len (as configured for the file type) from the start of the header).
///
/// Matches that were successfully paired or completed with max_len are removed from the input Vec.
///
/// # Panics
/// Panics if a file type has both no footers and no max length (which would be a config validation error),
/// or if id_ftype_map is missing any match ids that are present in `matches`.
pub fn pair<'a>(matches: &mut Vec<Match>, id_ftype_map: &HashMap<u64, (usize, &'a FileType, MatchPart)>, end_of_matches: bool) -> Vec<MatchPair<'a>> {
	// TODO: Maybe add a config that changes how this function works to allow the configurability of scalpel - Currently all we're missing is excluding the footer bytes and allowing duplicate footer/headers
	//       e.g. if we have 2 identical ids, the id_ftype_list will only contain an entry for 1 of the headers/footers that have that id... This may be difficult to allow with current design, all we know
	//       about a match is it's id, and if a match maps to multiple different headers/footers that's difficult to handle - though maybe not impossible... But would it make sense? Tbh, I could maybe change
	//       it so that each header/footer has a unique id associated with it... but that doesn't solve the problem as then you just end up with a sequence of bytes potentially mapping to multiple unique ids
	// NOTE: Do we want to solve [ H0, H1, F0, F1 ] (all of the same file type) as [ H0F1, H1F0 ] (current) or [ H0F0, H1F1 ]? The current case is perhaps better tackled with PairLast, however it's also a more
	//       likely way to reconstruct correctly? Files aren't often interleaved like that - but byte sequences that dont't correspond to real headers/footers might cause such situations, and fragmentation is
	//       always something to consider

	let mut complete_matches = Vec::new();
	// Map from FileType idx to list of Match idxs that are of that filetype. This list is referred to as a match stack for reasons although not being an actual stack
	let mut match_tracker: HashMap<usize, Vec<usize>> = HashMap::new();
	let mut matches_to_remove = Vec::new();

	for match_idx in 0..matches.len() {
		let (ftype_idx, ftype, match_part) = *id_ftype_map.get(&matches[match_idx].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[match_idx].id));

		if ftype.has_footer() && match_part == MatchPart::Header { // If the match file type has footers and this is a header...
			// Push the index of the match to the match tracker at the file type index
			if let Some(match_idxs) = match_tracker.get_mut(&ftype_idx) {
				match_idxs.push(match_idx);
			} else {
				match_tracker.insert(ftype_idx, vec![match_idx]);
			}
		} else if match_part == MatchPart::Header { // If the match file type doesn't have footers and this is a header...
			// Very easy just complete this match with a length
			complete_matches.push(
				MatchPair::new_sized(
					ftype,
					&matches[match_idx],
					ftype.max_len.expect(&format!("File type {} does not have either at least one footer or a max_len", ftype.extension.clone().unwrap_or("<no extension>".to_string())))
				)
			);

			// And mark this match for removal
			matches_to_remove.push(match_idx);
		} else { // If this is a footer...
			if ftype.pairing == PairingStrategy::PairNext {
				if let Some(match_stack) = match_tracker.get_mut(&ftype_idx) {
					let mut pair_match_idx = None;
					if let Some(mi) = match_stack.pop() { // If the top match index is for a file type that uses the PairNext pairing strategy, then match with that
						let (_, mi_ftype, mi_match_part) = id_ftype_map.get(&matches[mi].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[mi].id));
						assert_eq!(*mi_match_part, MatchPart::Header);
						assert_eq!(mi_ftype.pairing, ftype.pairing);

						pair_match_idx = Some(mi);
					}

					if let Some(pair_match_idx) = pair_match_idx {
						// We only want to complete the match if the header and footer are in range of each other,
						// but we want to remove those matches in either case cause they won't match anyway (probably...?)
						if in_range(&matches[pair_match_idx], &matches[match_idx], ftype.max_len) {
							complete_matches.push(
								MatchPair::new(
									ftype,
									&matches[pair_match_idx],
									&matches[match_idx]
								)
							);
						}
						matches_to_remove.push(pair_match_idx);
						matches_to_remove.push(match_idx);
					} else { // If there are no headers that occurred before this footer, or were otherwise paired with different footers...
						matches_to_remove.push(match_idx); // Then simply remove this match
					}
				} else { // If there are no headers that occurred before this footer, or were otherwise paired with different footers...
					matches_to_remove.push(match_idx); // Then simply remove this match
				}
			} else { // PairLast
				// Whether this current footer should be pushed to the match tracker or not. Also used to determine whether this match should be
				// marked for removal or not
				let mut add_footer = true;
				if let Some(match_stack) = match_tracker.get_mut(&ftype_idx) {
					// If there is a previous footer, and that is within bounds of the max size for the file type and this footer is not, then that previous footer is the last one so
					// complete the match with that one and disregard this footer
					if let Some((header_idx, &header_match_idx)) = match_stack.iter().enumerate().rfind(|&(_, &e)| id_ftype_map.get(&matches[e].id).unwrap().2 == MatchPart::Header) {
						if let Some(&mi) = match_stack.get(match_stack.len() - 1) {
							if mi != header_match_idx && in_range(&matches[header_match_idx], &matches[mi], ftype.max_len) && !in_range(&matches[header_match_idx], &matches[match_idx], ftype.max_len) {
								complete_matches.push(
									MatchPair::new(
										ftype,
										&matches[header_match_idx],
										&matches[mi]
									)
								);
								add_footer = false;
								match_stack.remove(match_stack.len() - 1);
								match_stack.remove(header_idx);
								matches_to_remove.push(mi);
								matches_to_remove.push(header_match_idx);
							}
						}
					}

					if add_footer {
						match_stack.push(match_idx);
						// add_footer = false;
					}
				}

				// if add_footer {
				// 	matches_to_remove.push(match_idx);
				// }
			}
		}
	}

	// Process any remaining matches in the match stacks
	for (_, match_stack) in match_tracker.iter_mut() {
		let mut i = 0;
		while i < match_stack.len() {
			let mut increment = true;

			let match_idx = match_stack[i];
			let (_, ftype, match_part) = *id_ftype_map.get(&matches[match_idx].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[match_idx].id));

			if ftype.pairing == PairingStrategy::PairNext {
				assert_eq!(match_part, MatchPart::Header);
				// If the current match part is a header, then if there is a currently-tracked header
				// that doesn't require a footer, complete it with the file type's max size. If it
				// does require a footer, then ignore and remove it
				if !ftype.requires_footer {
					complete_matches.push(MatchPair::new_sized(
						&ftype,
						&matches[match_idx],
						ftype.max_len.expect(&format!("File type {} does not have either at least one footer or a max_len", ftype.extension.clone().unwrap_or("<no extension>".to_string())))
					));
				}
				matches_to_remove.push(match_idx);
			} else { // PairLast
				if match_part == MatchPart::Header {
					let mut pair_idx: Option<usize> = None;
					let mut left_range = false;
					if (i + 1) < match_stack.len() {
						for j in (i + 1)..match_stack.len() {
							let (_, _, j_match_part) = *id_ftype_map.get(&matches[match_stack[j]].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[match_stack[j]].id));
							if j_match_part == MatchPart::Footer && in_range(&matches[match_idx], &matches[match_stack[j]], ftype.max_len) {
								pair_idx = Some(j);
							} else if /*j_match_part == MatchPart::Footer && */!in_range(&matches[match_idx], &matches[match_stack[j]], ftype.max_len) {
								left_range = true;
							}
						}
					}

					if left_range || end_of_matches {
						if let Some(pair_idx) = pair_idx {
							complete_matches.push(
								MatchPair::new(
									&ftype,
									&matches[match_idx],
									&matches[match_stack[pair_idx]]
								)
							);
							matches_to_remove.push(match_idx);
							matches_to_remove.push(match_stack[pair_idx]);
							match_stack.remove(pair_idx);
							match_stack.remove(i);
							increment = false;
						} else if end_of_matches && !ftype.requires_footer {
							if let Some(max_len) = ftype.max_len {
								complete_matches.push(
									MatchPair::new_sized(
										&ftype,
										&matches[match_idx],
										max_len
									)
								);
							}
							matches_to_remove.push(match_idx);
						} else if ftype.requires_footer && left_range {
							matches_to_remove.push(match_idx);
							match_stack.remove(i);
							increment = false;
						}
					}
				} else { // Footer
					// Check if there's any headers that precede this footer. If not, then remove this footer
					if !match_stack.iter().take(i).any(|&mi| id_ftype_map.get(&matches[mi].id).unwrap().2 == MatchPart::Header) {
						matches_to_remove.push(match_idx);
						match_stack.remove(i);
						increment = false;
					}
				}
			}

			if increment {
				i += 1;
			}
		}
	}

	matches_to_remove.sort();
	matches_to_remove.dedup();

	for &rem_idx in matches_to_remove.iter().rev() {
		matches.remove(rem_idx);
	}

	complete_matches
}

#[cfg(test)]
mod test {
    use crate::{search::{match_id_hash_slice, pairing::MatchPair, Match}, searchlight::config::{FileType, PairingStrategy, SearchlightConfig}};

    use super::{pair, preprocess_config};

	#[test]
	fn test_pairing() {
		let match_ids: &[u64] = &[
			match_id_hash_slice("ft0_header".as_bytes()),
			match_id_hash_slice("ft0_footer".as_bytes()),
			match_id_hash_slice("ft1_header".as_bytes()),
			match_id_hash_slice("ft1_footer".as_bytes()),
			match_id_hash_slice("ft2_header".as_bytes()),
			match_id_hash_slice("ft2_footer".as_bytes()),
			match_id_hash_slice("ft3_header".as_bytes()),
			match_id_hash_slice("ft3_footer".as_bytes()),
			match_id_hash_slice("ft4_header".as_bytes()),
			match_id_hash_slice("ft4_footer".as_bytes()),
			match_id_hash_slice("ft5_header".as_bytes()),
			match_id_hash_slice("ft5_footer".as_bytes()),
		];

		let mut match_lists = vec![
			vec![
				// Case - Simple PairNext
				Match {
					id: match_ids[0],
					start_idx: 0,
					end_idx: 3
				},
				Match {
					id: match_ids[1],
					start_idx: 6,
					end_idx: 7
				},

				// Case - Interleaved PairNext matches of different file types
				Match {
					id: match_ids[0],
					start_idx: 10,
					end_idx: 15
				},
				Match {
					id: match_ids[2],
					start_idx: 13,
					end_idx: 16
				},
				Match {
					id: match_ids[1],
					start_idx: 18,
					end_idx: 20
				},
				Match {
					id: match_ids[3],
					start_idx: 19,
					end_idx: 23
				},

				// Case - Interleaved PairNext matches of the same file type
				Match {
					id: match_ids[0],
					start_idx: 27,
					end_idx: 29
				},
				Match {
					id: match_ids[0],
					start_idx: 30,
					end_idx: 32
				},
				Match {
					id: match_ids[1],
					start_idx: 33,
					end_idx: 34
				},
				Match {
					id: match_ids[1],
					start_idx: 35,
					end_idx: 37
				},

				// Case - Simple PairLast
				Match {
					id: match_ids[4],
					start_idx: 45,
					end_idx: 47
				},
				Match {
					id: match_ids[5],
					start_idx: 49,
					end_idx: 52
				},

				// Case - Interleaved PairLast matches of different file types
				Match { // idx 12
					id: match_ids[4],
					start_idx: 57,
					end_idx: 59
				},
				Match {
					id: match_ids[6],
					start_idx: 60,
					end_idx: 62
				},
				Match {
					id: match_ids[5],
					start_idx: 64,
					end_idx: 66
				},
				Match { // idx 15
					id: match_ids[7],
					start_idx: 67,
					end_idx: 69
				},

				// Case - Interleaved PairLast matches of the same file type
				Match { // idx 16
					id: match_ids[6],
					start_idx: 70,
					end_idx: 72
				},
				Match {
					id: match_ids[6],
					start_idx: 73,
					end_idx: 76
				},
				Match {
					id: match_ids[7],
					start_idx: 77,
					end_idx: 78
				},
				Match { // idx 19
					id: match_ids[7],
					start_idx: 79,
					end_idx: 81
				},

				// Case - Simple PairNext (out of bounds)
				Match { // idx 20
					id: match_ids[0],
					start_idx: 83,
					end_idx: 85
				},
				Match { // idx 21
					id: match_ids[1],
					start_idx: 91,
					end_idx: 94
				},

				// Case - Simple PairLast (out of bounds)
				Match { // idx 22
					id: match_ids[6],
					start_idx: 95,
					end_idx: 99
				},
				Match { // idx 23
					id: match_ids[7],
					start_idx: 108,
					end_idx: 112
				},

				// Case - PairNext with two candidates
				Match { // idx 24
					id: match_ids[0],
					start_idx: 115,
					end_idx: 117
				},
				Match {
					id: match_ids[1],
					start_idx: 119,
					end_idx: 120
				},
				Match { // idx 25
					id: match_ids[1],
					start_idx: 122,
					end_idx: 124
				},

				// Case - PairLast with two candidates
				Match { // idx 26
					id: match_ids[4],
					start_idx: 125,
					end_idx: 128
				},
				Match {
					id: match_ids[5],
					start_idx: 129,
					end_idx: 131
				},
				Match { // idx 27
					id: match_ids[5],
					start_idx: 132,
					end_idx: 134
				},
			],

			// BREAKPOINT

			vec![
				// Case - Single PairNext that doesn't require a footer
				Match { // 28
					id: match_ids[8],
					start_idx: 140,
					end_idx: 144
				},

				// Case - Single PairLast that doesn't require a footer
				Match { // 28
					id: match_ids[10],
					start_idx: 148,
					end_idx: 152
				},
			]
		];

		let config = SearchlightConfig {
			file_types: vec![
				FileType {
					headers: vec![ "ft0_header".bytes().collect() ],
					footers: vec![ "ft0_footer".bytes().collect() ],
					extension: Some("ft0".to_string()),
					pairing: PairingStrategy::PairNext,
					max_len: Some(10),
					requires_footer: true,
					..Default::default()
				},
				FileType {
					headers: vec![ "ft1_header".bytes().collect() ],
					footers: vec![ "ft1_footer".bytes().collect() ],
					extension: Some("ft1".to_string()),
					pairing: PairingStrategy::PairNext,
					max_len: Some(10),
					requires_footer: true,
					..Default::default()
				},
				FileType {
					headers: vec![ "ft2_header".bytes().collect() ],
					footers: vec![ "ft2_footer".bytes().collect() ],
					extension: Some("ft2".to_string()),
					pairing: PairingStrategy::PairLast,
					max_len: Some(10),
					requires_footer: true,
					..Default::default()
				},
				FileType {
					headers: vec![ "ft3_header".bytes().collect() ],
					footers: vec![ "ft3_footer".bytes().collect() ],
					extension: Some("ft3".to_string()),
					pairing: PairingStrategy::PairLast,
					max_len: Some(11),
					requires_footer: true,
					..Default::default()
				},
				FileType {
					headers: vec![ "ft4_header".bytes().collect() ],
					footers: vec![ "ft4_footer".bytes().collect() ],
					extension: Some("ft4".to_string()),
					pairing: PairingStrategy::PairNext,
					max_len: Some(10),
					requires_footer: false,
					..Default::default()
				},
				FileType {
					headers: vec![ "ft5_header".bytes().collect() ],
					footers: vec![ "ft5_footer".bytes().collect() ],
					extension: Some("ft5".to_string()),
					pairing: PairingStrategy::PairLast,
					max_len: Some(10),
					requires_footer: false,
					..Default::default()
				},
			],
			..Default::default()
		};

		config.validate().unwrap();

		let expected_pairs = [
			MatchPair {
				file_type: &config.file_types[0],
				start_idx: 0,
				end_idx: 7,
			},
			MatchPair {
				file_type: &config.file_types[0],
				start_idx: 10,
				end_idx: 20,
			},
			MatchPair {
				file_type: &config.file_types[1],
				start_idx: 13,
				end_idx: 23,
			},
			MatchPair {
				file_type: &config.file_types[0],
				start_idx: 27,
				end_idx: 37,
			},
			MatchPair {
				file_type: &config.file_types[0],
				start_idx: 30,
				end_idx: 34,
			},
			MatchPair {
				file_type: &config.file_types[2],
				start_idx: 45,
				end_idx: 52,
			},
			MatchPair {
				file_type: &config.file_types[2],
				start_idx: 57,
				end_idx: 66,
			},
			MatchPair {
				file_type: &config.file_types[3],
				start_idx: 60,
				end_idx: 69,
			},
			MatchPair {
				file_type: &config.file_types[3],
				start_idx: 70,
				end_idx: 81,
			},
			MatchPair {
				file_type: &config.file_types[3],
				start_idx: 73,
				end_idx: 78,
			},
			MatchPair {
				file_type: &config.file_types[0],
				start_idx: 115,
				end_idx: 120,
			},
			MatchPair {
				file_type: &config.file_types[2],
				start_idx: 125,
				end_idx: 134,
			},
			MatchPair {
				file_type: &config.file_types[4],
				start_idx: 140,
				end_idx: 150,
			},
			MatchPair {
				file_type: &config.file_types[5],
				start_idx: 148,
				end_idx: 158,
			},
		];

		let id_ftype_map = preprocess_config(&config);

		println!("matches (before): {:?}\n", match_lists);

		let mut match_list = match_lists[0].clone();

		let mut match_pairs = pair(&mut match_list, &id_ftype_map, false);

		match_list.append(&mut match_lists[1]);

		match_pairs.append(&mut pair(&mut match_list, &id_ftype_map, true));

		match_pairs.sort_by_key(|e| e.start_idx);

		println!("matches (after): {:?}\n", match_list);
		println!("match pairs: {:?}", match_pairs);

		assert_eq!(match_pairs, expected_pairs);
		assert!(match_list.is_empty());
	}
}