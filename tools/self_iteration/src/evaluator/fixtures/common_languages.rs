pub(super) const PYTHON_OPERATIONS_MD: &str = r#"# Syntax service operations

The ServiceRunner class owns the async dispatch lifecycle for production workers.
The dispatch_event function normalizes payload text before writing event records.
"#;

pub(super) const PYTHON_INIT: &str = r#""#;

pub(super) const PYTHON_DECORATORS: &str = r#"
def traced_operation(name):
    def wrap(func):
        async def inner(*args, **kwargs):
            return await func(*args, **kwargs)
        inner.operation_name = name
        return inner
    return wrap
"#;

pub(super) const PYTHON_ERRORS: &str = r#"
class ServiceError(RuntimeError):
    pass


class OverloadedServiceError(ServiceError):
    pass
"#;

pub(super) const PYTHON_SERVICE: &str = r#"
from .decorators import traced_operation
from .errors import OverloadedServiceError, ServiceError


class AsyncResource:
    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def write_event(self, event):
        return event["payload"]


class ServiceRunner:
    def __init__(self, resource):
        self.resource = resource
        self.payload_filter = lambda value: value.strip()

    @traced_operation("dispatch")
    async def dispatch_event(self, event):
        async with self.resource as resource:
            payload = await resource.write_event(event)
            return self.normalize_payload(payload)

    def normalize_payload(self, payload):
        if payload == "overload":
            raise OverloadedServiceError("overload")
        return self.payload_filter(payload)


async def run_service(event):
    runner = ServiceRunner(AsyncResource())
    return await runner.dispatch_event(event)
"#;

pub(super) const PYTHON_FAKE_SERVICE: &str = r#"
class ServiceRunner:
    def dispatch_event(self, event):
        return event
"#;

pub(super) const JAVASCRIPT_RUNTIME: &str = r#"
import { createRegistry } from "./registry.js";

export class RuntimeController {
  constructor(registry = createRegistry()) {
    this.registry = registry;
  }

  async dispatchEvent(event) {
    const handler = this.registry.resolve(event.type);
    return handler(event.payload);
  }
}

export async function runRuntime(events) {
  const controller = new RuntimeController();
  return Promise.all(events.map((event) => controller.dispatchEvent(event)));
}
"#;

pub(super) const JAVASCRIPT_REGISTRY: &str = r#"
export function createRegistry() {
  const handlers = new Map();
  const payloadPipeline = (payload) => normalizePayload(payload);
  handlers.set("write", payloadPipeline);
  return {
    resolve(type) {
      return handlers.get(type) ?? missingHandler;
    },
  };
}

export function normalizePayload(payload) {
  return String(payload).trim();
}

function missingHandler(payload) {
  throw new Error(`missing handler ${payload}`);
}
"#;

pub(super) const JAVASCRIPT_INDEX: &str = r#"
export { RuntimeController, runRuntime } from "./runtime.js";
export { createRegistry, normalizePayload } from "./registry.js";
"#;

pub(super) const JAVASCRIPT_FAKE_RUNTIME: &str = r#"
export class RuntimeController {
  dispatchEvent(event) {
    return event;
  }
}
"#;

pub(super) const TYPESCRIPT_PROTOCOL: &str = r#"
export interface StreamTransport<TEvent> {
  send(event: TEvent): Promise<void>;
}

export type StreamEnvelope<TPayload> = {
  id: string;
  payload: TPayload;
};

export type PayloadProjector<TPayload> = (payload: TPayload) => TPayload;

export const trimPayload: PayloadProjector<string> = (payload) => payload.trim();

export async function sendEnvelope<TPayload>(
  transport: StreamTransport<StreamEnvelope<TPayload>>,
  payload: TPayload,
): Promise<StreamEnvelope<TPayload>> {
  const envelope = { id: "syntax-envelope", payload };
  await transport.send(envelope);
  return envelope;
}
"#;

