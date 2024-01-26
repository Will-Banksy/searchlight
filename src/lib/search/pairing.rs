use std::collections::HashMap;

use crate::{lib::searchlight::config::{FileType, SearchlightConfig}, sl_warn};

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

#[derive(PartialEq, Clone, Copy)]
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

/// Process the list of found matches, identifying them with `id_ftype_map` (generated from `pairing::preprocess_config`)
/// and pairing headers up with footers (or, if no footer exists for that file type, returns a `MatchPair` for a range
/// max_len (as configured for the file type) from the start of the header)
///
/// Matches that were successfully paired or completed with max_len are removed from the input Vec
///
/// # Panics
/// Panics if a file type has both no footers and no max length (which would be a config validation error),
/// or if id_ftype_map is missing any match ids that are present in `matches`
pub fn pair<'a>(matches: &mut Vec<Match>, id_ftype_map: HashMap<u64, (usize, &'a FileType, MatchPart)>, config: &'a SearchlightConfig) -> Vec<MatchPair<'a>> {
	let mut complete_matches = Vec::new();
	let match_tracker: HashMap<usize, Vec<Match>> = HashMap::new();

	let mut i = 0;
	while i < matches.len() {
		let (ftype_idx, ftype, match_part) = *id_ftype_map.get(&matches[i].id).expect(&format!("Match id {} was not found in id_ftype_map", matches[i].id));

		if ftype.has_footer() && match_part == MatchPart::Header { // If the match file type has footers and this is a header...
			todo!()
		} else if match_part == MatchPart::Header { // If the match file type doesn't have footers and this is a header...
			// Very easy just complete this match with a length
			complete_matches.push(
				MatchPair::new_sized(
					ftype,
					matches.remove(i),
					ftype.max_len.expect(&format!("File type {} does not have either at least one footer or a max_len", ftype.extension.clone().unwrap_or("<no extension>".to_string())))
				)
			);
		} else { // If this is a footer...
			todo!()
		}

		i += 1;
	}

	complete_matches
}