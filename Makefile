build:
	cargo build --release && cp target/release/raw-downloader .
clean:
	cargo clean
