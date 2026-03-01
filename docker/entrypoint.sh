#!/bin/bash
set -e

if [ -n "$GITHUB_TOKEN" ]; then
    echo "$GITHUB_TOKEN" | gh auth login --with-token 2>/dev/null || true
    gh auth setup-git 2>/dev/null || true
fi

if [ -n "$ANYCODE_REPO" ]; then
    git clone "$ANYCODE_REPO" /workspace/repo 2>/dev/null || true
    cd /workspace/repo
fi

exec sandbox-agent server --host 0.0.0.0 --port 2468 --no-token