pub(super) const TYPESCRIPT_PROVIDER: &str = r#"
import type { StreamEnvelope, StreamTransport } from "./protocol";
import { sendEnvelope } from "./protocol";
import { trimPayload } from "./protocol";

export class ProviderRuntime implements StreamTransport<StreamEnvelope<string>> {
  async send(event: StreamEnvelope<string>): Promise<void> {
    await import("./protocol");
    this.record(event.payload);
  }

  record(payload: string): string {
    return trimPayload(payload);
  }
}

export async function runProvider(payload: string): Promise<StreamEnvelope<string>> {
  const runtime = new ProviderRuntime();
  return sendEnvelope(runtime, payload);
}
"#;

pub(super) const TYPESCRIPT_COMPONENT: &str = r#"
import React from "react";
import { runProvider } from "./provider";

export function ProviderPanel({ value }: { value: string }) {
  const [state, setState] = React.useState(value);
  React.useEffect(() => {
    runProvider(state).then((envelope) => setState(envelope.payload));
  }, [state]);
  return <section data-provider={state}>{state}</section>;
}
"#;

pub(super) const TYPESCRIPT_INDEX: &str = r#"
export type { StreamEnvelope, StreamTransport } from "./protocol";
export { ProviderRuntime, runProvider } from "./provider";
export { ProviderPanel } from "./component";
"#;

pub(super) const TYPESCRIPT_FAKE_PROVIDER: &str = r#"
export class ProviderRuntime {
  record(payload: string): string {
    return payload;
  }
}
"#;

pub(super) const GO_MOD: &str = r#"module example.com/syntax

go 1.22
"#;

pub(super) const GO_WORKER: &str = r#"
package processor

import (
    ctxalias "context"
    _ "embed"
    . "strings"
)

type EventProcessor interface {
    Process(ctx ctxalias.Context, event Event) error
}

type Event struct {
    Payload string
}

type Worker struct {
    processor EventProcessor
}

func NewWorker(processor EventProcessor) *Worker {
    return &Worker{processor: processor}
}

func (w *Worker) Run(ctx ctxalias.Context, events []Event) error {
    for _, event := range events {
        if err := w.processor.Process(ctx, event); err != nil {
            return err
        }
        _ = TrimSpace(event.Payload)
    }
    return nil
}
"#;

pub(super) const GO_PIPELINE: &str = r#"
package processor

import "context"

type PipelineProcessor struct{}

func (PipelineProcessor) Process(ctx context.Context, event Event) error {
    done := make(chan struct{})
    notify := func(payload string) string {
        return payload
    }
    go func() {
        defer close(done)
        _ = notify(event.Payload)
    }()
    <-done
    return ctx.Err()
}

func RunPipeline(events []Event) error {
    worker := NewWorker(PipelineProcessor{})
    return worker.Run(context.Background(), events)
}
"#;

pub(super) const GO_FAKE_WORKER: &str = r#"
package tests

type Worker struct{}

func (Worker) Run() {}
"#;

pub(super) const JAVA_SERVICE_CONTRACT: &str = r#"
package example;

public interface ServiceContract<T> {
    default T normalize(T value) {
        return value;
    }

    T handle(T value);
}
"#;

pub(super) const JAVA_ANNOTATED_SERVICE: &str = r#"
package example;

@Deprecated
public class AnnotatedService implements ServiceContract<String> {
    public AnnotatedService() {}

    @Override
    public String handle(String value) {
        return normalize(value).trim();
    }

    public static class Builder {
        public AnnotatedService build() {
            return new AnnotatedService();
        }
    }
}
"#;

pub(super) const JAVA_SERVICE_FACTORY: &str = r#"
package example;

import example.AnnotatedService.Builder;
import java.util.function.Function;

public final class ServiceFactory {
    public ServiceContract<String> create() {
        Builder builder = new Builder();
        return builder.build();
    }

