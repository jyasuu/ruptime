# Stage 1: Build
FROM rust:1.85.1 AS builder

# Set the working directory
WORKDIR /usr/src/app

# Cache dependencies
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY . .

# Build the project in release mode
RUN cargo build --release

# Stage 2: Runtime
FROM ubuntu:24.04

# Set the locale environment variables
ENV LANG en_US.UTF-8
ENV LANGUAGE en_US:en
ENV LC_ALL en_US.UTF-8

# Set the working directory
WORKDIR /app

# Copy the compiled binary from the build stage
COPY --from=builder /usr/src/app/target/release/uptime_monitor .

EXPOSE 8080

# Set the startup command
CMD ["./uptime_monitor"]