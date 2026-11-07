FROM rust:1.88-bookworm AS build
WORKDIR /src
COPY rust/Cargo.toml rust/Cargo.lock ./
COPY rust/src ./src
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home linkwise \
    && mkdir -p /var/lib/linkwise \
    && chown linkwise:linkwise /var/lib/linkwise
COPY --from=build /src/target/release/linkwise /usr/local/bin/linkwise
USER 10001:10001
ENTRYPOINT ["linkwise"]
