FROM rust:latest AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
COPY --from=builder /app/target/release/three-node /app/three-node
ENTRYPOINT ["/app/three-node"]
