pub mod iter;
pub mod str_parse;

#[cfg(test)]
pub fn init_test_logger() {
	let _ = env_logger::builder().is_test(true).try_init();
}