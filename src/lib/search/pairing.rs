use std::collections::HashMap;

use crate::{lib::searchlight::config::{FileType, PairingStrategy, SearchlightConfig}, sl_warn};

use super::{Match, match_id_hash_slice};

pub struct MatchPair<'a> {
	file_type: &'a FileType,
	start_idx: u64,
	end_idx: u64
}

impl<'a> MatchPair<'a> {
	pub fn new(file_type: &'a FileType, start: Match, end: Match) -> Self {
		MatchPair {
			file_type,
			start_idx: start.start_idx,
			end_idx: end.end_idx
		}
	}

	pub fn new_sized(file_type: &'a FileType, start: Match, size: u64) -> Self {
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
			if !config.quiet && id_ftype_map.contains_key(&id) {
				sl_warn!(
					"pairing",
					format!("Collision detected, matches of this byte sequence may be misattributed (header: {:?} in type {}) - All byte sequences used in headers and footers should be unique",
						header, config.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
					)
				);
			}
			id_ftype_map.insert(id, (i, &config.file_types[i], MatchPart::Header));
		}
		for footer in &config.file_types[i].footers {
			let id = match_id_hash_slice(&footer);
			if !config.quiet && id_ftype_map.contains_key(&id) {
				sl_warn!(
					"pairing",
					format!("Collision detected, matches of this byte sequence may be misattributed (footer: {:?} in type {}) - All byte sequences used in headers and footers should be unique",
						footer, config.file_types[i].extension.clone().unwrap_or("<no extension>".to_string())
					)
				);
			}
			id_ftype_map.insert(id, (i, &config.file_types[i], MatchPart::Footer));
		}
	}

	id_ftype_map
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
pub fn pair<'a>(matches: &mut Vec<Match>, id_ftype_map: HashMap<u64, (usize, &'a FileType, MatchPart)>, end_of_matches: bool) -> Vec<MatchPair<'a>> {
	let mut complete_matches = Vec::new();
	// Map from FileType idx to list of Match idxs that are of that filetype and use PairingStrategy::PairNext
	let mut pn_match_tracker: HashMap<usize, Vec<usize>> = HashMap::new();
	let mut matches_to_remove = Vec::new();
	// Map from FileType idx to list of Match idxs that are of that filetype and use PairingStrategy::PairLast // TODO: Can the two trackers be unified in a nice way?
	let mut pl_match_tracker: HashMap<usize, Vec<usize>> = HashMap::new();

	let mut match_idx = 0;
	while match_idx < matches.len() {
		let (ftype_idx, ftype, match_part) = *id_ftype_map.get(&matches[match_idx].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[match_idx].id));

		if ftype.has_footer() && match_part == MatchPart::Header { // If the match file type has footers and this is a header...
			if ftype.pairing == PairingStrategy::PairNext {
				// Push the index of the match to the match tracker at the file type index
				if let Some(match_idxs) = pn_match_tracker.get_mut(&ftype_idx) {
					match_idxs.push(match_idx);
				} else {
					pn_match_tracker.insert(ftype_idx, vec![match_idx]);
				}
			} else {
				// Push the index of the match to the match tracker at the file type index
				if let Some(match_idxs) = pl_match_tracker.get_mut(&ftype_idx) {
					match_idxs.push(match_idx);
				} else {
					pl_match_tracker.insert(ftype_idx, vec![match_idx]);
				}
			}
		} else if match_part == MatchPart::Header { // If the match file type doesn't have footers and this is a header...
			// Very easy just complete this match with a length
			complete_matches.push(
				MatchPair::new_sized(
					ftype,
					matches[match_idx].clone(),
					ftype.max_len.expect(&format!("File type {} does not have either at least one footer or a max_len", ftype.extension.clone().unwrap_or("<no extension>".to_string())))
				)
			);
			// And mark this match for removal
			matches_to_remove.push(match_idx);
		} else { // If this is a footer...
			if ftype.pairing == PairingStrategy::PairNext {
				if let Some(match_stack) = pn_match_tracker.get_mut(&ftype_idx) {
					let mut pair_match_idx = None;
					if let Some(mi) = match_stack.pop() { // If the top match index is for a file type that uses the PairNext pairing strategy, then match with that
						let (_, mi_ftype, mi_match_part) = id_ftype_map.get(&matches[mi].id).unwrap();
						assert_eq!(*mi_match_part, MatchPart::Header);
						assert_eq!(mi_ftype.pairing, ftype.pairing);

						if mi_ftype.pairing == PairingStrategy::PairNext {
							pair_match_idx = Some(mi);
						} else {
							unimplemented!()
						}
					}

					if let Some(pair_match_idx) = pair_match_idx {
						matches_to_remove.push(pair_match_idx);
						matches_to_remove.push(match_idx);
						complete_matches.push(
							MatchPair::new(
								ftype,
								matches[pair_match_idx].clone(),
								matches[match_idx].clone()
							)
						)
					} else { // If there are no headers that occurred before this footer, or were otherwise paired with different footers...
						matches_to_remove.push(match_idx); // Then simply remove this match
					}
				} else { // If there are no headers that occurred before this footer, or were otherwise paired with different footers...
					matches_to_remove.push(match_idx); // Then simply remove this match
				}
			} else { // PairLast
				let mut add_footer = true;
				if let Some(match_stack) = pl_match_tracker.get_mut(&ftype_idx) {
					if let Some((header_idx, &header_match_idx)) = match_stack.iter().enumerate().rfind(|&(_, &e)| id_ftype_map.get(&matches[e].id).unwrap().2 == MatchPart::Header) {
						if let Some(&mi) = match_stack.get(match_stack.len() - 1) {
							if let Some(max_len) = ftype.max_len {
								if mi != header_match_idx && ((matches[mi].end_idx - matches[header_match_idx].start_idx) < max_len) && ((matches[match_idx].end_idx - matches[header_match_idx].start_idx) > max_len) {
									match_stack.remove(header_idx);
									complete_matches.push(
										MatchPair::new(
											ftype,
											matches[header_match_idx].clone(),
											matches[mi].clone()
										)
									);
									add_footer = false;
									matches_to_remove.push(header_match_idx);
								}
							}
						}
					}

					if add_footer {
						match_stack.push(match_idx);
						add_footer = false;
					}
				}

				if add_footer {
					matches_to_remove.push(match_idx);
				}
			}
		}

		match_idx += 1;
	}

	// TODO: Iterate through each match stack in each match tracker and complete any remaining matches, if possible
	//       For the PairNext ones, simply look and see if there are any footers for the headers
	//       For the PairLast ones, look for the last footer before another header to pair with any headers
	//       Also maybe get rid of some of the confusing logic in the footer handling code, to replace with hopefully simpler code here - some code should run when it's not the end of the matches yet, some should not

	// TODO: Remove all matches that were marked for removal, in reverse order to not fuck up the indices

	complete_matches
}