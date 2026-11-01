#!/usr/bin/env bash
# In the test container: make /wpt the pinned web-platform-tests checkout (wpt/commit, only the
# folders in wpt/paths and wpt/areas), make its hostnames resolve, build its manifest and serve it. Prints
# nothing but progress; returns once the server answers. The server keeps running.
set -euo pipefail

REPO=/work
COMMIT=$(cat "$REPO/wpt/commit")
mapfile -t PATHS < <(cat "$REPO/wpt/paths" "$REPO/wpt/areas" | grep -v '^\s*#' | grep -v '^\s*$')

if [ ! -d /wpt/.git ]; then
    git init -q /wpt
    git -C /wpt remote add origin https://github.com/web-platform-tests/wpt.git
fi
# Set every time: a checkout made before a folder was added to wpt/paths would lack it.
git -C /wpt sparse-checkout set --cone "${PATHS[@]}"
if [ "$(git -C /wpt rev-parse -q --verify HEAD 2>/dev/null)" != "$COMMIT" ]; then
    echo "wpt: fetching $COMMIT"
    git -C /wpt fetch -q --depth 1 --filter=blob:none origin "$COMMIT"
    git -C /wpt -c advice.detachedHead=false checkout -q "$COMMIT"
else
    git -C /wpt checkout -q "$COMMIT" -- . 2>/dev/null || true
fi

# The server refuses to start until its hostnames resolve; /etc/hosts is ours in here.
if ! grep -q "web-platform.test" /etc/hosts; then
    (cd /wpt && ./wpt make-hosts-file) >> /etc/hosts
fi

echo "wpt: building the manifest"
(cd /wpt && ./wpt manifest --no-download -p /wpt/MANIFEST.json) > /tmp/wpt-manifest.log 2>&1 || {
    cat /tmp/wpt-manifest.log; exit 1;
}

echo "wpt: starting the server"
(cd /wpt && exec ./wpt serve) > /tmp/wpt-serve.log 2>&1 &
for _ in $(seq 120); do
    if curl -sf -o /dev/null http://web-platform.test:8000/; then
        echo "wpt: serving http://web-platform.test:8000/"
        exit 0
    fi
    sleep 1
done
echo "wpt: the server did not start"; tail -30 /tmp/wpt-serve.log; exit 1
