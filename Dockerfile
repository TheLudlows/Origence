# Local native libraries require C++, CMake, Ninja and protoc at build time.
FROM rust:1.96-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends cmake ninja-build protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
ENV CMAKE_GENERATOR=Ninja
ARG BUILD_JOBS=1
RUN cargo build --release --locked -j ${BUILD_JOBS}

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgomp1 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home context \
    && mkdir -p /data && chown context:context /data
COPY --from=build /build/target/release/opencontext /usr/local/bin/opencontext
USER context
ENV OC_DATA_DIR=/data
EXPOSE 8080
ENTRYPOINT ["opencontext"]
CMD ["serve", "--bind", "0.0.0.0:8080"]
