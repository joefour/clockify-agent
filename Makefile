CLI         := clockify
TRAY        := clockify-agent-tray
SWIFT_SRC   := $(wildcard tray/*.swift)
SELFTEST    := $(or $(TMPDIR),/tmp)/$(TRAY)-selftest
# A space in the name: quote every path that holds $(APP).
APP         := Clockify Agent.app
AGENT       := com.clockifyagent.tray
AGENT_PLIST := $(HOME)/Library/LaunchAgents/$(AGENT).plist
BIN_DIR     := $(HOME)/.local/bin
APP_DIR     := $(HOME)/Applications
# Login Items shows the app's name only for a team-signed agent (an ad-hoc one
# shows as "exec"): sign with TEAM's Apple Development identity, as gws-mcp and
# claude-usage do. "-" (ad-hoc) when this Mac has none.
TEAM        ?= S36W2NCK97
SIGN_ID     ?= $(or $(shell security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: .*\)"$$/\1/p' | while read -r n; do security find-certificate -c "$$n" -p 2>/dev/null | openssl x509 -noout -subject 2>/dev/null | grep -q "OU=$(TEAM)" && { echo "$$n"; break; }; done),-)

# Unload the login agent and wait until it has exited.
stop-agent = launchctl bootout gui/$$(id -u)/$(AGENT) 2>/dev/null; while launchctl print gui/$$(id -u)/$(AGENT) >/dev/null 2>&1; do sleep 0.2; done

.PHONY: build test tray-test app check install uninstall

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

install: build app ## install the CLI, run the tray now and at every login
	sed -e 's|@HOME@|$(HOME)|g' -e 's|@REPO@|$(CURDIR)|g' tray/$(AGENT).plist > $(AGENT).plist.tmp
	plutil -lint $(AGENT).plist.tmp
	@$(stop-agent)
	mkdir -p $(BIN_DIR) "$(APP_DIR)" $(dir $(AGENT_PLIST))
	install -m 755 $(CLI) $(BIN_DIR)/
	rm -rf "$(APP_DIR)/$(APP)" && cp -R "build/$(APP)" "$(APP_DIR)/"
	/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$(APP_DIR)/$(APP)"
	mv $(AGENT).plist.tmp $(AGENT_PLIST)
	launchctl bootstrap gui/$$(id -u) $(AGENT_PLIST)

uninstall: ## stop the tray, remove the login item and the app (the CLI stays)
	@$(stop-agent)
	rm -rf $(AGENT_PLIST) "$(APP_DIR)/$(APP)"
