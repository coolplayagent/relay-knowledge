const KOTLIN_CLIENT: &str = r#"
package example

import kotlin.time.Duration

typealias RequestHandler = (String) -> String

object ClientRegistry {
    fun defaultHandler(): RequestHandler = { value -> value.trim() }
}

class SyntaxClient(private val handler: RequestHandler = ClientRegistry.defaultHandler()) {
    fun newCall(request: String): String {
        return handler(request)
    }

    companion object {
        fun withTimeout(timeout: Duration): SyntaxClient {
            return SyntaxClient { value -> "$timeout:$value" }
        }
    }
}
"#;

const KOTLIN_PIPELINE: &str = r#"
package example

fun runClientPipeline(values: List<String>): List<String> {
    val client = SyntaxClient()
    return values.map { value -> client.newCall(value) }
}
"#;

const KOTLIN_FAKE_CLIENT: &str = r#"
package example

class SyntaxClient {
    fun newCall(): String = "fake"
}
"#;

const PHP_KERNEL: &str = r#"<?php
namespace App;

use App\Contracts\Bootable;
use App\Providers\CacheProvider;

final class Kernel implements Bootable
{
    public function __construct(private CacheProvider $provider) {}

    public function boot(): void
    {
        $this->provider->register();
    }
}
"#;

const PHP_BOOTABLE: &str = r#"<?php
namespace App\Contracts;

interface Bootable
{
    public function boot(): void;
}
"#;

const PHP_CACHE_PROVIDER: &str = r#"<?php
namespace App\Providers;

trait LogsBoot
{
    public function logBoot(string $name): string
    {
        $normalizer = fn(string $value): string => trim($value);
        return $normalizer($name);
    }
}

final class CacheProvider
{
    use LogsBoot;

    public function register(): void
    {
        $this->logBoot('cache');
    }
}
"#;

const PHP_FAKE_KERNEL: &str = r#"<?php
namespace Tests;

final class Kernel
{
    public function boot(): void {}
}
"#;

const RUBY_CONTROLLER: &str = r#"
require_relative "extensions"

module App
  class Controller
    include Extensions

    def self.build
      new(Runtime.new)
    end

    def initialize(runtime)
      @runtime = runtime
    end

    def dispatch(event)
      normalize_event(@runtime.handle(event))
    end
  end
end
"#;

const RUBY_EXTENSIONS: &str = r#"
module App
  module Extensions
    def normalize_event(event)
      event.to_s.strip
    end
  end
end
"#;

const RUBY_RUNTIME: &str = r#"
module App
  class Runtime
    def handle(event)
      normalizer = ->(payload) { payload.to_s.strip }
      normalizer.call(event)
    end
  end
end
"#;

const RUBY_FAKE_CONTROLLER: &str = r#"
class Controller
  def dispatch(event)
    event
  end
end
"#;

const SCALA_PIPELINE: &str = r#"
package example

import example.Runtime.Event

trait Stage:
  def run(event: Event): Event

object Pipeline:
  inline def identityStage: Stage = new Stage:
    def run(event: Event): Event = event

  def execute(events: List[Event]): List[Event] =
    val invoke: Event => Event = event => identityStage.run(event)
    events.map(invoke)
"#;

const SCALA_RUNTIME: &str = r#"
package example

object Runtime:
  case class Event(payload: String)

class RuntimeService(stage: Stage):
  def dispatch(event: Runtime.Event): Runtime.Event =
    stage.run(event)
"#;

const SCALA_FAKE_PIPELINE: &str = r#"
package example

object Pipeline:
  def execute(): Unit = ()
"#;

const SWIFT_SESSION_CLIENT: &str = r#"
import Foundation

protocol SessionTransport {
    func send(_ request: URLRequest) async throws -> Data
}

final class SessionClient {
    private let transport: SessionTransport

    init(transport: SessionTransport) {
        self.transport = transport
    }

    func request(url: URL) async throws -> Data {
        let request = URLRequest(url: url)
        return try await transport.send(request)
    }
}
"#;

const SWIFT_REQUEST_PIPELINE: &str = r#"
import Foundation

struct RequestPipeline {
    let client: SessionClient

    func dispatch(urls: [URL]) async throws -> [Data] {
        let request = { (url: URL) async throws -> Data in
            try await client.request(url: url)
        }
        var output: [Data] = []
        for url in urls {
            output.append(try await request(url))
        }
        return output
    }
}
"#;

const SWIFT_FAKE_SESSION_CLIENT: &str = r#"
import Foundation

final class SessionClient {
    func request() {}
}
"#;

const CONFIG_DOCUMENT_README_MD: &str = r#"# Runtime Guide

Install Notes
=============

[local install](docs/reference.md#install)
![runtime diagram](assets/runtime.png)

[ref]: docs/reference.md "Reference"

```md
# Disabled Fixture Heading
[disabled](docs/disabled.md)
```
"#;

const CONFIG_DOCUMENT_REFERENCE_MD: &str = r#"# Reference

## Install

The install reference backs local Markdown import extraction.
"#;

const CONFIG_DOCUMENT_SERVICE_CONF: &str = r#"[server]
enabled=true
port=8080

[server.tls]
cert=server.pem
"#;

const CONFIG_DOCUMENT_RUNTIME_JSON: &str = r#"{
  "server": {
    "port": 8080
  },
  "containers": [
    {
      "name": "app"
    }
  ],
  "matrix": [
    [
      {
        "name": "nested"
      }
    ]
  ]
}
"#;
