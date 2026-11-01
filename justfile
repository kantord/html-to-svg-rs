image := "html-to-svg-test"
# Caches survive between runs: the cargo registry, build output, node_modules (pnpm keeps its store
# in there), the web-platform-tests checkout.
# Every node_modules (root and each package) gets a volume of its own, so the container never
# shares the host's; a new package needs one here too.
volumes := "-v html-to-svg-cargo-registry:/opt/cargo/registry -v html-to-svg-target:/cache/target -v html-to-svg-node-modules:/work/node_modules -v html-to-svg-pkg-node-modules:/work/packages/playwright-html-to-svg-rs/node_modules -v html-to-svg-fixtures-node-modules:/work/packages/test-fixtures/node_modules -v html-to-svg-wpt-runner-node-modules:/work/packages/wpt-runner/node_modules -v html-to-svg-wpt:/wpt -v html-to-svg-node-pkg-node-modules:/work/packages/html-to-svg-rs/node_modules"
run := "podman run --rm --init --ipc=host -v " + justfile_directory() + ":/work:Z " + volumes + " " + image

# Build the test image (fast when nothing in it changed).
image:
    podman build -t {{image}} -f container/Containerfile .

# Every check, in the test container: Rust fmt, clippy and tests, then the Playwright tests.
test: image
    {{run}} bash -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && pnpm install --frozen-lockfile && pnpm --filter playwright-html-to-svg-rs build && pnpm --filter html-to-svg-rs build:debug && pnpm -r typecheck && pnpm test && node packages/wpt-runner/src/main.ts check'

# A shell in the test container, with the same mounts.
test-shell: image
    podman run -it --rm --init --ipc=host -v {{justfile_directory()}}:/work:Z {{volumes}} {{image}} bash

# Web-platform-tests reftests through html-to-svg, in the test container; updates
# wpt/results.jsonl and keeps captures of what fails under out/wpt/. `just wpt css/css-borders`
# runs one area (default: every reftest in the checkout, see wpt/paths). Exits non-zero when a
# test that passed now fails. See wpt/README.md.
wpt *ARGS: image
    {{run}} bash -c 'pnpm install --frozen-lockfile && cargo build --release -p html-to-svg-cli && pnpm --filter playwright-html-to-svg-rs build && wpt/serve.sh && node packages/wpt-runner/src/main.ts run --binary /cache/target/release/html-to-svg {{ARGS}}'

# out/wpt/report.html from the captures the last `just wpt` left: for each test that did not pass,
# Chrome's screenshot, our SVG drawn by Chrome, and where they differ. Then opens it.
wpt-report: image
    {{run}} bash -c 'pnpm install --frozen-lockfile && wpt/serve.sh >/dev/null && node packages/wpt-runner/src/main.ts report'
    -@xdg-open out/wpt/report.html >/dev/null 2>&1 || echo "open out/wpt/report.html"

# Capture the Rust regression fixtures (crates/html-to-svg-rs/tests/regressions/<name>/page.*)
# with Chrome, in the test container: `just regression-capture` for all, or name some. The page
# is served over HTTP from the repository root, so web fonts and images are real responses.
regression-capture *NAMES: image
    {{run}} bash -c 'set -e; cargo build -q -p html-to-svg-cli; \
      export CHROME=$(ls -d /ms-playwright/chromium-*/chrome-linux*/chrome | head -1); \
      python3 -m http.server -d /work 8123 >/dev/null 2>&1 & sleep 1; \
      cd crates/html-to-svg-rs/tests/regressions; \
      for d in ${0:-*/}; do d=${d%/}; page=$(ls $d/page.* | head -1); \
        /cache/target/debug/html-to-svg capture "http://localhost:8123/crates/html-to-svg-rs/tests/regressions/$page" $d --size 320x240 --settle 0.5; \
      done' "{{NAMES}}"

# Proof of concept on live sites (Hacker News, Google, NYTimes, this repository's GitHub page,
# shadcn/ui): SVG and PNG snapshots in out/live/<site>/, and out/live/index.html to look at them.
# Needs the network; not part of `just test`. See packages/playwright-html-to-svg-rs/tests/live/.
live-snapshots: image
    {{run}} bash -c 'pnpm install --frozen-lockfile && cargo build -q -p html-to-svg-cli && pnpm --filter playwright-html-to-svg-rs build && cd packages/playwright-html-to-svg-rs && HTML_TO_SVG_LIVE=1 pnpm exec playwright test --project=live --reporter=list; status=$?; node tests/live/gallery.ts; exit $status'
    -@xdg-open out/live/index.html >/dev/null 2>&1 || echo "open out/live/index.html"
