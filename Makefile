.DEFAULT_GOAL := help

# ─── Colors ────────────────────────────────────────────────────────────────────
BOLD   := \033[1m
RESET  := \033[0m
GREEN  := \033[32m
YELLOW := \033[33m
CYAN   := \033[36m

# ─── Help ──────────────────────────────────────────────────────────────────────
.PHONY: help
help: ## Show this help message
	@echo ""
	@echo "$(BOLD)AgriSense — Developer Commands$(RESET)"
	@echo ""
	@awk 'BEGIN {FS = ":.*##"} /^[a-zA-Z_-]+:.*?##/ { printf "  $(CYAN)%-28s$(RESET) %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@echo ""

# ─── Setup ─────────────────────────────────────────────────────────────────────
.PHONY: setup
setup: ## Initial developer setup (copy .env, install tools)
	@echo "$(GREEN)Setting up AgriSense development environment...$(RESET)"
	@cp -n .env.example .env || true
	@which cargo > /dev/null || (echo "Install Rust: https://rustup.rs" && exit 1)
	@which docker > /dev/null || (echo "Install Docker: https://docs.docker.com/get-docker/" && exit 1)
	@which node > /dev/null || (echo "Install Node.js >= 20: https://nodejs.org" && exit 1)
	@cargo install sqlx-cli --no-default-features --features rustls,postgres 2>/dev/null || true
	@cargo install cargo-watch 2>/dev/null || true
	@echo "$(GREEN)✅ Setup complete. Run 'make dev' to start.$(RESET)"

# ─── Infrastructure ────────────────────────────────────────────────────────────
.PHONY: infra-up
infra-up: ## Start infrastructure (Postgres, Redis, NATS, monitoring)
	docker compose -f docker-compose.yml up -d postgres redis nats
	@echo "$(GREEN)✅ Infrastructure started$(RESET)"

.PHONY: infra-down
infra-down: ## Stop infrastructure
	docker compose down

.PHONY: infra-logs
infra-logs: ## Show infrastructure logs
	docker compose logs -f

.PHONY: monitoring-up
monitoring-up: ## Start full monitoring stack (Prometheus, Grafana, Loki)
	docker compose --profile monitoring up -d
	@echo "$(GREEN)✅ Monitoring stack started$(RESET)"
	@echo "  Grafana:    http://localhost:3000 (admin/admin)"
	@echo "  Prometheus: http://localhost:9090"

# ─── Database ──────────────────────────────────────────────────────────────────
.PHONY: db-create
db-create: ## Create the database
	sqlx database create

.PHONY: db-migrate
db-migrate: ## Run all pending migrations
	sqlx migrate run --source infrastructure/postgres/migrations

.PHONY: db-migrate-revert
db-migrate-revert: ## Revert last migration
	sqlx migrate revert --source infrastructure/postgres/migrations

.PHONY: db-reset
db-reset: ## Drop and recreate database, run all migrations
	sqlx database drop -y
	sqlx database create
	$(MAKE) db-migrate

.PHONY: db-shell
db-shell: ## Open psql shell
	docker compose exec postgres psql -U agrisense -d agrisense

# ─── Build ─────────────────────────────────────────────────────────────────────
.PHONY: build
build: ## Build all Rust services
	cargo build --workspace

.PHONY: build-release
build-release: ## Build all services in release mode
	cargo build --workspace --release

.PHONY: check
check: ## Run cargo check on workspace
	cargo check --workspace

.PHONY: lint
lint: ## Run clippy on all workspace members
	cargo clippy --workspace --all-targets --all-features -- -D warnings

.PHONY: fmt
fmt: ## Format all Rust code
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting without applying
	cargo fmt --all -- --check

# ─── Test ──────────────────────────────────────────────────────────────────────
.PHONY: test
test: ## Run all tests
	cargo test --workspace

.PHONY: test-service
test-service: ## Run tests for a specific service (usage: make test-service S=brain-service)
	cargo test -p $(S)

.PHONY: test-coverage
test-coverage: ## Generate test coverage report (requires cargo-tarpaulin)
	cargo tarpaulin --workspace --out Html --output-dir coverage/

# ─── Dev Servers ───────────────────────────────────────────────────────────────
.PHONY: dev
dev: infra-up ## Start all Rust services in watch mode
	@echo "$(YELLOW)Starting all services in development mode...$(RESET)"
	cargo watch -x "run --package brain-service" &
	cargo watch -x "run --package farm-service" &
	cargo watch -x "run --package agronomy-service" &
	cargo watch -x "run --package finance-service" &
	cargo watch -x "run --package ai-service" &
	cargo watch -x "run --package platform-service" &
	cargo watch -x "run --package analytics-service" &
	cargo watch -x "run --package marketplace-service" &
	cargo watch -x "run --package apps/whatsapp-gateway" &
	@echo "$(GREEN)✅ All services started$(RESET)"

.PHONY: dev-brain
dev-brain: ## Run brain-service in watch mode
	cargo watch -x "run --package brain-service"

.PHONY: dev-farm
dev-farm: ## Run farm-service in watch mode
	cargo watch -x "run --package farm-service"

.PHONY: dev-ai
dev-ai: ## Run ai-service in watch mode
	cargo watch -x "run --package ai-service"

.PHONY: dev-gateway
dev-gateway: ## Run whatsapp-gateway in watch mode
	cargo watch -x "run --package whatsapp-gateway"

# ─── Protobuf ──────────────────────────────────────────────────────────────────
.PHONY: proto-gen
proto-gen: ## Generate Rust + TypeScript code from proto files
	@echo "$(YELLOW)Generating protobuf bindings...$(RESET)"
	@which protoc > /dev/null || (echo "Install protoc: apt install protobuf-compiler" && exit 1)
	@which buf > /dev/null || (echo "Install buf: https://buf.build/docs/installation" && exit 1)
	buf generate contracts/grpc
	@echo "$(GREEN)✅ Proto generation complete$(RESET)"

# ─── Admin Dashboard (Next.js) ──────────────────────────────────────────────────
.PHONY: admin-install
admin-install: ## Install admin-dashboard dependencies
	cd apps/admin-dashboard && npm install

.PHONY: admin-dev
admin-dev: ## Start admin-dashboard dev server
	cd apps/admin-dashboard && npm run dev

.PHONY: partner-install
partner-install: ## Install partner-portal dependencies
	cd apps/partner-portal && npm install

.PHONY: partner-dev
partner-dev: ## Start partner-portal dev server
	cd apps/partner-portal && npm run dev

# ─── Docker ────────────────────────────────────────────────────────────────────
.PHONY: docker-build
docker-build: ## Build all Docker images
	docker compose build

.PHONY: docker-push
docker-push: ## Push images to registry (requires REGISTRY env)
	docker compose push

# ─── Kubernetes ────────────────────────────────────────────────────────────────
.PHONY: k8s-apply-staging
k8s-apply-staging: ## Apply staging Kubernetes manifests
	kubectl apply -k deployments/k8s/staging

.PHONY: k8s-apply-production
k8s-apply-production: ## Apply production Kubernetes manifests
	kubectl apply -k deployments/k8s/production

.PHONY: k8s-diff-production
k8s-diff-production: ## Diff production manifests against cluster
	kubectl diff -k deployments/k8s/production

# ─── Clean ──────────────────────────────────────────────────────────────────────
.PHONY: clean
clean: ## Remove build artifacts
	cargo clean
	rm -rf apps/admin-dashboard/.next apps/partner-portal/.next
	rm -rf coverage/

.PHONY: nuke
nuke: clean infra-down ## Full clean including Docker volumes
	docker compose down -v
