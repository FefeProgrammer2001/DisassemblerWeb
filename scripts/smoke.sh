#!/usr/bin/env bash
# Usage: scripts/smoke.sh <base-url>
# Compiles a tiny C program through /api/build and checks that assembly came back.
set -euo pipefail
base="${1:?base url}"
body='{"lang":"c","arch":"x86_64","code":"int main(void){int a=1;return a+1;}","stdin":"","trace":false}'
out=$(curl -fsS -X POST -H 'Content-Type: application/json' -d "$body" "$base/api/build")
echo "$out" | head -c 600; echo
echo "$out" | grep -q '"ok":true' || { echo "smoke: build did not return ok:true"; exit 1; }
echo "$out" | grep -q 'main' || { echo "smoke: 'main' not found in response"; exit 1; }
