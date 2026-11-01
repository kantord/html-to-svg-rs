image := "html-to-svg-test"
# Caches survive between runs: the cargo registry, build output, node_modules (pnpm keeps its store in there).
# Every node_modules (root and each package) gets a volume of its own, so the container never
# shares the host's; a new package needs one here too.
volumes := "-v html-to-svg-cargo-registry:/opt/cargo/registry -v html-to-svg-target:/cache/target -v html-to-svg-node-modules:/work/node_modules -v html-to-svg-pkg-node-modules:/work/packages/playwright-html-to-svg-rs/node_modules -v html-to-svg-fixtures-node-modules:/work/packages/test-fixtures/node_modules"
run := "podman run --rm --init --ipc=host -v " + justfile_directory() + ":/work:Z " + volumes + " " + image

# Build the test image (fast when nothing in it changed).
image:
    podman build -t {{image}} -f container/Containerfile .

# Every check, in the test container: Rust fmt, clippy and tests, then the Playwright tests.
test: image
    {{run}} bash -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && pnpm install --frozen-lockfile && pnpm -r typecheck && pnpm test'

# A shell in the test container, with the same mounts.
test-shell: image
    podman run -it --rm --init --ipc=host -v {{justfile_directory()}}:/work:Z {{volumes}} {{image}} bash
