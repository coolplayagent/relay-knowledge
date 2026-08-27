use crate::{
    code::{SnapshotBuild, parser::frameworks::FrameworkFileInput},
    domain::{CodeRepositoryRegistration, FrameworkEdgeKind, FrameworkKind, FrameworkNodeKind},
};

use super::extract;

#[test]
fn angular_component_and_template_bindings_become_framework_graph() {
    let source = r#"
import { Component, input, output } from '@angular/core';
@Component({
  selector: 'app-card',
  template: `@if (visible()) { <app-icon #iconRef [name]="icon" (click)="open()" /><ng-progress /> }`,
})
export class CardComponent {
  icon = input.required<string>();
  opened = output<void>();
  open() {}
}
"#;
    let build = build();
    let facts = extract(
        &build,
        FrameworkFileInput {
            path: "src/card.component.ts",
            file_id: "file-1",
            language_id: "typescript",
            content: source,
            symbols: &[],
        },
    )
    .unwrap();

    assert!(facts.nodes.iter().any(|node| {
        node.framework == FrameworkKind::Angular
            && node.kind == FrameworkNodeKind::Component
            && node.name == "CardComponent"
    }));
    assert!(
        facts
            .nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::ControlFlow)
    );
    assert!(facts.nodes.iter().any(|node| {
        node.kind == FrameworkNodeKind::TemplateVariable && node.name == "iconRef"
    }));
    assert!(
        facts
            .edges
            .iter()
            .any(|edge| edge.kind == FrameworkEdgeKind::Renders)
    );
    assert!(facts.edges.iter().any(|edge| {
        edge.kind == FrameworkEdgeKind::Renders
            && edge.target_hint.as_deref() == Some("ng-progress")
    }));
    assert!(
        facts
            .edges
            .iter()
            .any(|edge| edge.kind == FrameworkEdgeKind::HandlesOutput)
    );
}

#[test]
fn vue_sfc_extracts_macros_slots_and_component_usage() {
    let source = r#"<script setup lang="ts">
import CopyIcon from './CopyIcon.vue'
const props = defineProps<{ label: string }>()
const emit = defineEmits<{ change: [value: string] }>()
</script>
<template><CopyIcon v-for="item in items" :title="item.label" @click="emit('change', item.label)"/><slot name="footer"/></template>"#;
    let build = build();
    let facts = extract(
        &build,
        FrameworkFileInput {
            path: "src/VersionSelect.vue",
            file_id: "file-2",
            language_id: "vue",
            content: source,
            symbols: &[],
        },
    )
    .unwrap();

    assert!(
        facts
            .nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Component && node.name == "VersionSelect")
    );
    assert!(
        facts
            .nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Prop && node.name == "label")
    );
    assert!(
        facts
            .nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Slot && node.name == "footer")
    );
    assert!(
        facts.nodes.iter().any(|node| {
            node.kind == FrameworkNodeKind::TemplateVariable && node.name == "item"
        })
    );
    assert!(
        facts
            .edges
            .iter()
            .any(|edge| edge.kind == FrameworkEdgeKind::Renders
                && edge.target_hint.as_deref() == Some("CopyIcon"))
    );
}

#[test]
fn angular_external_template_is_not_misread_as_inline_template() {
    let source = r#"
@Component({
  selector: 'app-shell',
  templateUrl: './shell.component.html',
})
export class ShellComponent {}
"#;
    let build = build();
    let facts = extract(
        &build,
        FrameworkFileInput {
            path: "src/shell.component.ts",
            file_id: "file-3",
            language_id: "typescript",
            content: source,
            symbols: &[],
        },
    )
    .unwrap();

    assert!(
        !facts
            .nodes
            .iter()
            .any(|node| node.kind == FrameworkNodeKind::Template)
    );
    assert!(facts.edges.iter().any(|edge| {
        edge.kind == FrameworkEdgeKind::OwnsTemplate
            && edge.target_hint.as_deref() == Some("src/shell.component.html")
    }));
}

fn build() -> SnapshotBuild {
    SnapshotBuild::new(
        &CodeRepositoryRegistration::new(
            "repository",
            "framework",
            "/tmp/framework",
            Vec::new(),
            Vec::new(),
        )
        .unwrap(),
        "HEAD".to_owned(),
        "tree".to_owned(),
        true,
        1,
        0,
    )
}
