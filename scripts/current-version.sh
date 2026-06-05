#!/bin/sh
set -eu

awk '
  $1 == "version" && $2 == "=" {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' Cargo.toml
