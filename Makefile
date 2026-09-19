########################################################

TARGET ?=  --all-targets
FEATURES ?= --all-features

########################################################

.PHONY: help
help:
	@awk 'BEGIN {FS = ":.*?## "} /^[0-9a-zA-Z_-]+:.*?## / {sub("\\\\n",sprintf("\n%22c"," "), $$2);printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

########################################################

.PHONY: all
all: build test fmt checkup ## Build and test project

.PHONY: build
build: ## Build project
	cargo build --workspace $(TARGET) $(FEATURES)

.PHONY: test
test: ## Run tests
	cargo test --workspace $(TARGET) $(FEATURES)

.PHONY: fmt
fmt: ## Check code formatting
	cargo fmt --all -- --check

.PHONY: lint
lint: ## Run linter
	cargo clippy --workspace $(TARGET) $(FEATURES) -- -D warnings

.PHONY: checkup
checkup: lint ## Run linter and check project
	cargo check --workspace $(TARGET) $(FEATURES)

.PHONY: deps
deps: ## Check project dependencies for advisories
	cargo deny check advisories

########################################################

.PHONY: update
update:
	cargo update --verbose

.PHONY: features
features:
	cargo features

########################################################

.PHONY: fix
fix:
	cargo fix

.PHONY: fix-all
fix-all:
	cargo fix --all
	cargo clippy --workspace $(TARGET) $(FEATURES) --fix

########################################################
