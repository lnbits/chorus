FROM node:24-bookworm-slim AS web
WORKDIR /src/apps/web
COPY apps/web/package*.json ./
RUN npm ci
COPY apps/web/ ./
RUN npm run check && npm run build

FROM rust:1.98-bookworm AS rust
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --locked --release -p chorus-node -p chorus-verifier

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home chorus
COPY --from=rust /src/target/release/chorus-node /usr/local/bin/chorus-node
COPY --from=rust /src/target/release/fedwasm /usr/local/bin/fedwasm
COPY --from=web /src/apps/web/build /opt/chorus/web
USER chorus
WORKDIR /home/chorus
VOLUME ["/home/chorus/data"]
EXPOSE 8787
ENTRYPOINT ["chorus-node"]
CMD ["serve", "--bind", "0.0.0.0:8787", "--web-root", "/opt/chorus/web"]
