CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest

.PHONY: build test tray-test check

build: ## compile the CLI
	go build -o $(CLI) .

test: ## Go unit tests
	go test ./...

# No XCTest target; the tray's pure parts and its process runner are checked behind -D SELFTEST, as in gws-mcp and claude-usage.
tray-test: ## check the tray's parsing, formatting, process tree and runner
	swiftc -D SELFTEST -suppress-warnings $(SWIFT_SRC) -o $(SELFTEST)
	$(SELFTEST)

check: test tray-test
