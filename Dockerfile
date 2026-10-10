# SQLite/LanceDB require a C/C++ compiler and protoc at build time.
FROM rust:1.98.1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler libprotobuf-dev \
    && rm -rf /var/lib/apt/lists/*
RUN test -f /usr/include/google/protobuf/empty.proto \
    && protoc --proto_path=/usr/include --descriptor_set_out=/tmp/protobuf-check.pb google/protobuf/empty.proto
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
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
