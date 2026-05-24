SHELL := /bin/bash

FRONTEND_DIR := frontend
TAURI_DIR := src-tauri
NPM := npm
CARGO := cargo

.DEFAULT_GOAL := help

.PHONY: help install dev build build-frontend build-tauri check check-frontend check-rust test fmt clean clean-frontend clean-rust windows-msi

help:
	@printf "autodns desktop targets:\n"
	@printf "  make install         Install frontend dependencies\n"
	@printf "  make dev             Run the Tauri development app\n"
	@printf "  make build           Build the Tauri desktop bundle\n"
	@printf "  make build-frontend  Build only the frontend\n"
	@printf "  make build-tauri     Build only the Rust/Tauri crate\n"
	@printf "  make check           Run frontend and Rust checks\n"
	@printf "  make test            Run Rust tests\n"
	@printf "  make fmt             Format Rust code\n"
	@printf "  make clean           Remove frontend and Rust build outputs\n"
	@printf "  make windows-msi     Build the Windows MSI installer on Windows\n"

install:
	$(NPM) --prefix $(FRONTEND_DIR) install

dev:
	cd $(TAURI_DIR) && $(CARGO) tauri dev

build: build-tauri

build-frontend:
	$(NPM) --prefix $(FRONTEND_DIR) run build

build-tauri:
	cd $(TAURI_DIR) && $(CARGO) tauri build

check: check-frontend check-rust

check-frontend:
	$(NPM) --prefix $(FRONTEND_DIR) run build

check-rust:
	$(CARGO) check --manifest-path $(TAURI_DIR)/Cargo.toml

test:
	$(CARGO) test --manifest-path $(TAURI_DIR)/Cargo.toml

fmt:
	$(CARGO) fmt --manifest-path $(TAURI_DIR)/Cargo.toml

clean: clean-frontend clean-rust

clean-frontend:
	rm -rf $(FRONTEND_DIR)/dist

clean-rust:
	$(CARGO) clean --manifest-path $(TAURI_DIR)/Cargo.toml

windows-msi:
	powershell -ExecutionPolicy Bypass -File scripts/build-windows-msi.ps1
