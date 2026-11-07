# Stage 1: Build
FROM rust:1.91 AS builder

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    build-essential \
    cmake \
    pkg-config \
    libssl-dev

# Set the working directory
WORKDIR /usr/src/app

# Cargo features to compile in. Defaults to the pure-Rust DB checks so they work
# out of the box. Kafka is opt-in because librdkafka is a C library that is
# painful to cross-build on musl:
#   docker build --build-arg FEATURES="postgres,redis,kafka" .
#   docker build --build-arg FEATURES="" .   # HTTP/TCP/DNS/etc. only
ARG FEATURES="postgres,redis,mysql,mongodb,rabbitmq"

# Copy source code
COPY . .

# Build the project in release mode
RUN apt-get update && apt-get install -y musl-tools
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo build --release --target=x86_64-unknown-linux-musl --features "${FEATURES}"


# Stage 2: Runtime
FROM alpine


# Set the locale environment variables
ENV LANG=en_US.UTF-8
ENV LANGUAGE=en_US:en
ENV LC_ALL=en_US.UTF-8

# Set the working directory
WORKDIR /app

# Copy the compiled binary from the build stage
COPY --from=builder /usr/src/app/target/x86_64-unknown-linux-musl/release/uptime_monitor .

EXPOSE 8080

# Set the startup command
CMD ["./uptime_monitor"]