    public String dispatch(String value) {
        Function<String, String> transformer = ignored -> create().handle(value);
        return transformer.apply(value);
    }
}
"#;

pub(super) const JAVA_FAKE_SERVICE: &str = r#"
package example;

class FakeService {
    String handle(String value) {
        return value;
    }
}
"#;

pub(super) const RUST_LIB: &str = r#"
pub mod model;
pub mod service;

pub use service::{EventHandler, RuntimeService};
"#;

pub(super) const RUST_MODEL: &str = r#"
pub enum RuntimeEvent {
    Start(String),
    Stop,
}
"#;

pub(super) const RUST_SERVICE: &str = r#"
use crate::model::RuntimeEvent;

macro_rules! trace_event {
    ($event:expr) => {
        format!("trace::{:?}", $event)
    };
}

pub trait EventHandler {
    fn handle_event(&self, event: RuntimeEvent) -> String;
}

pub struct RuntimeService;

impl RuntimeService {
    pub fn new() -> Self {
        Self
    }

    pub fn dispatch(&self, event: RuntimeEvent) -> String {
        let invoke = |event| self.handle_event(event);
        invoke(event)
    }
}

impl EventHandler for RuntimeService {
    fn handle_event(&self, event: RuntimeEvent) -> String {
        match event {
            RuntimeEvent::Start(payload) => payload,
            RuntimeEvent::Stop => trace_event!(RuntimeEvent::Stop),
        }
    }
}
"#;

pub(super) const RUST_FAKE_SERVICE: &str = r#"
struct RuntimeService;

impl RuntimeService {
    fn dispatch(&self) {}
}
"#;

pub(super) const BASH_INSTALL: &str = r#"
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib/runtime.sh
. "$SCRIPT_DIR/../lib/runtime.sh"

rk_install_main() {
  local command="${1:-install}"
  case "$command" in
    install) rk_runtime_dispatch "install" ;;
    doctor) rk_runtime_dispatch "doctor" ;;
    *) rk_missing_command "$command" ;;
  esac
}

rk_install_main "$@"
"#;

pub(super) const BASH_RUNTIME: &str = r#"
rk_runtime_dispatch() {
  local mode="$1"
  rk_prepare_home "$mode"
  rk_download_artifact "$mode"
}

rk_prepare_home() {
  mkdir -p "${RK_HOME:-$HOME/.relay-knowledge}/$1"
}

rk_download_artifact() {
  printf 'download:%s\n' "$1"
}

rk_missing_command() {
  printf 'missing:%s\n' "$1" >&2
  return 64
}
"#;

pub(super) const BASH_FAKE_RUNTIME: &str = r#"
rk_runtime_dispatch() {
  echo fake
}
"#;

pub(super) const CSHARP_BUFFER_POOL: &str = r#"
using System;
using System.Buffers;

namespace Syntax.Runtime;

public interface IBufferSink<T>
{
    void Write(T item);
}

public sealed class BufferPoolSink : IBufferSink<byte[]>
{
    public void Write(byte[] item)
    {
        ArrayPool<byte>.Shared.Return(item);
    }

    public byte[] RentBuffer(int size)
    {
        return ArrayPool<byte>.Shared.Rent(size);
    }
}
"#;

pub(super) const CSHARP_RUNTIME_SERVICE: &str = r#"
using System;
using Syntax.Runtime;

namespace Syntax.Runtime;

public sealed class RuntimeService
{
    private readonly BufferPoolSink sink = new();

    public void Dispatch(int size)
    {
        var buffer = sink.RentBuffer(size);
        Func<byte[], byte[]> returnBuffer = rented => rented;
        sink.Write(returnBuffer(buffer));
    }
}
"#;

pub(super) const CSHARP_FAKE_SERVICE: &str = r#"
namespace Syntax.Runtime.Tests;

public sealed class RuntimeService
{
    public void Dispatch() {}
}
"#;
