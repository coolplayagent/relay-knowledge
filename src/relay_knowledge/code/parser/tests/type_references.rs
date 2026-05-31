use crate::{
    code::parse_indexed_file,
    domain::{CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration},
};

use super::super::SnapshotBuild;

#[test]
fn python_type_annotations_are_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

class ConnectorService:
    async def save_w3_connector(
        self,
        request: W3ConnectorSaveRequest,
    ) -> W3ConnectorSaveResponse:
        pass
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest"
            && reference.kind == "type"
            && reference.line_range.start == 8
    }));
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveResponse"
            && reference.kind == "type"
            && reference.line_range.start == 9
    }));
    assert!(
        !snapshot
            .references
            .iter()
            .any(|reference| reference.name == "request"),
        "parameter names must not be indexed as type references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_quoted_forward_annotations_are_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

class ConnectorService:
    async def save_w3_connector(
        self,
        request: "List[W3ConnectorSaveRequest]",
    ) -> "W3ConnectorSaveResponse | None":
        pass
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest"
            && reference.kind == "type"
            && reference.line_range.start == 8
    }));
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveResponse"
            && reference.kind == "type"
            && reference.line_range.start == 9
    }));
    assert!(
        !snapshot.references.iter().any(|reference| {
            matches!(reference.name.as_str(), "List" | "Union" | "None") && reference.kind == "type"
        }),
        "quoted type-expression wrappers must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_typing_wrappers_are_not_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
from typing import Callable, Optional, TypeAlias, Union

class W3ConnectorSaveRequest:
    pass

class W3ConnectorSaveResponse:
    pass

ResponsePayload: TypeAlias = W3ConnectorSaveResponse

def load(
    request: Optional[W3ConnectorSaveRequest],
    callback: Callable[[W3ConnectorSaveRequest], W3ConnectorSaveResponse],
) -> Union[W3ConnectorSaveResponse, None]:
    pass
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        snapshot
            .references
            .iter()
            .any(|reference| reference.name == "W3ConnectorSaveRequest" && reference.kind == "type"),
        "wrapped request type should be indexed: {:?}",
        snapshot.references
    );
    assert!(
        snapshot.references.iter().any(|reference| {
            reference.name == "W3ConnectorSaveResponse" && reference.kind == "type"
        }),
        "wrapped response type should be indexed: {:?}",
        snapshot.references
    );
    assert!(
        !snapshot.references.iter().any(|reference| {
            matches!(
                reference.name.as_str(),
                "Callable" | "Optional" | "TypeAlias" | "Union" | "None"
            ) && reference.kind == "type"
        }),
        "typing wrapper names must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_literal_string_values_are_not_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
from typing import Literal

class W3ConnectorSaveRequest:
    pass

class W3ConnectorSaveResponse:
    pass

def load(
    status: Literal["READY"],
    request: "Literal['PENDING', 'DONE'] | W3ConnectorSaveRequest",
) -> W3ConnectorSaveResponse:
    pass
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest" && reference.kind == "type"
    }));
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveResponse" && reference.kind == "type"
    }));
    assert!(
        !snapshot.references.iter().any(|reference| {
            matches!(
                reference.name.as_str(),
                "Literal" | "READY" | "PENDING" | "DONE"
            ) && reference.kind == "type"
        }),
        "literal payload strings must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_local_typevars_are_not_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
from typing import TypeVar

T = TypeVar("T")
T_co = TypeVar("T_co", covariant=True)

class W3ConnectorSaveRequest:
    pass

def map_value(value: T, next_value: "T_co", request: W3ConnectorSaveRequest) -> T:
    return value
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest" && reference.kind == "type"
    }));
    assert!(
        !snapshot.references.iter().any(|reference| {
            matches!(reference.name.as_str(), "T" | "T_co") && reference.kind == "type"
        }),
        "local TypeVars must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_pep695_type_parameters_are_not_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

def map_value[T](
    value: T,
    request: W3ConnectorSaveRequest,
) -> T:
    return value
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest" && reference.kind == "type"
    }));
    assert!(
        !snapshot
            .references
            .iter()
            .any(|reference| { reference.name == "T" && reference.kind == "type" }),
        "PEP 695 type parameters must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn python_pep695_bounds_are_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/relay_teams/connector/service.py",
        br#"
class W3ConnectorSaveRequest:
    pass

class W3ConnectorSaveResponse:
    pass

def load[T: W3ConnectorSaveRequest](value: T) -> W3ConnectorSaveResponse:
    return value
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveRequest" && reference.kind == "type"
    }));
    assert!(snapshot.references.iter().any(|reference| {
        reference.name == "W3ConnectorSaveResponse" && reference.kind == "type"
    }));
    assert!(
        !snapshot
            .references
            .iter()
            .any(|reference| { reference.name == "T" && reference.kind == "type" }),
        "PEP 695 type parameter names must stay local while bounds remain references: {:?}",
        snapshot.references
    );
}

#[test]
fn typescript_type_annotations_are_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/session/session.ts",
        br#"
export interface InstanceContext {
  directory: string
}

export function plan(
  input: Record<string, InstanceContext>,
  instances: Array<InstanceContext>,
): Promise<InstanceContext> {
  const instance = instances[0]
  return Promise.resolve(instance)
}
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    let instance_references = snapshot
        .references
        .iter()
        .filter(|reference| reference.name == "InstanceContext" && reference.kind == "type")
        .count();
    assert!(
        instance_references >= 2,
        "parameter and return annotations should be indexed: {:?}",
        snapshot.references
    );
    assert!(
        !snapshot.references.iter().any(|reference| {
            reference.name == "InstanceContext" && reference.line_range.start == 2
        }),
        "interface definition names must remain definitions, not references"
    );
    assert!(
        !snapshot.references.iter().any(|reference| {
            matches!(reference.name.as_str(), "Array" | "Promise" | "Record")
                && reference.kind == "type"
        }),
        "TypeScript global generic wrappers must not become graph references: {:?}",
        snapshot.references
    );
}

#[test]
fn typescript_local_type_parameters_are_not_reference_facts() {
    let snapshot = parse_source_snapshot(
        "src/session/session.ts",
        br#"
export interface ExternalThing {
  id: string
}

export function map<TResult>(
  value: TResult,
): TResult {
  const next: TResult = value
  return next
}

export function wrap(value: ExternalThing): Promise<ExternalThing> {
  return Promise.resolve(value)
}
"#,
    );

    assert_eq!(snapshot.files[0].parse_status, CodeParseStatus::Parsed);
    assert!(
        !snapshot
            .references
            .iter()
            .any(|reference| { reference.name == "TResult" && reference.kind == "type" }),
        "local type parameters must not become graph reference facts: {:?}",
        snapshot.references
    );
    let external_references = snapshot
        .references
        .iter()
        .filter(|reference| reference.name == "ExternalThing" && reference.kind == "type")
        .count();
    assert_eq!(external_references, 2);
}

fn parse_source_snapshot(path: &str, source: &[u8]) -> CodeIndexSnapshot {
    let registration =
        CodeRepositoryRegistration::new("repo", "alias", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    let mut build = SnapshotBuild::new(
        &registration,
        "commit".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    );
    parse_indexed_file(&mut build, path, source).expect("file should parse");

    build.finish()
}
