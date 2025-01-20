#!/bin/bash

reflex -d none -r 'src/'  -s -- sh -c  'reset && cargo run livelong poem.txt'

