"""Shared JSON Schema contract checks for the relay-knowledge CLI skill."""

from __future__ import annotations

import copy
import json
import re
import tempfile
from pathlib import Path
from typing import Callable


SCHEMA_DRAFT = "https://json-schema.org/draft/2020-12/schema"
BUSINESS_GLOSSARY_REQUIRED_DEFS = (
    "text128",
    "text1024",
    "text32768",
    "nullableText128",
    "nullableText1024",
    "nullableText32768",
    "domain",
    "termStatus",
    "aliasKind",
    "mappingRelation",
    "technicalTargetKind",
    "alias",
    "semantics",
    "mapping",
    "term",
)
BUSINESS_TERM_STATUSES = ("active", "deprecated")
BUSINESS_ALIAS_KINDS = ("synonym", "abbreviation")
BUSINESS_MAPPING_RELATIONS = ("represented_by", "calculated_from")
TECHNICAL_TARGET_KINDS = (
    "file",
    "symbol",
    "config_key",
    "api",
    "software_component",
    "build_target",
    "iac",
    "design_element",
    "database_table",
    "database_column",
    "metric",
    "external",
)


def load_schema(path: Path) -> dict[str, object]:
    try:
        schema = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ValueError(f"{path} is missing") from error
    except json.JSONDecodeError as error:
        raise ValueError(f"{path} is not valid JSON: {error.msg}") from error
    if not isinstance(schema, dict):
        raise ValueError(f"{path} must contain a JSON object")
    return schema


def require_schema_value(
    path: Path,
    actual: object,
    expected: object,
    contract: str,
) -> None:
    if actual != expected:
        raise ValueError(f"{path} has invalid schema {contract}")


def schema_object_nodes(value: object):
    if isinstance(value, dict):
        expected_type = value.get("type")
        if expected_type == "object" or (
            isinstance(expected_type, list) and "object" in expected_type
        ):
            yield value
        for child in value.values():
            yield from schema_object_nodes(child)
    elif isinstance(value, list):
        for child in value:
            yield from schema_object_nodes(child)


def schema_property(definition: object, name: str) -> dict[str, object]:
    if not isinstance(definition, dict):
        return {}
    properties = definition.get("properties")
    if not isinstance(properties, dict):
        return {}
    value = properties.get(name)
    return value if isinstance(value, dict) else {}


def _instance_type_matches(expected: object, instance: object) -> bool:
    if isinstance(expected, list):
        return any(_instance_type_matches(item, instance) for item in expected)
    return {
        "object": isinstance(instance, dict),
        "array": isinstance(instance, list),
        "string": isinstance(instance, str),
        "integer": isinstance(instance, int) and not isinstance(instance, bool),
        "null": instance is None,
    }.get(expected, True)


def validate_schema_instance(
    schema: dict[str, object],
    instance: object,
    root: dict[str, object] | None = None,
    location: str = "$",
) -> None:
    """Validate the schema subset used by the two bundled skill contracts."""
    root = schema if root is None else root
    reference = schema.get("$ref")
    if isinstance(reference, str):
        prefix = "#/$defs/"
        definitions = root.get("$defs")
        if not reference.startswith(prefix) or not isinstance(definitions, dict):
            raise ValueError(f"{location} has unsupported schema reference {reference}")
        target = definitions.get(reference.removeprefix(prefix))
        if not isinstance(target, dict):
            raise ValueError(f"{location} has unresolved schema reference {reference}")
        validate_schema_instance(target, instance, root, location)
        return

    for keyword, required_matches in (("oneOf", 1), ("anyOf", None)):
        alternatives = schema.get(keyword)
        if not isinstance(alternatives, list):
            continue
        matches = 0
        for alternative in alternatives:
            if not isinstance(alternative, dict):
                continue
            try:
                validate_schema_instance(alternative, instance, root, location)
            except ValueError:
                continue
            matches += 1
        invalid_one_of = required_matches is not None and matches != required_matches
        invalid_any_of = required_matches is None and matches == 0
        if invalid_one_of or invalid_any_of:
            raise ValueError(f"{location} does not satisfy {keyword}")

    expected_type = schema.get("type")
    if expected_type is not None and not _instance_type_matches(expected_type, instance):
        raise ValueError(f"{location} has invalid type")
    if "const" in schema and instance != schema["const"]:
        raise ValueError(f"{location} does not match const")
    choices = schema.get("enum")
    if isinstance(choices, list) and instance not in choices:
        raise ValueError(f"{location} is not in enum")
    if isinstance(instance, str):
        if len(instance) < int(schema.get("minLength", 0)):
            raise ValueError(f"{location} is too short")
        if "maxLength" in schema and len(instance) > int(schema["maxLength"]):
            raise ValueError(f"{location} is too long")
        pattern = schema.get("pattern")
        if isinstance(pattern, str) and re.search(pattern, instance) is None:
            raise ValueError(f"{location} does not match pattern")
    if isinstance(instance, int) and not isinstance(instance, bool):
        if "minimum" in schema and instance < int(schema["minimum"]):
            raise ValueError(f"{location} is below minimum")
        if "maximum" in schema and instance > int(schema["maximum"]):
            raise ValueError(f"{location} is above maximum")
    if isinstance(instance, dict):
        required = schema.get("required", [])
        if isinstance(required, list) and any(name not in instance for name in required):
            raise ValueError(f"{location} is missing a required property")
        properties = schema.get("properties", {})
        if isinstance(properties, dict):
            for name, value in instance.items():
                subschema = properties.get(name)
                if isinstance(subschema, dict):
                    validate_schema_instance(subschema, value, root, f"{location}.{name}")
                elif schema.get("additionalProperties") is False:
                    raise ValueError(f"{location}.{name} is not allowed")
    if isinstance(instance, list):
        if len(instance) < int(schema.get("minItems", 0)):
            raise ValueError(f"{location} has too few items")
        if "maxItems" in schema and len(instance) > int(schema["maxItems"]):
            raise ValueError(f"{location} has too many items")
        if schema.get("uniqueItems") is True:
            serialized = [json.dumps(item, sort_keys=True) for item in instance]
            if len(serialized) != len(set(serialized)):
                raise ValueError(f"{location} has duplicate items")
        items = schema.get("items")
        if isinstance(items, dict):
            for index, value in enumerate(instance):
                validate_schema_instance(items, value, root, f"{location}[{index}]")


