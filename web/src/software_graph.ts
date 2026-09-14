import cytoscape, {
  type Core,
  type ElementDefinition,
  type SingularElementReturnValue,
  type StylesheetJson
} from "cytoscape";

import type {
  CodeRepositoryStatus,
  SoftwareEntity,
  SoftwareEntityKind,
  SoftwareGlobalResponse,
  SoftwareShapeDiagnostic,
  SoftwareStatement
} from "./api/contracts";
import { loadCodeRepositories, loadSoftwareProjection } from "./api/client.js";
import { element, sectionShell, statusPill, textElement, type Tone } from "./ui.js";

type SoftwareGraphHost = HTMLElement & {
  __relaySoftwareGraph?: Core;
  __relaySelectSoftwareElement?: (id: string) => void;
};

type SoftwareGraphParts = {
  controls: HTMLElement;
  status: HTMLElement;
  canvas: SoftwareGraphHost;
  empty: HTMLElement;
  details: HTMLElement;
  findings: HTMLElement;
};

type SoftwareElementData = {
  id: string;
  label: string;
  kind: string;
  details: Record<string, string>;
};

const DEFAULT_LIMIT = 180;
const MAX_LIMIT = 500;

const softwareState = {
  repositoryAlias: "",
  limit: DEFAULT_LIMIT
};

let currentSoftwareGraph: Core | null = null;
let requestSerial = 0;

export function softwareGraphSection(): HTMLElement {
  destroySoftwareGraph();
  const section = sectionShell("software", "Software graph");
  const controls = element("div", "software-controls");
  const status = element("div", "software-status");
  const workspace = element("div", "software-workspace");
  const canvasShell = element("div", "software-canvas-shell");
  const canvas = element("div", "software-canvas") as SoftwareGraphHost;
  const empty = textElement("div", "software-empty", "Loading repositories");
  const details = element("aside", "software-details hidden");
  const findings = element("div", "software-findings");

  canvas.dataset.testid = "software-graph-canvas";
  canvas.setAttribute("aria-label", "Software ontology graph");
  canvasShell.append(canvas, empty);
  workspace.append(canvasShell, details);
  controls.append(textElement("span", "muted-line", "Loading indexed repositories"));
  section.append(controls, status, workspace, findings);

  const parts = { controls, status, canvas, empty, details, findings };
  window.requestAnimationFrame(() => void initializeSoftwareGraph(parts));

  return section;
}

async function initializeSoftwareGraph(parts: SoftwareGraphParts) {
  const serial = ++requestSerial;
  try {
    const response = await loadCodeRepositories();
    if (serial !== requestSerial) {
      return;
    }
    if (response.repositories.length === 0) {
      parts.controls.replaceChildren();
      parts.status.replaceChildren(statusPill("unavailable", "warn"));
      parts.empty.textContent = "No indexed repositories";
      return;
    }

    const repository = selectedRepository(response.repositories);
    softwareState.repositoryAlias = repository.alias;
    parts.controls.replaceChildren(repositoryControls(response.repositories, parts));
    await refreshSoftwareGraph(parts, response.repositories);
  } catch (error) {
    if (serial === requestSerial) {
      renderLoadError(parts, error);
    }
  }
}

function selectedRepository(repositories: CodeRepositoryStatus[]): CodeRepositoryStatus {
  return (
    repositories.find((repository) => repository.alias === softwareState.repositoryAlias) ??
    repositories[0]
  );
}

function repositoryControls(
  repositories: CodeRepositoryStatus[],
  parts: SoftwareGraphParts
): HTMLFormElement {
  const form = element("form", "software-control-form");
  const repositoryField = element("label", "software-field");
  const repositorySelect = document.createElement("select");
  repositoryField.htmlFor = "software-repository";
  repositorySelect.id = "software-repository";
  repositorySelect.setAttribute("aria-label", "Software repository");
  for (const repository of repositories) {
    const option = document.createElement("option");
    option.value = repository.alias;
    option.textContent = repository.alias;
    option.selected = repository.alias === softwareState.repositoryAlias;
    repositorySelect.append(option);
  }
  repositorySelect.addEventListener("change", () => {
    softwareState.repositoryAlias = repositorySelect.value;
  });
  repositoryField.append(textElement("span", undefined, "Repository"), repositorySelect);

  const limitField = element("label", "software-field software-limit-field");
  const limitInput = document.createElement("input");
  limitField.htmlFor = "software-limit";
  limitInput.id = "software-limit";
  limitInput.type = "number";
  limitInput.min = "1";
  limitInput.max = String(MAX_LIMIT);
  limitInput.value = String(softwareState.limit);
  limitInput.setAttribute("aria-label", "Software graph limit");
  limitInput.addEventListener("input", () => {
    softwareState.limit = boundedLimit(limitInput.valueAsNumber);
  });
  limitField.append(textElement("span", undefined, "Limit"), limitInput);

  const load = document.createElement("button");
  load.type = "submit";
  load.className = "button";
  load.textContent = "Load";
  load.setAttribute("aria-label", "Load software graph");
  form.append(repositoryField, limitField, load);
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    softwareState.repositoryAlias = repositorySelect.value;
    softwareState.limit = boundedLimit(limitInput.valueAsNumber);
    void refreshSoftwareGraph(parts, repositories);
  });

  return form;
}

