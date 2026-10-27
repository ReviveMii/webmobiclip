rm -rf target
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir ../pkg ./target/wasm32-unknown-unknown/release/mobiclip_wasm.wasm
