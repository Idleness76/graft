#!/usr/bin/env bash

# Simple CLI approach to query the chunks database
# Uses the Rust binary we just created

echo "🔍 Querying rust_book_chunks.sqlite database..."
echo

# Build and run the query_chunks binary
cargo run --bin query_chunks --quiet

echo
echo "✅ Query completed!"