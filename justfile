image := "html-to-svg-test"
# Caches survive between runs: the cargo registry, build output, node_modules (pnpm keeps its store
# in there), the web-platform-tests checkout.
# Every node_modules (root and each package) gets a volume of its own, so the container never
# shares the host's; a new package needs one here too.
volumes := "-v html-to-svg-cargo-registry:/opt/cargo/registry -v html-to-svg-target:/cache/target -v html-to-svg-node-modules:/work/node_modules -v html-to-svg-pkg-node-modules:/work/packages/playwright-html-to-svg-rs/node_modules -v html-to-svg-fixtures-node-modules:/work/packages/test-fixtures/node_modules -v html-to-svg-wpt-runner-node-modules:/work/packages/wpt-runner/node_modules -v html-to-svg-wpt:/wpt"
run := "podman run --rm --init --ipc=host -v " + justfile_directory() + ":/work:Z " + volumes + " " + image

# Build the test image (fast when nothing in it changed).
image:
    podman build -t {{image}} -f container/Containerfile .

# Every check, in the test container: Rust fmt, clippy and tests, then the Playwright tests.
test: image
    {{run}} bash -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && pnpm install --frozen-lockfile && pnpm --filter playwright-html-to-svg-rs build && pnpm -r typecheck && pnpm test && node packages/wpt-runner/src/main.ts check'

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
