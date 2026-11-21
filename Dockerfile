# A static binary in an empty image: the image is the binary, and the place database beside it.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY spec spec
RUN cargo build --release --locked -p visits --features geoip && strip target/release/visits

# DB-IP's IP to City Lite and IP to ASN Lite (CC BY 4.0, https://db-ip.com), the month the image is
# built in, or the month before while this month's is not out yet; the build fails without either.
# The Tor Project's exit list as of the build, empty when it cannot be fetched. All are refreshed by
# building the image again.
FROM alpine:3 AS geo
RUN mkdir /geo && y=$(date -u +%Y) && m=$(date -u +%m) && m=${m#0} && \
    for back in 0 1; do \
      mm=$((m - back)); yy=$y; if [ "$mm" -lt 1 ]; then mm=12; yy=$((y - 1)); fi; \
      f=$(printf 'dbip-city-lite-%d-%02d.mmdb.gz' "$yy" "$mm"); \
      if wget -q -O /tmp/city.mmdb.gz "https://download.db-ip.com/free/$f"; then \
        gunzip -c /tmp/city.mmdb.gz > /geo/city.mmdb && echo "$f" > /geo/SOURCE && break; \
      fi; \
    done && test -s /geo/city.mmdb && \
    for back in 0 1; do \
      mm=$((m - back)); yy=$y; if [ "$mm" -lt 1 ]; then mm=12; yy=$((y - 1)); fi; \
      f=$(printf 'dbip-asn-lite-%d-%02d.mmdb.gz' "$yy" "$mm"); \
      if wget -q -O /tmp/asn.mmdb.gz "https://download.db-ip.com/free/$f"; then \
        gunzip -c /tmp/asn.mmdb.gz > /geo/asn.mmdb && echo "$f" >> /geo/SOURCE && break; \
      fi; \
    done && test -s /geo/asn.mmdb && \
    (wget -q -O /geo/tor.txt https://check.torproject.org/torbulkexitlist || : > /geo/tor.txt)

FROM scratch
COPY --from=build /src/target/release/visits /visits
COPY --from=geo /geo /geo
ENV VISITS_GEOIP=/geo/city.mmdb VISITS_ASN=/geo/asn.mmdb VISITS_TOR=/geo/tor.txt
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/visits"]
CMD ["serve"]
