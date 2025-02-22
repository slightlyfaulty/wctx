use std::fs::{read_to_string, write};
use std::path::Path;
use better_minify_js::{Session, TopLevelMode, minify};

fn main() {
	let js_files = vec![
		"src/daemon/providers/assets/kwin/kwin.js",
	];

	for js_file in &js_files {
		println!("cargo:rerun-if-changed={}", js_file);
	}

	let session = Session::new();

	for js_file in js_files {
		let js = read_to_string(js_file).unwrap();
		let mut minified = Vec::new();

		minify(&session, TopLevelMode::Global, js.as_bytes(), &mut minified).unwrap();

		let output_path = Path::new(js_file).with_extension("min.js");
		write(output_path, minified).unwrap();
	}
}
