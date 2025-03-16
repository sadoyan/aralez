#!/bin/bash
export RUST_LOG=INFO
reflex -d none -r 'src/'  -s -- sh -c  'reset && cargo run -- --address 0.0.0.0 --port 6193'

