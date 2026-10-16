# Turso/LanceDB need native build tools; Lance builds protoc from source.
FROM rust:1.98.0-bookworm AS build-inputs
RUN apt-get update && apt-get install -y --no-install-recommends cmake make g++ \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY .cargo ./.cargo
COPY tools/cmake ./tools/cmake
COPY src ./src
COPY tests ./tests
COPY examples ./examples
COPY vendor ./vendor
# Validate every declared target against the actual Docker build context.
RUN cargo metadata --no-deps --format-version 1 > /tmp/cargo-metadata.json

FROM build-inputs AS build
# Bound Cargo compilation to control memory use.
ARG BUILD_JOBS=1
RUN cargo build --release --locked -j ${BUILD_JOBS}

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgomp1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home context \
    && mkdir -p /data && chown context:context /data
COPY --from=build /build/target/release/origence /usr/local/bin/origence
USER context
ENV OC_DATA_DIR=/data
EXPOSE 8080
ENTRYPOINT ["origence"]
CMD ["serve", "--bind", "0.0.0.0:8080"]