def _require_phrases(path: Path, text: str, phrases: tuple[str, ...]) -> None:
    normalized_text = " ".join(text.split())
    for phrase in phrases:
        if " ".join(phrase.split()) not in normalized_text:
            raise ValueError(f"{path} is missing schema boundary phrase: {phrase}")


def check_business_glossary_schema_contract(
    path: Path,
    schema: dict[str, object],
) -> None:
    require_schema_value(path, schema.get("$schema"), SCHEMA_DRAFT, "draft")
    require_schema_value(
        path,
        schema_property(schema, "schema_version").get("const"),
        1,
        "business glossary version",
    )
    definitions = schema.get("$defs")
    if not isinstance(definitions, dict):
        raise ValueError(f"{path} is missing business glossary schema $defs")
    for name in BUSINESS_GLOSSARY_REQUIRED_DEFS:
        if name not in definitions:
            raise ValueError(f"{path} is missing business glossary schema $defs/{name}")
    if any(node.get("additionalProperties") is not True for node in schema_object_nodes(schema)):
        raise ValueError(f"{path} must allow unknown fields on every object schema")

    for definition, expected in (
        (schema, ["schema_version"]),
        (definitions["domain"], ["id", "name"]),
        (
            definitions["term"],
            ["id", "domain", "canonical_name", "definition"],
        ),
        (definitions["alias"], ["value", "kind"]),
        (definitions["mapping"], ["relation", "target_kind", "target"]),
    ):
        required = definition.get("required") if isinstance(definition, dict) else None
        require_schema_value(path, required, expected, "required fields")

    require_schema_value(path, schema_property(schema, "domains").get("maxItems"), 256, "domain maximum")
    require_schema_value(path, schema_property(schema, "terms").get("maxItems"), 10000, "term maximum")
    require_schema_value(
        path,
        schema_property(definitions["term"], "aliases").get("maxItems"),
        32,
        "alias maximum",
    )
    require_schema_value(
        path,
        schema_property(definitions["term"], "mappings").get("maxItems"),
        64,
        "mapping maximum",
    )
    for boundary in ("includes", "excludes"):
        require_schema_value(
            path,
            schema_property(definitions["semantics"], boundary).get("maxItems"),
            256,
            f"semantics {boundary} maximum",
        )
    for definition, expected in (
        ("termStatus", BUSINESS_TERM_STATUSES),
        ("aliasKind", BUSINESS_ALIAS_KINDS),
        ("mappingRelation", BUSINESS_MAPPING_RELATIONS),
        ("technicalTargetKind", TECHNICAL_TARGET_KINDS),
    ):
        value = definitions[definition]
        enum = value.get("enum") if isinstance(value, dict) else None
        require_schema_value(path, enum, list(expected), f"{definition} enum")
    for definition, expected in (
        ("text128", 128),
        ("nullableText128", 128),
        ("text1024", 1024),
        ("nullableText1024", 1024),
        ("text32768", 32768),
        ("nullableText32768", 32768),
    ):
        value = definitions[definition]
        maximum = value.get("maxLength") if isinstance(value, dict) else None
        require_schema_value(path, maximum, expected, f"{definition} maximum")

    description = schema.get("description")
    if not isinstance(description, str):
        raise ValueError(f"{path} is missing business glossary boundary description")
    _require_phrases(
        path,
        description,
        (
            "allow unknown fields",
            "relay-knowledge map validate is authoritative",
            "intentionally authored and may be edited directly",
        ),
    )


