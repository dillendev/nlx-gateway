FROM rust:1.64.0 AS build

COPY . /build

WORKDIR /build

RUN apt-get update && \
    apt-get install -y protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

RUN cargo build --release

FROM gcr.io/distroless/cc

COPY --from=build /build/target/release/nlx-gateway /nlx-gateway

CMD ["/nlx-gateway"]
