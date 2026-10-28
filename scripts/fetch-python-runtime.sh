#!/usr/bin/env bash
# Downloads the relocatable CPython that the packaged app runs box scripts with into
# app/src-tauri/python-runtime/ (bundled as a resource, see tauri.conf.json).
# Usage: scripts/fetch-python-runtime.sh [target-triple]   (default: $TAURI_ENV_TARGET_TRIPLE, else this machine)
# Needs curl, tar and sha256sum or shasum; on macOS also codesign and security when APPLE_SIGNING_IDENTITY is set.
set -euo pipefail

# python-build-standalone "install_only_stripped" builds.
release=20261013
version=3.12.15

host_triple() {
  local arch os
  case "$(uname -m)" in arm64 | aarch64) arch=aarch64 ;; *) arch=x86_64 ;; esac
  case "$(uname -s)" in Darwin) os=apple-darwin ;; Linux) os=unknown-linux-gnu ;; *) os=pc-windows-msvc ;; esac
  echo "$arch-$os"
}
triple="${1:-${TAURI_ENV_TARGET_TRIPLE:-$(host_triple)}}"
case "$triple" in
  aarch64-apple-darwin) sha=9866e11cf2ae9caa1e671383786f248859a52dd8bfb702fb4c0dc2c65f1d36de ;;
  x86_64-apple-darwin) sha=d09998f85752b646873a8a6bc7eea442a0d251b1c8d2413aa8ea06ef131c3335 ;;
  x86_64-pc-windows-msvc) sha=7a2aef48e5ee569e310b29b32681debbc5d888e92790d85331a47736c6b06aad ;;
  aarch64-pc-windows-msvc) sha=864342350eec45119bf295d7af7d60a36842e1c6d181cce760fcc6f60e5a072a ;;
  x86_64-unknown-linux-gnu) sha=178d3d83b6e8e99285a65d3fce9c16b87952cc5efdb4d1a0e3ff3be18ff0fc9c ;;
  aarch64-unknown-linux-gnu) sha=9467a99bacc7d90b64b02de7fa9ecfc99b72905e64e74f7ff6b460ad95cfed5a ;;
  *) echo "fetch-python-runtime: no pinned CPython build for $triple" >&2; exit 1 ;;
esac

name="cpython-$version+$release-$triple-install_only_stripped.tar.gz"
dest="$(cd "$(dirname "$0")/.." && pwd)/app/src-tauri/python-runtime"
stamp="$dest/.choren-python"

if [[ "$(cat "$stamp" 2>/dev/null)" != "$name" ]]; then
  echo "fetch-python-runtime: downloading $name"
  tmp="$(mktemp -d "$(dirname "$dest")/.python-runtime-XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT
  curl -fsSL -o "$tmp/python.tar.gz" "https://github.com/astral-sh/python-build-standalone/releases/download/$release/${name//+/%2B}"
  # macOS has shasum; Git's sh on Windows may lack it (Perl's core_perl is off its PATH) but has sha256sum.
  if command -v sha256sum >/dev/null; then check=(sha256sum -c -); else check=(shasum -a 256 -c -); fi
  echo "$sha  $tmp/python.tar.gz" | "${check[@]}" >/dev/null
  tar -xzf "$tmp/python.tar.gz" -C "$tmp"

  # Not needed by the box host (standard library only) or by box scripts: Tk, tests, pip, headers.
  # macOS's bin/python3.12 is statically linked, so libpython3.12.dylib (for embedding) is unused too,
  # and without _dbm and _crypt (dbm falls back to dbm.dumb; crypt is gone in 3.13) it is the only
  # Mach-O left to sign. bin/python and bin/python3 link to python3.12; Tauri would bundle full copies.
  (
    cd "$tmp/python"
    shopt -s nullglob
    rm -rf include share libs Scripts tcl DLLs/_tkinter* DLLs/tcl* DLLs/tk* \
      bin/idle* bin/pip* bin/pydoc* bin/2to3* bin/*-config bin/python bin/python3
    # Windows paths ignore case, so there these would match the standard library in Lib/ (threading.py).
    if [[ "$triple" != *-windows-* ]]; then
      rm -rf lib/pkgconfig lib/libpython3.12.dylib lib/*tcl* lib/*tk* lib/itcl* lib/thread* lib/python3.12/lib-dynload/_{tkinter,dbm,crypt}*
    fi
    for lib in lib/python3.12 Lib; do
      for p in test idlelib tkinter turtledemo turtle.py ensurepip lib2to3 pydoc_data venv 'config-3.12*' 'site-packages/pip*'; do
        rm -rf "${lib:?}"/$p
      done
    done
  )
  echo "$name" > "$tmp/python/.choren-python"
  rm -rf "$dest"
  mv "$tmp/python" "$dest"
  echo "fetch-python-runtime: $triple CPython $version in $dest"
fi

# Tauri signs only the app's own binaries, not Mach-O files among its resources, and notarization
# rejects any that lack a Developer ID signature with the hardened runtime and a timestamp. When
# this runs before Tauri has imported APPLE_CERTIFICATE (CI), import it into a keychain first.
identity="${APPLE_SIGNING_IDENTITY:-}"
keychain=()
if [[ -n "$identity" && "$triple" == *-apple-darwin ]]; then
  if [[ "$identity" != "-" && -n "${APPLE_CERTIFICATE:-}" ]] && ! security find-identity -v -p codesigning | grep -qF "$identity"; then
    keychain=(--keychain "$(mktemp -d)/choren-python.keychain-db")
    password="$(uuidgen)"
    security create-keychain -p "$password" "${keychain[1]}"
    security set-keychain-settings -lut 3600 "${keychain[1]}"
    security unlock-keychain -p "$password" "${keychain[1]}"
    echo "$APPLE_CERTIFICATE" | base64 --decode > "${keychain[1]}.p12"
    security import "${keychain[1]}.p12" -k "${keychain[1]}" -P "${APPLE_CERTIFICATE_PASSWORD:-}" -T /usr/bin/codesign >/dev/null
    rm -f "${keychain[1]}.p12"
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "${keychain[1]}" >/dev/null
    # shellcheck disable=SC2046
    security list-keychains -d user -s "${keychain[1]}" $(security list-keychains -d user | tr -d '"')
  fi
  timestamp=--timestamp
  [[ "$identity" == "-" ]] && timestamp=
  find "$dest" -type f \( -perm -u+x -o -name '*.so' -o -name '*.dylib' \) -print0 |
    while IFS= read -r -d '' f; do
      if file -b "$f" | grep -q Mach-O; then
        codesign --force --options runtime $timestamp ${keychain[@]+"${keychain[@]}"} --sign "$identity" "$f"
        echo "fetch-python-runtime: signed ${f#"$dest"/} as $identity"
      fi
    done
fi
