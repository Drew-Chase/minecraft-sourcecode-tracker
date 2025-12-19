FROM rust:1.91.0-alpine AS builder
WORKDIR /build
COPY . .

RUN apk add --no-cache musl-dev pkgconfig openssl-dev openssl-libs-static
RUN cargo build --release
RUN strip target/release/minecraft-sourcecode-tracker

FROM alpine:latest
RUN mkdir -p /app
COPY --from=builder /build/target/release/minecraft-sourcecode-tracker /app/minecraft_sourcecode_tracker
RUN chmod +x /app/minecraft_sourcecode_tracker
WORKDIR /app

ENTRYPOINT ["/bin/sh", "-c", "\
  : ${GIT_USERNAME:?GIT_USERNAME is required} && \
  : ${GIT_AUTH_TOKEN:?GIT_AUTH_TOKEN is required} && \
  : ${GIT_URL:?GIT_URL is required} && \
  exec /app/minecraft_sourcecode_tracker --git-username \"$GIT_USERNAME\" --git-auth-token \"$GIT_AUTH_TOKEN\" --git-url \"$GIT_URL\""]