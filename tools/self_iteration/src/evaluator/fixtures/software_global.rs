pub(super) const SOFTWARE_GLOBAL_CARGO_TOML: &str = r#"
[package]
name = "software-global-fixture"
version = "0.1.0"
edition = "2021"

[features]
observability = []

[dependencies]
serde = "1"
"#;

pub(super) const SOFTWARE_GLOBAL_CARGO_LOCK: &str = r#"
version = 3

[[package]]
name = "tokio"
version = "1.36.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

pub(super) const SOFTWARE_GLOBAL_LIB_RS: &str = r#"
use serde::Serialize;

pub struct Config;

pub trait GraphApi {
    fn graph_version(&self) -> u64;
}

impl Config {
    pub fn get_bool(&self, key: &str) -> bool {
        key == "payments.enabled"
    }
}

#[derive(Serialize)]
pub struct CheckoutEvent {
    pub enabled: bool,
}

pub fn checkout_enabled(config: &Config) -> bool {
    config.get_bool("payments.enabled")
}
"#;

pub(super) const SOFTWARE_GLOBAL_SDK_PROBE_C: &str = r#"
#include <securec.h>

int relay_secure_copy(void *dst, unsigned int dst_len, const void *src, unsigned int src_len)
{
    return memcpy_s(dst, dst_len, src, src_len);
}
"#;

pub(super) const SOFTWARE_GLOBAL_PACKAGE_JSON: &str = r#"
{
  "name": "relay-global-web",
  "version": "0.1.0",
  "scripts": {
    "build": "vite build",
    "test": "vitest run"
  },
  "dependencies": {
    "react": "^18.2.0"
  }
}
"#;

pub(super) const SOFTWARE_GLOBAL_APP_JS: &str = r#"
import React from "react";

export function RelayGlobalPanel() {
  return React.createElement("section", null, "relay global");
}
"#;

pub(super) const SOFTWARE_GLOBAL_GO_MOD: &str = r#"
module example.com/relay/global

go 1.22
"#;

pub(super) const SOFTWARE_GLOBAL_CMAKE: &str = r#"
cmake_minimum_required(VERSION 3.20)
project(relay_global_agent)
add_executable(relay_global_agent src/sdk_probe.c)
"#;

pub(super) const SOFTWARE_GLOBAL_MAKEFILE: &str = r#"
package:
	cargo package

diagnostics:
	cargo test
"#;

pub(super) const SOFTWARE_GLOBAL_WORKFLOW: &str = r#"
name: Relay Global CI
on: [push]
jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test
"#;

pub(super) const SOFTWARE_GLOBAL_DOCKERFILE: &str = r#"
FROM rust:1.76
EXPOSE 8080
"#;

pub(super) const SOFTWARE_GLOBAL_COMPOSE: &str = r#"
services:
  web:
    image: relay/global-web:latest
"#;

pub(super) const SOFTWARE_GLOBAL_K8S: &str = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: relay-global-api
"#;

pub(super) const SOFTWARE_GLOBAL_TERRAFORM: &str = r#"
provider "aws" {}
resource "aws_ecs_service" "relay_global" {}
module "network" {}
"#;

pub(super) const SOFTWARE_GLOBAL_SYSTEMD: &str = r#"
[Unit]
Description=Relay global fixture

[Service]
ExecStart=/usr/bin/relay-global service run
"#;

pub(super) const SOFTWARE_GLOBAL_ARCHITECTURE_MD: &str = r#"
# Architecture
Relay global separates repository indexing from software projection serving.

## Module relay-core
Owns software projection refresh and lifecycle extraction.

## Capability Global software projection
Provides dependency, SDK, build, IaC, and design context for generation.
"#;

pub(super) const SOFTWARE_GLOBAL_README_MD: &str = r#"
# Getting Started

## Chapter Index
This heading is documentation and must not become a software system.
"#;

pub(super) const SOFTWARE_GLOBAL_CATALOG_MD: &str = r#"

---
software-system: relay-platform
api: Catalog API
---
# Catalog Guide
Explicit catalog metadata may promote controlled software entities.
"#;

pub(super) const SOFTWARE_GLOBAL_KNOWLEDGE_MAP: &str = r#"
schema_version: 1
map_version: 1
updated_at: unix:0
topics:
- id: global-runtime
  title: Global runtime knowledge
  description: Runtime and software projection routing.
sources:
- id: global-runtime-doc
  topic: global-runtime
  kind: doc
  uri: docs/architecture.md
  source_scope: docs
  read_policy: direct
  write_policy: manual-review
  status: active
  version: 1
routes:
- topic: global-runtime
  source_order:
  - global-runtime-doc
  fallback: bounded-search
history:
- version: 1
  action: init
  actor: fixture
  summary: Created software projection knowledge route.
"#;

pub(super) const SOFTWARE_GLOBAL_FLAGS_YAML: &str = r#"
payments:
  enabled: true
"#;

pub(super) const SOFTWARE_GLOBAL_SMOKE_RS: &str = r#"
#[test]
fn relay_global_smoke() {
    assert!(true);
}
"#;

pub(super) const SOFTWARE_GLOBAL_TEMPLATE: &str = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: relay-global-template
"#;

pub(super) const SOFTWARE_GLOBAL_OPENAPI: &str = r#"
openapi: 3.1.0
info:
  title: Relay Global API
  version: 1.0.0
paths: {}
"#;
