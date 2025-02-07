#!/bin/bash
export RUST_LOG=INFO
reflex -d none -r 'src/'  -s -- sh -c  'reset && cargo run '

