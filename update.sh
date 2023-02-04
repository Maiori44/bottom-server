#!/usr/bin/bash

cd ~/bottom-server
git fetch && git pull && cargo install --path=.
