# syntax=docker/dockerfile:1

# The Centinel book, built and then served as static files.
#
#   docker build -t centinel-book contrib/book
#   docker run --rm -p 8080:80 centinel-book
#
# The build context is this directory, not the repository root. The book reads
# no file above itself, and a context that holds `target/` sends gigabytes to
# the builder for a folder of Markdown.

# ---------------------------------------------------------------------------
# Build. mdBook turns src/ into book/.
# ---------------------------------------------------------------------------
FROM alpine:3.23 AS build

# Keep this in step with the mdBook that `just book` installs. A book that
# builds on a workstation and fails here is a version difference, and the
# message that reports it does not say so.
ARG MDBOOK_VERSION=0.5.4

# BuildKit sets TARGETARCH from the platform it builds for.
ARG TARGETARCH

RUN apk add --no-cache curl

# The release tarballs hold one static musl binary. `cargo install mdbook`
# gives the same file, but it first pulls a Rust toolchain and compiles for
# minutes.
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) target=x86_64-unknown-linux-musl ;; \
      arm64) target=aarch64-unknown-linux-musl ;; \
      *) echo "no mdBook release for TARGETARCH=$TARGETARCH" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/rust-lang/mdBook/releases/download/v${MDBOOK_VERSION}/mdbook-v${MDBOOK_VERSION}-${target}.tar.gz" \
      | tar -xz -C /usr/local/bin mdbook

WORKDIR /src

# Named copies, and not `COPY . .`: the output directory is `book/`, and a
# stale one from a workstation build must not enter the image.
COPY book.toml ./
COPY theme ./theme
COPY src ./src

# book.toml sets `create-missing = false`. A page that SUMMARY.md lists and no
# file supplies stops the build here, instead of shipping an empty chapter.
RUN mdbook build

# ---------------------------------------------------------------------------
# Serve. The output is plain HTML, so this stage keeps no toolchain.
# ---------------------------------------------------------------------------
FROM nginx:1.30-alpine

COPY --from=build /src/book /usr/share/nginx/html

# Two things the default configuration does not do. mdBook writes a 404.html
# that carries the book's own sidebar; without `error_page`, a wrong URL gets
# nginx's own page and the reader loses the navigation. And nginx compresses
# text/html alone by default, which leaves the search index — the largest file
# in the book — to go over the wire whole.
COPY <<'EOF' /etc/nginx/conf.d/default.conf
server {
    listen 80;
    listen [::]:80;

    root /usr/share/nginx/html;
    index index.html;

    gzip on;
    gzip_min_length 1024;
    # Both JavaScript types. nginx has changed this mapping once already, and a
    # base image that changes it again would stop compression with no error.
    gzip_types text/css application/javascript text/javascript image/svg+xml;

    location / {
        try_files $uri $uri/ $uri.html =404;
    }

    error_page 404 /404.html;
}
EOF

EXPOSE 80