async function refreshSoftwareGraph(
  parts: SoftwareGraphParts,
  repositories: CodeRepositoryStatus[]
) {
  const serial = ++requestSerial;
  const repository = selectedRepository(repositories);
  const refSelector = repository.last_indexed_commit ?? "HEAD";
  parts.status.replaceChildren(statusPill("loading", "warn"));
  parts.empty.textContent = "Loading software graph";
  parts.empty.classList.remove("hidden");
  parts.details.className = "software-details hidden";
  parts.findings.replaceChildren();

  try {
    const [statements, conflicts] = await Promise.all([
      loadSoftwareProjection({
        alias: repository.alias,
        refSelector,
        kind: "statements",
        limit: softwareState.limit
      }),
      loadSoftwareProjection({
        alias: repository.alias,
        refSelector,
        kind: "conflicts",
        limit: softwareState.limit
      })
    ]);
    if (serial !== requestSerial) {
      return;
    }
    renderSoftwareStatus(parts.status, statements, repository);
    renderSoftwareCanvas(parts, statements);
    renderSoftwareFindings(parts.findings, conflicts, statements.entities);
  } catch (error) {
    if (serial === requestSerial) {
      renderLoadError(parts, error);
    }
  }
}

function renderSoftwareStatus(
  host: HTMLElement,
  response: SoftwareGlobalResponse,
  repository: CodeRepositoryStatus
) {
  const status = response.status;
  host.replaceChildren(
    statusPill(status.freshness, freshnessTone(status.freshness)),
    textElement("span", "muted-line", `ontology ${status.ontology_version}`),
    textElement("span", "muted-line", `schema ${status.projection_schema_version}`),
    textElement(
      "span",
      "muted-line",
      `${status.entity_count} entities / ${status.statement_count} statements`
    ),
    textElement(
      "span",
      "muted-line",
      `provenance ${(status.completeness_basis_points / 100).toFixed(0)}%`
    ),
    textElement("span", "muted-line", `${status.conflict_count} conflicts`),
    textElement(
      "span",
      "muted-line",
      `${status.source_coverage.source_path_count} source paths`
    ),
    textElement("span", "muted-line", shortCommit(repository.last_indexed_commit))
  );
}

function renderSoftwareCanvas(parts: SoftwareGraphParts, response: SoftwareGlobalResponse) {
  destroySoftwareGraph();
  const elements = softwareElements(response.entities, response.statements);
  if (elements.length === 0) {
    parts.empty.textContent = "No software ontology data";
    parts.empty.classList.remove("hidden");
    return;
  }
  parts.empty.classList.add("hidden");

  const graph = cytoscape({
    container: parts.canvas,
    elements,
    style: softwareCanvasStyle(),
    minZoom: 0.08,
    maxZoom: 3,
    layout: {
      name: "cose",
      animate: false,
      padding: 42,
      nodeRepulsion: 7200,
      idealEdgeLength: 92
    }
  });
  currentSoftwareGraph = graph;
  parts.canvas.__relaySoftwareGraph = graph;
  parts.canvas.__relaySelectSoftwareElement = (id: string) => {
    const target = graph.getElementById(id);
    if (target.length > 0) {
      target.select();
      focusSoftwareElement(graph, target, parts.details);
    }
  };
  graph.on("tap", "node, edge", (event) => {
    focusSoftwareElement(graph, event.target as SingularElementReturnValue, parts.details);
  });
  graph.on("select", "node, edge", (event) => {
    focusSoftwareElement(graph, event.target as SingularElementReturnValue, parts.details);
  });
  graph.on("tap", (event) => {
    if (event.target === graph) {
      graph.elements().unselect().removeClass("faded neighbor");
      parts.details.className = "software-details hidden";
      parts.details.replaceChildren();
    }
  });
}

