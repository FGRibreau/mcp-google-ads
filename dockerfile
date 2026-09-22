FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 mcp && mkdir /data && chown mcp /data
COPY --from=build /src/target/release/mcp-google-ads /usr/local/bin/
USER mcp
ENTRYPOINT ["mcp-google-ads"]
