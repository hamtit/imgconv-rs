#!/bin/sh
# Download the pinned C/C++ codec sources into vendor/ (not committed);
# build.rs builds them with CMake and links them statically.
#
# - libaom 3.15.1: the release tarball from aomedia. Same version as the
#   libaom bundled with sharp 0.35.5, so AVIF output compares like for like.
# - libjxl 0.12.0: the upstream source tree (with its brotli / highway /
#   skcms third_party checkouts) as packaged in the jpegxl-src crate. Only
#   the libjxl/ directory (BSD-3-Clause) is extracted; nothing else from that
#   crate is used. Same version as the libjxl-native package.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)

fetch() { # name version url sha256 strip-components [subdir]
  dest="$ROOT/vendor/$1"
  if [ -f "$dest/.version" ] && [ "$(cat "$dest/.version")" = "$2" ]; then return; fi
  tmp=$(mktemp -d)
  curl -sSfL --retry 3 -o "$tmp/src.tar.gz" "$3"
  echo "$4  $tmp/src.tar.gz" | sha256sum -c - >/dev/null
  rm -rf "$dest"
  mkdir -p "$dest"
  if [ -n "${6:-}" ]; then
    tar -xzf "$tmp/src.tar.gz" -C "$tmp" "$6"
    cp -R "$tmp/$6/." "$dest/"
  else
    tar -xzf "$tmp/src.tar.gz" -C "$dest" --strip-components="$5"
  fi
  echo "$2" > "$dest/.version"
  rm -rf "$tmp"
}

fetch libaom 3.15.1 \
  https://storage.googleapis.com/aom-releases/libaom-3.15.1.tar.gz \
  8ca0c52746174603500f0adb6f2a215d69c9ca2aab2acb3caa06fb791d8d01bf 1
fetch libjxl 0.12.0 \
  https://static.crates.io/crates/jpegxl-src/jpegxl-src-0.12.0.crate \
  06f56a51e36f688517deebb0b7c10a11c598476936838938495c93764696180a 0 jpegxl-src-0.12.0/libjxl