function softwareElements(
  entities: SoftwareEntity[],
  statements: SoftwareStatement[]
): ElementDefinition[] {
  const elements: ElementDefinition[] = [];
  const knownNodes = new Set<string>();

  for (const entity of entities) {
    if (knownNodes.has(entity.entity_key)) {
      continue;
    }
    knownNodes.add(entity.entity_key);
    elements.push(entityElement(entity));
  }

  for (const statement of statements) {
    ensureReferenceNode(elements, knownNodes, statement.subject_id);
    const target = statementTarget(elements, knownNodes, statement);
    if (!target) {
      continue;
    }
    elements.push({
      group: "edges",
      data: {
        id: statement.statement_id,
        source: statement.subject_id,
        target,
        label: formatContractValue(statement.predicate),
        kind: "statement",
        color: statementColor(statement.fact_state, statement.resolution_state),
        details: statementDetails(statement)
      },
      classes: `fact-${statement.fact_state}`
    });
  }

  return elements;
}

function entityElement(entity: SoftwareEntity): ElementDefinition {
  const evidence = entity.evidence_refs.map(evidenceLabel).join(", ");
  return {
    group: "nodes",
    data: {
      id: entity.entity_key,
      label: entity.name,
      kind: entity.entity_kind,
      color: entityColor(entity.entity_kind),
      shape: entityShape(entity.entity_kind),
      size: entitySize(entity.entity_kind),
      details: {
        Kind: formatContractValue(entity.entity_kind),
        Source: formatContractValue(entity.source_kind),
        Scope: entity.source_scope,
        Namespace: entity.namespace ?? "—",
        Evidence: evidence || "snapshot identity",
        "Stable key": entity.entity_key,
        Occurrence: entity.occurrence_id,
        ...entity.attributes
      }
    },
    classes: `entity-${entity.entity_kind}`
  };
}

function ensureReferenceNode(
  elements: ElementDefinition[],
  knownNodes: Set<string>,
  id: string
) {
  if (knownNodes.has(id)) {
    return;
  }
  knownNodes.add(id);
  elements.push({
    group: "nodes",
    data: {
      id,
      label: shortIdentity(id),
      kind: "referenced_entity",
      color: cssColor("--muted"),
      shape: "ellipse",
      size: 30,
      details: {
        Kind: "referenced entity",
        "Stable key": id,
        Note: "Entity occurrence is outside the bounded response window"
      }
    }
  });
}

function statementTarget(
  elements: ElementDefinition[],
  knownNodes: Set<string>,
  statement: SoftwareStatement
): string | null {
  if (statement.object_id) {
    ensureReferenceNode(elements, knownNodes, statement.object_id);
    return statement.object_id;
  }
  if (!statement.object_value) {
    return null;
  }
  const id = `literal:${statement.statement_id}`;
  if (!knownNodes.has(id)) {
    knownNodes.add(id);
    elements.push({
      group: "nodes",
      data: {
        id,
        label: statement.object_value,
        kind: "literal",
        color: cssColor("--soft"),
        shape: "rectangle",
        size: 28,
        details: {
          Kind: "literal",
          Value: statement.object_value
        }
      }
    });
  }
  return id;
}

function statementDetails(statement: SoftwareStatement): Record<string, string> {
  return {
    Predicate: formatContractValue(statement.predicate),
    Assertion: formatContractValue(statement.assertion_mode),
    Resolution: formatContractValue(statement.resolution_state),
    State: formatContractValue(statement.fact_state),
    Source: formatContractValue(statement.source_kind),
    Confidence: `${(statement.confidence_basis_points / 100).toFixed(0)}%`,
    Evidence: statement.evidence_refs.map(evidenceLabel).join(", ") || "—",
    Extractor: `${statement.extractor_id} ${statement.extractor_version}`,
    "Statement id": statement.statement_id
  };
}

