#!/bin/sh
# Builds the release binary and packages it two ways in dist/:
#   zquig_<version>-1_amd64.deb     for Debian, Ubuntu and derivatives (needs cargo-deb)
#   Zquig-linux-x86_64.tar.gz       the binary with install.sh, for any distribution
set -eu
cd "$(dirname "$0")"

rm -rf dist
cargo build --locked --release
cargo deb --no-build --locked --output dist/

stage=dist/Zquig
mkdir -p "$stage"
cp target/release/zquig install.sh README.md LICENSE SECURITY.md "$stage"
chmod 755 "$stage/zquig" "$stage/install.sh"
tar -czf dist/Zquig-linux-x86_64.tar.gz -C dist Zquig
rm -rf "$stage"
ls -l dist
