#!/bin/sh

export CARGO_HOME=$1/target/cargo-home

if [[ $DEBUG = true ]]
then
    echo "DEBUG MODE"
    cargo build --manifest-path $1/Cargo.toml -p gst-wpe-broadcast-demo && cp $1/target/debug/gst-wpe-broadcast-demo $2
else
    echo "RELEASE MODE"
    cargo build --manifest-path $1/Cargo.toml --release -p gst-wpe-broadcast-demo && cp $1/target/release/gst-wpe-broadcast-demo $2
fi