function softwareCanvasStyle(): StylesheetJson {
  const text = cssColor("--text");
  const muted = cssColor("--muted");
  const line = cssColor("--line-strong");
  const selected = cssColor("--amber");
  const surface = cssColor("--surface-raised");
  return [
    {
      selector: "node",
      style: {
        "background-color": "data(color)",
        "border-color": line,
        "border-width": 1,
        color: text,
        content: "data(label)",
        "font-size": 10,
        height: "data(size)",
        shape: "data(shape)",
        "text-background-color": surface,
        "text-background-opacity": 0.9,
        "text-background-padding": 2,
        "text-margin-y": -8,
        "text-max-width": 120,
        "text-valign": "bottom",
        "text-wrap": "ellipsis",
        width: "data(size)"
      }
    },
    {
      selector: "edge",
      style: {
        "curve-style": "bezier",
        color: muted,
        "font-size": 9,
        label: "data(label)",
        "line-color": "data(color)",
        opacity: 0.8,
        "target-arrow-color": "data(color)",
        "target-arrow-shape": "triangle",
        "text-background-color": surface,
        "text-background-opacity": 0.9,
        "text-background-padding": 1,
        width: 1.4
      }
    },
    {
      selector: "node:selected",
      style: {
        "border-color": selected,
        "border-width": 3
      }
    },
    {
      selector: "edge:selected",
      style: {
        color: selected,
        "line-color": selected,
        "target-arrow-color": selected,
        opacity: 1,
        width: 3
      }
    },
    {
      selector: ".neighbor",
      style: { opacity: 1 }
    },
    {
      selector: ".faded",
      style: { opacity: 0.16 }
    }
  ] as unknown as StylesheetJson;
}

function focusSoftwareElement(
  graph: Core,
  target: SingularElementReturnValue,
  panel: HTMLElement
) {
  graph.elements().removeClass("faded neighbor");
  const related = target.closedNeighborhood();
  graph.elements().difference(related).addClass("faded");
  related.addClass("neighbor");

  const data = target.data() as SoftwareElementData;
  const list = element("dl", "software-detail-list");
  for (const [label, value] of Object.entries(data.details ?? {})) {
    const item = element("div", "software-detail-item");
    item.append(textElement("dt", undefined, label), textElement("dd", undefined, value));
    list.append(item);
  }
  panel.className = "software-details";
  panel.replaceChildren(textElement("div", "software-detail-title", data.label), list);
}

function renderSoftwareFindings(
  host: HTMLElement,
  response: SoftwareGlobalResponse,
  entities: SoftwareEntity[]
) {
  const labels = new Map(entities.map((entity) => [entity.entity_key, entity.name]));
  host.replaceChildren(
    findingsTable(response.statements, labels),
    diagnosticsTable(response.diagnostics)
  );
}

function findingsTable(
  statements: SoftwareStatement[],
  labels: Map<string, string>
): HTMLElement {
  const panel = element("div", "software-finding-panel");
  const table = element("table", "software-table software-conflicts");
  table.append(tableCaption("Conflicts and unresolved statements"));
  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  for (const label of ["State", "Predicate", "Subject", "Object", "Source", "Evidence"]) {
    headRow.append(textElement("th", undefined, label));
  }
  head.append(headRow);
  const body = document.createElement("tbody");
  for (const statement of statements) {
    const row = document.createElement("tr");
    row.append(
      stateCell(statement),
      textElement("td", undefined, formatContractValue(statement.predicate)),
      textElement("td", undefined, labels.get(statement.subject_id) ?? shortIdentity(statement.subject_id)),
      textElement(
        "td",
        undefined,
        statement.object_id
          ? labels.get(statement.object_id) ?? shortIdentity(statement.object_id)
          : statement.object_value ?? "—"
      ),
      textElement("td", undefined, formatContractValue(statement.source_kind)),
      textElement("td", undefined, statement.evidence_refs.map(evidenceLabel).join(", ") || "—")
    );
    body.append(row);
  }
  if (statements.length === 0) {
    body.append(emptyTableRow(6, "No conflicts or unresolved statements"));
  }
  table.append(head, body);
  panel.append(table);
  return panel;
}

function diagnosticsTable(diagnostics: SoftwareShapeDiagnostic[]): HTMLElement {
  const panel = element("div", "software-finding-panel");
  const table = element("table", "software-table software-diagnostics");
  table.append(tableCaption("Shape diagnostics"));
  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  for (const label of ["Severity", "Code", "Field", "Message"]) {
    headRow.append(textElement("th", undefined, label));
  }
  head.append(headRow);
  const body = document.createElement("tbody");
  for (const diagnostic of diagnostics) {
    const row = document.createElement("tr");
    const severity = document.createElement("td");
    severity.append(statusPill(diagnostic.severity, diagnostic.severity === "error" ? "bad" : "warn"));
    row.append(
      severity,
      textElement("td", undefined, formatContractValue(diagnostic.code)),
      textElement("td", undefined, diagnostic.field),
      textElement("td", undefined, diagnostic.message)
    );
    body.append(row);
  }
  if (diagnostics.length === 0) {
    body.append(emptyTableRow(4, "No shape diagnostics"));
  }
  table.append(head, body);
  panel.append(table);
  return panel;
}

