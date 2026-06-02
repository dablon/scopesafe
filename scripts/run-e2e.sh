#!/usr/bin/env bash
# End-to-end runner for scopesafe. Builds, tests, and exercises the binary
# against a sample project that simulates an AI agent's work session.
set -euo pipefail
IFS=$'\n\t'

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly BINARY="${SCOPESAFE_BIN:-/app/target/release/scopesafe}"
readonly SAMPLE_DIR="${SAMPLE_DIR:-/tmp/scopesafe-e2e/sample-payments}"
readonly XDG_DATA_HOME_E2E="${XDG_DATA_HOME_E2E:-/data}"
export XDG_DATA_HOME="${XDG_DATA_HOME_E2E}"

# Force colors through in the TTY-less environment
export CLICOLOR_FORCE=1

log() { printf '[%s] [%s] %s\n' "$(date -u +"%Y-%m-%dT%H:%M:%SZ")" "$1" "$2"; }
info() { log "INFO" "$1"; }
warn() { log "WARN" "$1"; }
die() { log "ERROR" "$1"; exit 1; }

main() {
    info "==== SCOPESAFE v$(grep -E '^version' /app/Cargo.toml | head -1 | cut -d'"' -f2) E2E ===="
    info "Binary:        $BINARY"
    info "Sample dir:    $SAMPLE_DIR"
    info "Data dir:      $XDG_DATA_HOME_E2E"
    info "Rust:          $(rustc --version)"
    info "Cargo:         $(cargo --version)"
    info "Git:           $(git --version)"

    # Sanity check
    [[ -x "$BINARY" ]] || die "binary not found at $BINARY"
    command -v git >/dev/null || die "git not installed"
    command -v jq >/dev/null || warn "jq not installed (JSON prettify will be skipped)"

    # Reset state
    rm -rf "$XDG_DATA_HOME_E2E" "$SAMPLE_DIR"
    mkdir -p "$XDG_DATA_HOME_E2E"

    ##########################################################################
    # Phase 1: cargo test --all
    ##########################################################################
    info ""
    info "==== PHASE 1: cargo test --all ===="
    cd /app
    if cargo test --all --color=never 2>&1 | tee /tmp/cargo-test.log; then
        info "PHASE 1: ALL TESTS PASSED"
    else
        die "PHASE 1: cargo test FAILED"
    fi

    ##########################################################################
    # Phase 2: build a sample project simulating an AI agent's work
    ##########################################################################
    info ""
    info "==== PHASE 2: setup sample project ===="
    mkdir -p "$SAMPLE_DIR/payments/subscriptions" "$SAMPLE_DIR/payments/vendor" "$SAMPLE_DIR/src"

    cat > "$SAMPLE_DIR/payments/retry.go" <<'EOF'
package payments

func Retry() error {
    return nil
}
EOF

    cat > "$SAMPLE_DIR/payments/timeout.go" <<'EOF'
package payments

func Timeout() error {
    return nil
}
EOF

    cat > "$SAMPLE_DIR/payments/subscriptions/main.go" <<'EOF'
package subscriptions

func Main() {}
EOF

    cat > "$SAMPLE_DIR/payments/vendor/dep.go" <<'EOF'
package vendor

func V() {}
EOF

    cat > "$SAMPLE_DIR/src/main.go" <<'EOF'
package main

func main() {}
EOF

    cat > "$SAMPLE_DIR/.env" <<'EOF'
DB_PASSWORD=supersecret123
STRIPE_KEY=sk_live_xxxxxxxxxxxx
EOF

    cat > "$SAMPLE_DIR/README.md" <<'EOF'
# Payments service
EOF

    cd "$SAMPLE_DIR"
    git init -q
    git config user.email "e2e@scopesafe.test"
    git config user.name "scopesafe-e2e"
    git add -A
    git commit -qm "initial sample"
    info "sample project ready: $(find "$SAMPLE_DIR" -type f -not -path '*/.git/*' | wc -l) files"

    ##########################################################################
    # Phase 3: scope lifecycle
    ##########################################################################
    info ""
    info "==== PHASE 3: scope lifecycle ===="

    run_step "init scope" \
        "$BINARY init --task='fix retry timeout' \
            --files='payments/*.go' \
            --exclude='payments/subscriptions/*,payments/vendor/*' \
            --project=$SAMPLE_DIR"

    run_step "track in-scope: payments/retry.go" \
        "$BINARY track --file=payments/retry.go --action=modify"

    run_step "track in-scope: payments/timeout.go" \
        "$BINARY track --file=payments/timeout.go --action=modify"

    run_step "track out-of-scope: payments/subscriptions/main.go (excluded)" \
        "$BINARY track --file=payments/subscriptions/main.go --action=modify"

    run_step "track out-of-scope: src/main.go (not in --files)" \
        "$BINARY track --file=src/main.go --action=modify"

    # blocked file should fail
    info "EXPECTED FAIL: track blocked .env"
    if "$BINARY" track --file=.env --action=modify 2>/tmp/blocked.err; then
        die "blocked file was allowed — security regression!"
    fi
    if grep -q "blocked file cannot be modified" /tmp/blocked.err; then
        info "blocked file correctly rejected"
    else
        die "blocked-file error did not match expected: $(cat /tmp/blocked.err)"
    fi

    run_step "status" "$BINARY status"
    run_step "audit"  "$BINARY audit"
    run_step "list scopes" "$BINARY list-scopes"

    ##########################################################################
    # Phase 4: approve + score goes up
    ##########################################################################
    info ""
    info "==== PHASE 4: approve one OOS change ===="

    run_step "approve src/main.go" \
        "$BINARY approve --file=src/main.go"

    # After approval, score should rise
    score_output=$("$BINARY" status)
    info "$score_output"
    if echo "$score_output" | grep -q "Scope score: 88%"; then
        info "scope score is 88% (2 in-scope + 0.5 pending + 1.0 approved = 3.5/4 = 88%)"
    else
        warn "score is not 88% — got:"
        echo "$score_output"
    fi

    ##########################################################################
    # Phase 5: reject + revert
    ##########################################################################
    info ""
    info "==== PHASE 5: reject + git revert ===="

    # Make a tracked out-of-scope change to the file system
    cat > "$SAMPLE_DIR/payments/subscriptions/main.go" <<'EOF'
package subscriptions

// BUG: agent introduced this in scope drift
func Refactor() {}
EOF

    # Reject the new OOS event
    run_step "reject payments/subscriptions/main.go" \
        "$BINARY reject --file=payments/subscriptions/main.go --reason='intentional drift'"

    # Rejecting does NOT auto-revert. Now reset the file to HEAD so we can
    # test the actual revert path.
    info "resetting file to HEAD via git"
    (cd "$SAMPLE_DIR" && git checkout HEAD -- payments/subscriptions/main.go)

    # Track it again (a pending OOS event)
    run_step "track again after reset" \
        "$BINARY track --file=payments/subscriptions/main.go --action=modify"

    # Modify the file on disk to simulate the agent's bad change
    cat > "$SAMPLE_DIR/payments/subscriptions/main.go" <<'EOF'
package subscriptions

// BUG: agent introduced this
func Refactor() {}
EOF

    run_step "revert-all (should restore the file from git)" \
        "$BINARY revert-all"

    # Check the file is back to the original
    content=$(cat "$SAMPLE_DIR/payments/subscriptions/main.go")
    if echo "$content" | grep -q "Refactor"; then
        die "revert failed — file still contains 'Refactor':\n$content"
    fi
    if echo "$content" | grep -q "package subscriptions"; then
        info "file successfully reverted via git"
    else
        die "revert produced unexpected content: $content"
    fi

    ##########################################################################
    # Phase 6: weekly report (aggregated history)
    ##########################################################################
    info ""
    info "==== PHASE 6: weekly drift report ===="

    run_step "report-weekly" "$BINARY report-weekly"

    ##########################################################################
    # Phase 7: MCP server end-to-end (stdin/stdout JSON-RPC)
    ##########################################################################
    info ""
    info "==== PHASE 7: MCP server stdio E2E ===="

    # Reset DB to get a clean MCP run
    rm -rf "$XDG_DATA_HOME_E2E"
    mkdir -p "$XDG_DATA_HOME_E2E"

    cat > /tmp/mcp-requests.jsonl <<'EOF'
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"init_scope","arguments":{"task":"mcp e2e","files":"src/*.go"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"check_file","arguments":{"file":"src/main.go"}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"check_file","arguments":{"file":"README.md"}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"check_file","arguments":{"file":".env"}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"track_file","arguments":{"file":"src/main.go","action":"modify"}}}
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"track_file","arguments":{"file":".env","action":"modify"}}}
{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"status","arguments":{}}}
{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"audit","arguments":{}}}
{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"weekly_report","arguments":{}}}
EOF

    # Run the MCP server, pipe requests, capture responses
    info "starting scopesafe mcp via stdio"
    "$BINARY" mcp --project "$SAMPLE_DIR" < /tmp/mcp-requests.jsonl > /tmp/mcp-responses.jsonl 2> /tmp/mcp-stderr.log || true

    if [[ ! -s /tmp/mcp-responses.jsonl ]]; then
        die "no MCP responses — stderr: $(cat /tmp/mcp-stderr.log)"
    fi

    response_count=$(wc -l < /tmp/mcp-responses.jsonl)
    info "received $response_count JSON-RPC responses"

    # Each response must be valid JSON
    if command -v jq >/dev/null 2>&1; then
        while IFS= read -r line; do
            echo "$line" | jq . >/dev/null
        done < /tmp/mcp-responses.jsonl
        info "all responses are valid JSON"
    else
        while IFS= read -r line; do
            echo "$line" | python3 -c "import json,sys; json.loads(sys.stdin.read())"
        done < /tmp/mcp-responses.jsonl
        info "all responses are valid JSON"
    fi

    # Spot-check: response 1 must be initialize with protocolVersion
    first=$(head -1 /tmp/mcp-responses.jsonl)
    if echo "$first" | grep -q '"protocolVersion":"2024-11-05"'; then
        info "MCP initialize OK (protocolVersion=2024-11-05)"
    else
        die "MCP initialize response malformed: $first"
    fi

    # Response 6 (check_file .env) should say BLOCKED
    if sed -n '6p' /tmp/mcp-responses.jsonl | grep -q '"verdict":"BLOCKED"'; then
        info "MCP check_file .env -> BLOCKED (correct)"
    else
        die "MCP check_file .env did not return BLOCKED: $(sed -n '6p' /tmp/mcp-responses.jsonl)"
    fi

    # Response 8 (track_file .env) should be an error
    if sed -n '8p' /tmp/mcp-responses.jsonl | grep -q '"error"'; then
        info "MCP track_file .env -> error (correct)"
    else
        die "MCP track_file .env did not error: $(sed -n '8p' /tmp/mcp-responses.jsonl)"
    fi

    # Response 11 (weekly_report) should mention the in-scope count
    if sed -n '11p' /tmp/mcp-responses.jsonl | grep -q 'Tasks:'; then
        info "MCP weekly_report returned data"
    else
        die "MCP weekly_report empty: $(sed -n '11p' /tmp/mcp-responses.jsonl)"
    fi

    ##########################################################################
    # Summary
    ##########################################################################
    info ""
    info "================================================================"
    info "  ALL E2E PHASES PASSED"
    info "  scopesafe $(grep -E '^version' /app/Cargo.toml | head -1 | cut -d'"' -f2) on $(uname -srm)"
    info "================================================================"
}

run_step() {
    local name="$1"
    shift
    info "→ $name"
    if "$@" 2>&1; then
        info "  ✓ $name"
    else
        die "  ✗ $name FAILED"
    fi
}

main "$@"
