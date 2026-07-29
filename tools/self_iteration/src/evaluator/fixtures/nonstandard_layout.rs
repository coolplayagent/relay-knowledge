pub(super) const NONSTANDARD_PYTHON_SESSION_CLIENT: &str = r#"
class ExternalPythonSessionClient:
    def open_external_session(self, payload):
        return f"python-session:{payload}"
"#;

pub(super) const NONSTANDARD_TYPESCRIPT_SESSION_CLIENT: &str = r#"
export class ExternalTypeScriptSessionClient {
  openExternalSession(payload: string): string {
    return `typescript-session:${payload}`;
  }
}
"#;

pub(super) const NONSTANDARD_GO_SESSION_CLIENT: &str = r#"
package session

import "context"

type ExternalGoSessionClient interface {
    OpenExternalSession(ctx context.Context, payload string) error
}
"#;

pub(super) const NONSTANDARD_JAVA_SESSION_CLIENT: &str = r#"
package example;

public class ExternalJavaSessionClient {
    public String openExternalSession(String payload) {
        return "java-session:" + payload;
    }
}
"#;

pub(super) const NONSTANDARD_CPP_SESSION_CLIENT_HPP: &str = r#"#pragma once

namespace nonstandard {

class ExternalCppSessionClient {
public:
    void openExternalSession();
};

void external_session_client();

}  // namespace nonstandard
"#;

pub(super) const NONSTANDARD_CPP_SESSION_CLIENT_CPP: &str = r#"#include <external_session_client.hpp>

namespace nonstandard {

void ExternalCppSessionClient::openExternalSession() {
    external_session_client();
}

void external_session_client() {}

}  // namespace nonstandard
"#;

pub(super) const NONSTANDARD_SWIFT_SESSION_CLIENT: &str = r#"
import Foundation

final class ExternalSwiftSessionClient {
    func openExternalSession(payload: String) -> String {
        "swift-session:\(payload)"
    }
}
"#;

pub(super) const NONSTANDARD_APPLICATION_TS: &str = r#"
import { ExternalTypeScriptSessionClient } from "ts_sdk/sessionClient";

export function runExternalSessionWorkflow(payload: string): string {
  const client = new ExternalTypeScriptSessionClient();
  return client.openExternalSession(payload);
}
"#;

pub(super) const NONSTANDARD_CARGO_TOML: &str = r#"
[package]
name = "nonstandard-layout-fixture"
version = "0.1.0"

[dependencies]
serde = "1.0"
"#;

pub(super) const NONSTANDARD_CARGO_LOCK: &str = r#"
version = 3

[[package]]
name = "tokio"
version = "1.36.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

pub(super) const NONSTANDARD_PACKAGE_JSON: &str = r#"
{
  "name": "nonstandard-layout-fixture",
  "version": "0.1.0",
  "dependencies": {
    "react": "^18.2.0"
  }
}
"#;

pub(super) const NONSTANDARD_PACKAGE_LOCK_JSON: &str = r#"
{
  "lockfileVersion": 3,
  "packages": {
    "node_modules/vite": {
      "version": "5.1.0"
    }
  }
}
"#;

pub(super) const NONSTANDARD_GO_MOD: &str = r#"
module example.com/nonstandard

go 1.22

require google.golang.org/grpc v1.62.0
"#;

pub(super) const NONSTANDARD_PYPROJECT_TOML: &str = r#"
[project]
name = "nonstandard-layout-fixture"
dependencies = [
  "requests>=2.31",
]
"#;

pub(super) const NONSTANDARD_POM_XML: &str = r#"
<project>
  <dependencyManagement>
    <dependencies>
      <dependency>
        <groupId>org.springframework.boot</groupId>
        <artifactId>spring-boot-dependencies</artifactId>
        <version>3.2.0</version>
        <type>pom</type>
        <scope>import</scope>
      </dependency>
    </dependencies>
  </dependencyManagement>
</project>
"#;

pub(super) const NONSTANDARD_BUILD_GRADLE_KTS: &str = r#"
plugins {
    java
}

dependencies {
    implementation("org.slf4j:slf4j-api:2.0.9")
}
"#;

pub(super) const NONSTANDARD_CONANFILE_TXT: &str = r#"
[requires]
zlib/1.2.13
"#;

pub(super) const NONSTANDARD_CONANFILE_PY: &str = r#"
from conan import ConanFile

class NonstandardConan(ConanFile):
    def requirements(self):
        self.requires("openssl/3.2.1")
"#;