function tableCaption(label: string): HTMLTableCaptionElement {
  return textElement("caption", undefined, label);
}

function emptyTableRow(columnCount: number, message: string): HTMLTableRowElement {
  const row = document.createElement("tr");
  const cell = textElement("td", "muted-line", message);
  cell.colSpan = columnCount;
  row.append(cell);
  return row;
}

function stateCell(statement: SoftwareStatement): HTMLTableCellElement {
  const cell = document.createElement("td");
  const label =
    statement.fact_state === "active" ? statement.resolution_state : statement.fact_state;
  cell.append(statusPill(label, statementTone(statement)));
  return cell;
}

function statementTone(statement: SoftwareStatement): Tone {
  if (statement.fact_state === "rejected" || statement.fact_state === "conflicting") {
    return "bad";
  }
  if (statement.fact_state === "superseded" || statement.resolution_state !== "resolved") {
    return "warn";
  }
  return "good";
}

function freshnessTone(freshness: SoftwareGlobalResponse["status"]["freshness"]): Tone {
  if (freshness === "fresh") {
    return "good";
  }
  return freshness === "degraded" ? "bad" : "warn";
}

function entityColor(kind: SoftwareEntityKind): string {
  if (kind === "software_system" || kind === "component" || kind === "api") {
    return cssColor("--cyan");
  }
  if (kind === "build_definition" || kind === "build_job" || kind === "pipeline") {
    return cssColor("--amber");
  }
  if (kind === "deployment_unit" || kind === "runtime_service" || kind === "resource") {
    return cssColor("--green");
  }
  if (kind === "test_case" || kind === "runtime_observation") {
    return cssColor("--red");
  }
  return cssColor("--soft");
}

function entityShape(kind: SoftwareEntityKind): string {
  if (kind === "software_system" || kind === "component" || kind === "file_revision") {
    return "round-rectangle";
  }
  if (kind === "release_artifact" || kind === "runtime_observation") {
    return "diamond";
  }
  if (kind === "api") {
    return "hexagon";
  }
  return "ellipse";
}

function entitySize(kind: SoftwareEntityKind): number {
  return kind === "repository_snapshot" || kind === "software_system" ? 44 : 34;
}

function statementColor(
  factState: SoftwareStatement["fact_state"],
  resolution: SoftwareStatement["resolution_state"]
): string {
  if (factState === "conflicting" || factState === "rejected") {
    return cssColor("--red");
  }
  if (factState === "superseded" || resolution !== "resolved") {
    return cssColor("--amber");
  }
  return cssColor("--line-strong");
}

function evidenceLabel(reference: SoftwareStatement["evidence_refs"][number]): string {
  const range = reference.line_range;
  return `${reference.path}:${range.start}${range.end === range.start ? "" : `-${range.end}`}`;
}

function renderLoadError(parts: SoftwareGraphParts, error: unknown) {
  destroySoftwareGraph();
  parts.status.replaceChildren(statusPill("error", "bad"));
  parts.empty.textContent = error instanceof Error ? error.message : "Software graph unavailable";
  parts.empty.classList.remove("hidden");
  parts.findings.replaceChildren();
}

function destroySoftwareGraph() {
  if (currentSoftwareGraph) {
    currentSoftwareGraph.destroy();
    currentSoftwareGraph = null;
  }
}

function boundedLimit(value: number): number {
  return Number.isFinite(value)
    ? Math.min(MAX_LIMIT, Math.max(1, Math.round(value)))
    : DEFAULT_LIMIT;
}

function cssColor(variable: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(variable).trim();
}

function shortCommit(commit: string | undefined): string {
  return commit ? `commit ${commit.slice(0, 12)}` : "unversioned scope";
}

function shortIdentity(identity: string): string {
  const suffix = identity.includes(":") ? identity.slice(identity.lastIndexOf(":") + 1) : identity;
  return suffix.length > 14 ? `${suffix.slice(0, 12)}…` : suffix;
}

function formatContractValue(value: string): string {
  return value.replaceAll("_", " ");
}
