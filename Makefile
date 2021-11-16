build:
	cargo build --release

cp-bin:
	sudo cp ./target/release/nemo-proxy /usr/local/bin/nemo

build-cp-bin:
	make build
	make cp-bin