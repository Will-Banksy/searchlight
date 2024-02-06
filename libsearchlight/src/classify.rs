pub enum Classification {
	// Data that shows no discernable pattern may be classified as binary
	Binary,
	// Data that shows the pattern of being predominantly ASCII values may be classified as UTF-8
	Utf8Text,
	// Data that has an excess of '<' and '>' characters in roughly identical amounts may be classified as XML
	Xml,
}

// TODO: Classification algorithms... Split classification into generic classification and specialised classification? What to do in the case of not processing by chunks too?