#!/usr/bin/bash

cd /home/felix/bottom-server
git fetch && git pull && cargo install --path=.
