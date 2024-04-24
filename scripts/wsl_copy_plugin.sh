#!/bin/sh
set -e
export CARGO_BUILD_TARGET=x86_64-pc-windows-gnu
cargo build --release --lib
cp 'target/x86_64-pc-windows-gnu/release/w4on2_plugin.dll' '/mnt/c/Program Files/Common Files/VST3/w4on2_plugin.vst3'
