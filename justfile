# Centinel task runner.

# Build the documentation and open it in a browser. Rebuilds on every save.
book:
    #!/usr/bin/env sh
    set -e
    # Installs mdbook first if this machine does not have it.
    command -v mdbook >/dev/null 2>&1 || cargo install mdbook --locked
    # The output directory comes from book.toml. `--dest-dir` would resolve against
    # the working directory instead, and drop the book beside the crates.
    mdbook serve --open {{ justfile_directory() / "contrib/book" }}
