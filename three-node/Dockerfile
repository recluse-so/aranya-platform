FROM rust:latest AS builder
WORKDIR /app
COPY . .
# RUN cargo build --bin aranya-daemon --release
RUN cargo build --release
# RUN target/release/aranya-daemon <path to owner's daemon config>

# docker pull rust:slim-bullseye
FROM rust:slim-bullseye
COPY --from=builder /app/target/release/three-node /app/three-node
ENTRYPOINT ["/app/three-node"]
