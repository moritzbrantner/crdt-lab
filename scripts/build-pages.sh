#!/usr/bin/env bash
set -euo pipefail

WASM_BINDGEN_VERSION=0.2.100

rustup target add wasm32-unknown-unknown
if ! command -v wasm-bindgen >/dev/null 2>&1 || [[ "$(wasm-bindgen --version)" != "wasm-bindgen ${WASM_BINDGEN_VERSION}" ]]; then
  cargo install wasm-bindgen-cli --version "${WASM_BINDGEN_VERSION}" --locked
fi

cargo build --release --target wasm32-unknown-unknown -p crdt-lab-wasm
rm -rf dist
mkdir -p dist/pkg
wasm-bindgen \
  target/wasm32-unknown-unknown/release/crdt_lab_wasm.wasm \
  --out-dir dist/pkg \
  --target web \
  --no-typescript
cp site/index.html site/styles.css site/app.js dist/
