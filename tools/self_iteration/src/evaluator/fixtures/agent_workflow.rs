pub(super) const AGENT_WORKFLOW_CARGO_TOML: &str = r#"[package]
name = "agent-workflow-fixture"
version = "0.1.0"
edition = "2021"
"#;

pub(super) const AGENT_WORKFLOW_CORE_CONTEXT_RS: &str = r#"pub struct AgentContextPackBuilder {
    max_context_chars: usize,
    evidence_floor: usize,
}

impl AgentContextPackBuilder {
    pub fn new(max_context_chars: usize, evidence_floor: usize) -> Self {
        Self {
            max_context_chars,
            evidence_floor,
        }
    }

    pub fn build_context_packet(&self, request: &AgentWorkflowRequest) -> AgentContextPacket {
        let summary = format!(
            "{}:{}:{}",
            request.repository_alias, self.max_context_chars, self.evidence_floor
        );
        AgentContextPacket {
            summary,
            freshness_mode: request.freshness_mode.clone(),
        }
    }
}

pub struct AgentWorkflowRequest {
    pub repository_alias: String,
    pub freshness_mode: String,
}

pub struct AgentContextPacket {
    pub summary: String,
    pub freshness_mode: String,
}
"#;

pub(super) const AGENT_WORKFLOW_CORE_ORCHESTRATOR_RS: &str = r#"use crate::context::{AgentContextPackBuilder, AgentWorkflowRequest};

pub struct AgentWorkflowOrchestrator {
    context_builder: AgentContextPackBuilder,
}

impl AgentWorkflowOrchestrator {
    pub fn new(context_builder: AgentContextPackBuilder) -> Self {
        Self { context_builder }
    }

    pub fn analyze_issue_entrypoint(&self, request: &AgentWorkflowRequest) -> String {
        let packet = self.context_builder.build_context_packet(request);
        format!("{}:{}", packet.summary, packet.freshness_mode)
    }
}
"#;

pub(super) const AGENT_WORKFLOW_CORE_LIB_RS: &str = r#"pub mod context;
pub mod orchestrator;
"#;

pub(super) const AGENT_WORKFLOW_WEB_CONTEXT_TS: &str = r#"export type AgentEvidenceCard = {
  path: string;
  excerpt: string;
  retrievalLayer: string;
};

export function buildContextPacket(cards: AgentEvidenceCard[], maxContextChars: number): string {
  return cards
    .slice(0, 4)
    .map((card) => `${card.path}:${card.retrievalLayer}:${card.excerpt}`)
    .join("\n")
    .slice(0, maxContextChars);
}
"#;

pub(super) const AGENT_WORKFLOW_WEB_ENTRY_TS: &str = r#"import { buildContextPacket, AgentEvidenceCard } from "./contextPacket";

export function renderAgentWorkflowAnswer(cards: AgentEvidenceCard[]): string {
  return buildContextPacket(cards, 4096);
}
"#;

pub(super) const AGENT_WORKFLOW_OPS_POLICY_PY: &str = r#"AGENT_POLICY_BUDGET = {
    "max_tool_calls": 6,
    "max_source_reads": 8,
    "max_context_chars": 9000,
    "freshness": "wait-until-fresh",
}


def load_agent_policy(environment: str) -> dict[str, object]:
    policy = dict(AGENT_POLICY_BUDGET)
    policy["environment"] = environment
    return policy
"#;

pub(super) const AGENT_WORKFLOW_CONFIG_YAML: &str = r#"agent_workflow:
  max_tool_calls: 6
  max_source_reads: 8
  max_output_chars: 64000
  freshness_state: wait-until-fresh
  fallback_policy: bounded-search
"#;

pub(super) const AGENT_WORKFLOW_DOC_MD: &str = r#"# Agent Workflow Evaluation Fixture

The coding-agent workflow combines definition lookup, cross-language context packet construction,
configuration tracing, and freshness policy verification. The expected answer must cite structured
evidence before bounded text fallback and keep the packed context under the configured budget.

Freshness scenarios use wait-until-fresh for normal issue analysis and allow-stale only when a
caller explicitly accepts stale graph evidence while diagnostics report the freshness state.
"#;
