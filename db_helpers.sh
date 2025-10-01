#!/usr/bin/env bash

# Simple helper functions for database queries
# Source this file to get the functions: source db_helpers.sh

# Quick query - just show the basic info
quick_query() {
    echo "📊 Quick database info:"
    cargo run --bin query_chunks --quiet 2>/dev/null | head -15
}

# Full query - show everything
full_query() {
    echo "📋 Full database query:"
    cargo run --bin query_chunks --quiet 2>/dev/null
}

# Just show stats
db_stats() {
    echo "📈 Database statistics:"
    cargo run --bin query_chunks --quiet 2>/dev/null | grep -E "(version|Total embeddings)"
}

echo "Database helper functions loaded!"
echo "Available commands:"
echo "  quick_query  - Show first 5 chunks"
echo "  full_query   - Show chunks + embeddings"  
echo "  db_stats     - Show just the stats"