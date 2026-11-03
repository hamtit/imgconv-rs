#!/bin/sh
# Download the pinned C codec source into vendor/ (not committed);
# build.rs builds it with CMake and links it statically.
#
# - libaom 3.15.1: the release tarball from aomedia. Same version as the
#   libaom bundled with sharp 0.35.5, so AVIF output compares like for like.
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
