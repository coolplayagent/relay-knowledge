const SOFTWARE_GLOBAL_CARGO_TOML: &str = r#"
[package]
name = "software-global-fixture"
version = "0.1.0"
edition = "2021"

[features]
observability = []

[dependencies]
serde = "1"
"#;

const SOFTWARE_GLOBAL_CARGO_LOCK: &str = r#"
version = 3

[[package]]
name = "tokio"
version = "1.36.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

const SOFTWARE_GLOBAL_LIB_RS: &str = r#"
use serde::Serialize;

pub struct Config;

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

const SOFTWARE_GLOBAL_SDK_PROBE_C: &str = r#"
#include <securec.h>

int relay_secure_copy(void *dst, unsigned int dst_len, const void *src, unsigned int src_len)
{
    return memcpy_s(dst, dst_len, src, src_len);
}
"#;

const SOFTWARE_GLOBAL_PACKAGE_JSON: &str = r#"
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

const SOFTWARE_GLOBAL_APP_JS: &str = r#"
import React from "react";

export function RelayGlobalPanel() {
  return React.createElement("section", null, "relay global");
}
"#;

const SOFTWARE_GLOBAL_GO_MOD: &str = r#"
module example.com/relay/global

go 1.22
"#;

const SOFTWARE_GLOBAL_CMAKE: &str = r#"
cmake_minimum_required(VERSION 3.20)
project(relay_global_agent)
add_executable(relay_global_agent src/sdk_probe.c)
"#;

const SOFTWARE_GLOBAL_MAKEFILE: &str = r#"
package:
	cargo package

diagnostics:
	cargo test
"#;

const SOFTWARE_GLOBAL_WORKFLOW: &str = r#"
name: Relay Global CI
on: [push]
jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test
"#;

const SOFTWARE_GLOBAL_DOCKERFILE: &str = r#"
FROM rust:1.76
EXPOSE 8080
"#;

const SOFTWARE_GLOBAL_COMPOSE: &str = r#"
services:
  web:
    image: relay/global-web:latest
"#;

const SOFTWARE_GLOBAL_K8S: &str = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: relay-global-api
"#;

const SOFTWARE_GLOBAL_TERRAFORM: &str = r#"
provider "aws" {}
resource "aws_ecs_service" "relay_global" {}
module "network" {}
"#;

const SOFTWARE_GLOBAL_SYSTEMD: &str = r#"
[Unit]
Description=Relay global fixture

[Service]
ExecStart=/usr/bin/relay-global service run
"#;

const SOFTWARE_GLOBAL_ARCHITECTURE_MD: &str = r#"
# Architecture
Relay global separates repository indexing from software projection serving.

## Module relay-core
Owns software projection refresh and lifecycle extraction.

## Capability Global software projection
Provides dependency, SDK, build, IaC, and design context for generation.
"#;

const SOFTWARE_GLOBAL_KNOWLEDGE_MAP: &str = r#"
schema_version: 1
map_version: 1
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
"#;

const SOFTWARE_GLOBAL_FLAGS_YAML: &str = r#"
payments:
  enabled: true
"#;

const SOFTWARE_GLOBAL_SMOKE_RS: &str = r#"
#[test]
fn relay_global_smoke() {
    assert!(true);
}
"#;

const SOFTWARE_GLOBAL_TEMPLATE: &str = r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: relay-global-template
"#;
