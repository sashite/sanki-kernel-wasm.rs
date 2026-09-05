# The pinned build of the sanki reference module (ADR-0034 §9; Rule System —
# Sanki §Build): the image by digest, the toolchain by version
# (rust-toolchain.toml), the dependency graph by Cargo.lock, the flags by
# .cargo/config.toml. Anyone rebuilding from the same commit obtains the same
# bytes, hence the same digest — the one a Rule System event names by `x`.
#
#   build/build.sh          # → dist/sanki.wasm, dist/digest.txt
#
# rust:1.96.0-slim-bookworm, multi-arch manifest as of 2026-09-05.
FROM rust:1.96.0-slim-bookworm@sha256:4732ca96fd086cb9be682050c3f0176288eebaac2b80aa2bcefccfaf198e1950 AS build

# The registry lives under /usr/local/cargo (the image's CARGO_HOME) and the
# sources under /build: the two prefixes .cargo/config.toml remaps, so that no
# path of this container reaches the module.
WORKDIR /build
RUN rustup target add wasm32-unknown-unknown

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo
COPY src ./src

# --locked: the lock file is the dependency graph, never resolved anew.
RUN cargo build --release --target wasm32-unknown-unknown --locked \
    && sha256sum target/wasm32-unknown-unknown/release/sashite_sanki_kernel_wasm.wasm \
       | cut -d ' ' -f 1 > digest.txt \
    && cat digest.txt

FROM scratch AS artifact
COPY --from=build /build/target/wasm32-unknown-unknown/release/sashite_sanki_kernel_wasm.wasm /sanki.wasm
COPY --from=build /build/digest.txt /digest.txt
