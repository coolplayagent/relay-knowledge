#!/usr/bin/env sh

# Stop immediately after the deep-profile prerequisite checks. The regression
# harness uses this distinct status to prove that check.sh reached stable gates.
exit 23
