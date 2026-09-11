# syntax=docker/dockerfile:1

FROM rust:1.97.1-bookworm AS builder
WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY apps ./apps
COPY services ./services
COPY packages ./packages
COPY infrastructure/postgres/migrations ./infrastructure/postgres/migrations

RUN cargo build --locked --release --workspace --bins

FROM debian:bookworm-slim AS runtime
ARG DEFAULT_SERVICE=brain-service
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 agrisense

COPY --from=builder /workspace/target/release/whatsapp-gateway /usr/local/bin/
COPY --from=builder /workspace/target/release/brain-service /usr/local/bin/
COPY --from=builder /workspace/target/release/farm-service /usr/local/bin/
COPY --from=builder /workspace/target/release/agronomy-service /usr/local/bin/
COPY --from=builder /workspace/target/release/finance-service /usr/local/bin/
COPY --from=builder /workspace/target/release/marketplace-service /usr/local/bin/
COPY --from=builder /workspace/target/release/analytics-service /usr/local/bin/
COPY --from=builder /workspace/target/release/ai-service /usr/local/bin/
COPY --from=builder /workspace/target/release/platform-service /usr/local/bin/

ENV AGRISENSE_SERVICE=${DEFAULT_SERVICE}
USER agrisense
EXPOSE 3001 3002 3003 3004 3005 3006 3007 3008 3009
CMD ["/bin/sh", "-c", "exec /usr/local/bin/$AGRISENSE_SERVICE"]
