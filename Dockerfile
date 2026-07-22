FROM rust:slim AS chef

RUN apt-get update \
	&& apt-get install -y --no-install-recommends \
	build-essential \
	ca-certificates \
	libssl-dev \
	pkg-config \
	&& rm -rf /var/lib/apt/lists/*

RUN cargo install --locked cargo-chef

WORKDIR /usr/src/cassie

FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /usr/src/cassie/recipe.json recipe.json

RUN --mount=type=cache,target=/usr/local/cargo/registry \
	cargo chef cook --release --locked --recipe-path recipe.json

COPY . .

# cargo-chef primes the target dir with a placeholder binary for dependency caching.
# Remove that stub so the final image always contains a binary built from the real sources.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
	rm -f target/release/cassie target/release/deps/cassie* \
	&& cargo build --release --locked --bin cassie \
	&& strip target/release/cassie || true

FROM node:24-bookworm-slim AS ui-builder

RUN npm install --global npm@11.16.0

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/cassie/ui

COPY ui/package.json ui/package-lock.json ./
RUN npm ci

COPY ui/ ./
RUN npm run build

FROM debian:trixie-slim AS runtime-fs

RUN mkdir -p /data/midge \
	&& chown -R 65532:65532 /data

FROM gcr.io/distroless/cc-debian13 AS runtime

WORKDIR /app

ENV CASSIE_REST_LISTEN=0.0.0.0:8080 \
	CASSIE_PGWIRE_LISTEN=0.0.0.0:5432 \
	CASSIE_MIDGE_DATA_DIR=/data/midge \
	CASSIE_ADMIN_UI_DIR=/app/ui/dist

COPY --from=runtime-fs --chown=65532:65532 /data /data
COPY --from=builder /usr/src/cassie/target/release/cassie /app/cassie
COPY --from=ui-builder /usr/src/cassie/ui/dist /app/ui/dist

USER 65532:65532

EXPOSE 8080 5432

VOLUME ["/data"]

ENTRYPOINT ["/app/cassie"]
