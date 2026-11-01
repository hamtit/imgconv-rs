#!/bin/sh
# Copy the license files of everything linked into the addon (Rust crates,
# including the C libraries they bundle, and libaom) into licenses/.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
rm -rf licenses
mkdir -p licenses
cargo metadata --format-version 1 --filter-platform "$(rustc -vV | sed -n 's/^host: //p')" \
  | python3 -c '
import json, sys
m = json.load(sys.stdin)
pkgs = {p["id"]: p for p in m["packages"]}
# Normal (non-build, non-dev) dependencies reachable from the root package.
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}
seen, todo = set(), [m["resolve"]["root"]]
while todo:
    i = todo.pop()
    if i in seen: continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] is None for k in d["dep_kinds"]): todo.append(d["pkg"])
for i in sorted(seen):
    p = pkgs[i]
    if i == m["resolve"]["root"]: continue
    print(p["name"], p["version"], p["license"] or "", p["manifest_path"].rsplit("/", 1)[0], sep="\t")
' > licenses/crates.tsv
while IFS="$(printf '\t')" read -r name version license dir; do
  dest="licenses/$name-$version"
  mkdir -p "$dest"
  find "$dir" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' -o -iname 'PATENTS*' \) -exec cp {} "$dest/" \;
  # -sys crates bundle C sources with their own license files.
  for vendored in "$dir"/vendor "$dir"/libjxl "$dir"/libjpeg-turbo; do
    [ -d "$vendored" ] && find "$vendored" -maxdepth 2 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'PATENTS*' -o -iname 'README.ijg' \) \
      -exec sh -c 'cp "$1" "$2/vendor-$(basename "$(dirname "$1")")-$(basename "$1")"' _ {} "$dest" \;
  done
  printf '%s %s: %s\n' "$name" "$version" "$license" >> licenses/SUMMARY.txt
done < licenses/crates.tsv
rm licenses/crates.tsv
mkdir -p licenses/libaom-$(cat vendor/libaom/.version)
cp vendor/libaom/LICENSE vendor/libaom/PATENTS licenses/libaom-$(cat vendor/libaom/.version)/
echo "libaom $(cat vendor/libaom/.version): BSD-2-Clause + AOMedia Patent License 1.0" >> licenses/SUMMARY.txt
# libjxl and the third_party libraries it links statically (Highway, Brotli, skcms).
jxl="licenses/libjxl-$(cat vendor/libjxl/.version)"
mkdir -p "$jxl"
cp vendor/libjxl/LICENSE vendor/libjxl/PATENTS "$jxl/"
for dep in highway brotli skcms; do
  cp vendor/libjxl/third_party/$dep/LICENSE "$jxl/$dep-LICENSE"
done
echo "libjxl $(cat vendor/libjxl/.version): BSD-3-Clause + patent grant (Highway: Apache-2.0 OR BSD-3-Clause, Brotli: MIT, skcms: BSD-3-Clause)" >> licenses/SUMMARY.txt
echo "collected $(ls licenses | wc -l) entries"
