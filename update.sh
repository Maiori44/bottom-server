#!/usr/bin/bash

cd /home/felix/bottom-server
git fetch && git pull && sudo -u felix cargo install --path=.