def _business_glossary_example() -> dict[str, object]:
    return {
        "schema_version": 1,
        "domains": [
            {
                "id": "revenue",
                "name": "Revenue",
                "description": "Revenue reporting concepts.",
            }
        ],
        "terms": [
            {
                "id": "arr",
                "domain": "revenue",
                "canonical_name": "Annual recurring revenue",
                "definition": "Contracted recurring revenue normalized to one year.",
                "language": "en",
                "status": "active",
                "aliases": [
                    {"value": "ARR", "kind": "abbreviation", "language": "en"}
                ],
                "semantics": {
                    "formula": "monthly_recurring_revenue * 12",
                    "aggregation": "sum",
                    "unit": "currency",
                    "grain": "account",
                    "time_basis": "annualized",
                    "includes": ["recurring subscriptions"],
                    "excludes": ["one-time services"],
                },
                "mappings": [
                    {
                        "relation": "represented_by",
                        "target_kind": "metric",
                        "target": "annual_recurring_revenue",
                        "path": "src/metrics.rs",
                        "source_scope": "repo",
                    }
                ],
            }
        ],
    }


def _expect_value_error(action: Callable[[], object], expected: str) -> None:
    try:
        action()
    except ValueError as error:
        if expected not in str(error):
            raise AssertionError(f"expected {expected!r} in {error!s}") from error
    else:
        raise AssertionError("expected ValueError")


def check_business_glossary_schema_examples(schema: dict[str, object]) -> None:
    validate_schema_instance(schema, {"schema_version": 1})
    example = _business_glossary_example()
    validate_schema_instance(schema, example)
    extended = copy.deepcopy(example)
    extended["future_extension"] = True
    extended["terms"][0]["future_term_field"] = {"accepted": True}
    validate_schema_instance(schema, extended)

    invalid_examples = []
    for mutation in (
        lambda value: value.update(schema_version=2),
        lambda value: value["domains"][0].update(id=" "),
        lambda value: value["terms"][0].update(status="draft"),
        lambda value: value["terms"][0].pop("definition"),
        lambda value: value["terms"][0]["aliases"][0].update(kind="short_name"),
        lambda value: value["terms"][0]["mappings"][0].update(relation="links_to"),
        lambda value: value["terms"][0]["mappings"][0].update(target_kind="service"),
    ):
        invalid = copy.deepcopy(example)
        mutation(invalid)
        invalid_examples.append(invalid)
    oversized_aliases = copy.deepcopy(example)
    oversized_aliases["terms"][0]["aliases"] = [
        {"value": f"Alias {index}", "kind": "synonym"} for index in range(33)
    ]
    invalid_examples.append(oversized_aliases)
    oversized_mappings = copy.deepcopy(example)
    oversized_mappings["terms"][0]["mappings"] = [
        {"relation": "represented_by", "target_kind": "symbol", "target": f"target_{index}"}
        for index in range(65)
    ]
    invalid_examples.append(oversized_mappings)
    oversized_boundaries = copy.deepcopy(example)
    oversized_boundaries["terms"][0]["semantics"]["includes"] = [
        f"Boundary {index}" for index in range(257)
    ]
    invalid_examples.append(oversized_boundaries)
    for invalid in invalid_examples:
        _expect_value_error(
            lambda value=invalid: validate_schema_instance(schema, value),
            "$",
        )


def check_business_glossary_schema(path: Path) -> None:
    schema = load_schema(path)
    check_business_glossary_schema_contract(path, schema)
    check_business_glossary_schema_examples(schema)


def self_test_business_glossary_schema(path: Path) -> None:
    check_business_glossary_schema(path)
    schema = load_schema(path)
    with tempfile.TemporaryDirectory(prefix="relay-knowledge-glossary-schema-") as directory:
        temporary = Path(directory)
        _expect_value_error(
            lambda: check_business_glossary_schema(temporary / "missing.json"),
            "is missing",
        )
        corrupted = temporary / "corrupted.json"
        corrupted.write_text("{not-json", encoding="utf-8")
        _expect_value_error(
            lambda: check_business_glossary_schema(corrupted),
            "is not valid JSON",
        )
        drifted = copy.deepcopy(schema)
        del drifted["$defs"]["term"]
        drifted_path = temporary / "drifted.json"
        drifted_path.write_text(json.dumps(drifted), encoding="utf-8")
        _expect_value_error(
            lambda: check_business_glossary_schema(drifted_path),
            "$defs/term",
        )
