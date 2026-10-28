FROM rust:latest as builder
WORKDIR /usr/src/solver
COPY . .
RUN cargo install --path .

FROM debian:bookworm-slim
RUN apt-get update && apt install -y openssl libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/cargo/bin/solver /usr/local/bin/solver
COPY . .
CMD ["solver"]
