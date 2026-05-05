FROM rust:1-bookworm AS test
WORKDIR /workspace
COPY . .
RUN rustup component add rustfmt clippy
RUN cargo fmt --check
RUN cargo clippy --all-targets -- -D warnings
RUN cargo test

FROM rust:1-bookworm AS build
WORKDIR /workspace
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim AS runtime
RUN useradd --system --user-group --home-dir /var/lib/trident trident \
    && mkdir -p /var/lib/trident \
    && chown -R trident:trident /var/lib/trident
COPY --from=build /workspace/target/release/trident /usr/local/bin/trident
USER trident
VOLUME ["/var/lib/trident"]
EXPOSE 7070
ENTRYPOINT ["trident"]
CMD ["--data-dir", "/var/lib/trident", "serve", "--bind", "0.0.0.0:7070"]
