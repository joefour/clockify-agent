CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest
# A space in the name: quote every path that holds $(APP).
APP         := Clockify Agent.app
# Login Items shows the app's name only for a team-signed agent (an ad-hoc one
# shows as "exec"): sign with TEAM's Apple Development identity, as gws-mcp and
# claude-usage do. "-" (ad-hoc) when this Mac has none.
TEAM        ?= S36W2NCK97
SIGN_ID     ?= $(or $(shell security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: .*\)"$$/\1/p' | while read -r n; do security find-certificate -c "$$n" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | grep -q "OU=$(TEAM)" && { echo "$$n"; break; }; done),-)

.PHONY: build test tray-test app check

build: ## compile the CLI
	go build -o $(CLI) .

test: ## Go unit tests
	go test ./...

# No XCTest target; the tray's pure parts and its process runner are checked behind -D SELFTEST, as in gws-mcp and claude-usage.
tray-test: ## check the tray's parsing, formatting, process tree and runner
	swiftc -D SELFTEST -suppress-warnings $(SWIFT_SRC) -o $(SELFTEST)
	$(SELFTEST)

app: ## build build/Clockify Agent.app, signed with SIGN_ID
	rm -rf "build/$(APP)" && mkdir -p "build/$(APP)/Contents/MacOS"
	swiftc -O $(SWIFT_SRC) -o "build/$(APP)/Contents/MacOS/$(TRAY)"
	cp tray/Info.plist "build/$(APP)/Contents/"
	codesign --force --sign "$(SIGN_ID)" "build/$(APP)"

check: test tray-test
