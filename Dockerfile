# A static binary in an empty image: the image is the binary.
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY spec spec
RUN cargo build --release --locked -p visits && strip target/release/visits

FROM scratch
COPY --from=build /src/target/release/visits /visits
USER 65534:65534
EXPOSE 8080
ENTRYPOINT ["/visits"]
CMD ["serve"]
