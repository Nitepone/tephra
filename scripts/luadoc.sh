#!/bin/sh
# Regenerates the Lua API reference (web/luadoc) from luadoc/ with LDoc.
# Install LDoc with `luarocks install ldoc` (or `apt install lua-ldoc`), or point LDOC at it.
set -eu
cd "$(dirname "$0")/../luadoc"
LDOC="${LDOC:-ldoc}"
rm -rf ../web/luadoc
"$LDOC" -c config.ld .
echo "Wrote web/luadoc. Rebuild tephra to serve it at /docs/lua/."
