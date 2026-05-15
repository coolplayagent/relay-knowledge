import cytoscape, {
  type Core,
  type ElementDefinition,
  type EventObject,
  type LayoutOptions,
  type SingularElementReturnValue,
  type StylesheetJson
} from "cytoscape";

import type {
  GraphCanvasEdge,
  GraphCanvasKind,
  GraphCanvasNode,
  GraphCanvasResponse
} from "./api/contracts";
import { loadGraphCanvas } from "./api/client.js";
import { element, icon, sectionShell, statusPill, textElement } from "./ui.js";

type GraphMode = {
  id: GraphCanvasKind;
  label: string;
};

type GraphElementData = {
  id: string;
  kind: string;
  label: string;
  subtitle?: string;
  source?: string;
  target?: string;
  source_scope?: string;
  graph_version: number;
  status?: string;
  confidence_basis_points?: number;
  evidence_count?: number;
  details: Record<string, string>;
};

type GraphCanvasHost = HTMLElement & {
  __relayGraphCanvas?: Core;
  __relaySelectGraphElement?: (id: string) => void;
};

const MODES: GraphMode[] = [
  { id: "knowledge", label: "Knowledge" },
  { id: "code", label: "Code" },
  { id: "mixed", label: "Mixed" }
];

const DEFAULT_LIMIT = 250;
const MAX_LIMIT = 1000;

const graphState = {
  mode: "knowledge" as GraphCanvasKind,
  sourceScope: "",
  query: "",
  limit: DEFAULT_LIMIT
};

let currentCy: Core | null = null;
let requestSerial = 0;

export function graphCanvasSection(): HTMLElement {
  destroyCurrentGraph();
  const section = sectionShell("graph", "Graph");
  const controls = graphControls();
  const status = element("div", "graph-status");
  const workspace = element("div", "graph-workspace");
  const canvasShell = element("div", "graph-canvas-shell");
  const canvas = element("div", "graph-canvas") as GraphCanvasHost;
  const empty = textElement("div", "graph-empty", "Loading graph");
  const details = element("aside", "graph-details hidden");

  canvas.dataset.testid = "graph-canvas";
  canvas.setAttribute("aria-label", "Graph canvas");
  canvasShell.append(canvas, empty);
  workspace.append(canvasShell, details);
  section.append(controls, status, workspace);

  window.requestAnimationFrame(() => void refreshGraph({ canvas, empty, status, details }));

  return section;
}

function graphControls(): HTMLElement {
  const controls = element("div", "graph-controls");
  const tabs = element("div", "graph-mode-tabs");
  tabs.setAttribute("role", "tablist");
  tabs.setAttribute("aria-label", "Graph canvas mode");
  tabs.append(...MODES.map(modeButton));

  const filters = element("form", "graph-filter-form");
  filters.append(
    textInput("Scope", graphState.sourceScope, (value) => {
      graphState.sourceScope = value;
    }),
    textInput("Query", graphState.query, (value) => {
      graphState.query = value;
    }),
    limitInput(),
    commandButton("Apply", "refresh-icon", "Apply graph filters")
  );
  filters.addEventListener("submit", (event) => {
    event.preventDefault();
    rerenderGraphPage();
  });

  const actions = element("div", "graph-actions");
  actions.append(
    canvasAction("Fit", "Fit graph to the viewport", () => currentCy?.fit(undefined, 38)),
    canvasAction("Zoom +", "Zoom in", () => zoomCurrentGraph(1.18)),
    canvasAction("Zoom -", "Zoom out", () => zoomCurrentGraph(0.84)),
    canvasAction("Reset", "Reset graph zoom and selection", () => {
      if (!currentCy) {
        return;
      }
      currentCy.elements().unselect();
      currentCy.fit(undefined, 38);
    })
  );

  controls.append(tabs, filters, actions);

  return controls;
}

function modeButton(mode: GraphMode): HTMLButtonElement {
  const button = document.createElement("button");
  const active = graphState.mode === mode.id;
  button.type = "button";
  button.className = active ? "graph-mode-tab active" : "graph-mode-tab";
  button.textContent = mode.label;
  button.setAttribute("role", "tab");
  button.setAttribute("aria-selected", String(active));
  button.addEventListener("click", () => {
    graphState.mode = mode.id;
    rerenderGraphPage();
  });

  return button;
}

function textInput(
  label: string,
  value: string,
  onChange: (value: string) => void
): HTMLElement {
  const field = element("label", "graph-field");
  const input = document.createElement("input");
  const fieldId = `graph-${label.toLowerCase()}`;
  field.htmlFor = fieldId;
  input.id = fieldId;
  input.name = fieldId;
  input.value = value;
  input.setAttribute("aria-label", label);
  input.addEventListener("input", () => onChange(input.value));
  field.append(textElement("span", undefined, label), input);

  return field;
}

function limitInput(): HTMLElement {
  const field = element("label", "graph-field graph-limit-field");
  const input = document.createElement("input");
  field.htmlFor = "graph-limit";
  input.id = "graph-limit";
  input.name = "graph-limit";
  input.type = "number";
  input.min = "1";
  input.max = String(MAX_LIMIT);
  input.value = String(graphState.limit);
  input.setAttribute("aria-label", "Limit");
  input.addEventListener("input", () => {
    graphState.limit = boundedLimit(input.valueAsNumber);
  });
  field.append(textElement("span", undefined, "Limit"), input);

  return field;
}

function commandButton(label: string, iconName: string, ariaLabel: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "submit";
  button.className = "button";
  button.setAttribute("aria-label", ariaLabel);
  button.append(icon(iconName), document.createTextNode(label));

  return button;
}

function canvasAction(
  label: string,
  ariaLabel: string,
  action: () => void
): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "button graph-tool-button";
  button.textContent = label;
  button.title = ariaLabel;
  button.setAttribute("aria-label", ariaLabel);
  button.addEventListener("click", action);

  return button;
}

async function refreshGraph(parts: {
  canvas: GraphCanvasHost;
  empty: HTMLElement;
  status: HTMLElement;
  details: HTMLElement;
}) {
  const serial = ++requestSerial;
  parts.empty.textContent = "Loading graph";
  parts.empty.classList.remove("hidden");
  parts.status.replaceChildren(statusPill("loading", "warn"));
  parts.details.className = "graph-details hidden";

  try {
    const snapshot = await loadGraphCanvas({
      kind: graphState.mode,
      sourceScope: trimmedValue(graphState.sourceScope),
      query: trimmedValue(graphState.query),
      limit: graphState.limit
    });
    if (serial !== requestSerial) {
      return;
    }
    renderGraph(snapshot, parts);
  } catch (error) {
    if (serial !== requestSerial) {
      return;
    }
    destroyCurrentGraph();
    parts.empty.textContent = error instanceof Error ? error.message : "Graph unavailable";
    parts.empty.classList.remove("hidden");
    parts.status.replaceChildren(statusPill("error", "bad"));
  }
}

function renderGraph(
  snapshot: GraphCanvasResponse,
  parts: {
    canvas: GraphCanvasHost;
    empty: HTMLElement;
    status: HTMLElement;
    details: HTMLElement;
  }
) {
  destroyCurrentGraph();
  const count = snapshot.nodes.length + snapshot.edges.length;
  parts.status.replaceChildren(
    statusPill(snapshot.summary.truncated ? "truncated" : "ready", snapshot.summary.truncated ? "warn" : "good"),
    textElement("span", "muted-line", `${snapshot.summary.node_count} nodes / ${snapshot.summary.edge_count} edges`),
    textElement("span", "muted-line", `version ${snapshot.metadata.graph_version}`)
  );

  if (count === 0) {
    parts.empty.textContent = "No graph data";
    parts.empty.classList.remove("hidden");
    return;
  }

  parts.empty.classList.add("hidden");
  const cy = cytoscape({
    container: parts.canvas,
    elements: canvasElements(snapshot),
    style: canvasStyle(),
    minZoom: 0.08,
    maxZoom: 3,
    layout: layoutFor(snapshot.summary.kind)
  });
  currentCy = cy;
  parts.canvas.__relayGraphCanvas = cy;
  parts.canvas.__relaySelectGraphElement = (id: string) => {
    const target = cy.getElementById(id);
    if (target.length > 0) {
      target.select();
      focusElement(cy, target, parts.details);
    }
  };

  cy.on("tap", "node, edge", (event: EventObject) => {
    focusElement(cy, event.target as SingularElementReturnValue, parts.details);
  });
  cy.on("select", "node, edge", (event: EventObject) => {
    focusElement(cy, event.target as SingularElementReturnValue, parts.details);
  });
  cy.on("tap", (event: EventObject) => {
    if (event.target === cy) {
      clearSelection(cy, parts.details);
    }
  });
}

function canvasElements(snapshot: GraphCanvasResponse): ElementDefinition[] {
  return [
    ...snapshot.nodes.map((node) => ({
      group: "nodes" as const,
      data: {
        ...node,
        color: colorForKind(node.kind),
        shape: shapeForKind(node.kind),
        size: nodeSize(node)
      },
      classes: `kind-${node.kind}`
    })),
    ...snapshot.edges.map((edge) => ({
      group: "edges" as const,
      data: {
        ...edge,
        color: colorForKind(edge.kind)
      },
      classes: `kind-${edge.kind}`
    }))
  ] as ElementDefinition[];
}

function canvasStyle(): StylesheetJson {
  const css = getComputedStyle(document.documentElement);
  const text = css.getPropertyValue("--text").trim();
  const muted = css.getPropertyValue("--muted").trim();
  const line = css.getPropertyValue("--line-strong").trim();
  const selected = css.getPropertyValue("--amber").trim();
  const surface = css.getPropertyValue("--surface-raised").trim();

  return [
    {
      selector: "node",
      style: {
        "background-color": "data(color)",
        "border-color": line,
        "border-width": 1,
        color: text,
        content: "data(label)",
        "font-size": 11,
        height: "data(size)",
        label: "data(label)",
        "min-zoomed-font-size": 8,
        shape: "data(shape)",
        "text-background-color": surface,
        "text-background-opacity": 0.82,
        "text-background-padding": 2,
        "text-margin-y": -8,
        "text-max-width": 110,
        "text-valign": "bottom",
        "text-wrap": "ellipsis",
        width: "data(size)"
      }
    },
    {
      selector: "edge",
      style: {
        "curve-style": "bezier",
        "font-size": 9,
        "line-color": "data(color)",
        "target-arrow-color": "data(color)",
        "target-arrow-shape": "triangle",
        "text-background-color": surface,
        "text-background-opacity": 0.82,
        "text-background-padding": 1,
        color: muted,
        label: "data(label)",
        opacity: 0.82,
        width: 1.4
      }
    },
    {
      selector: "node:selected",
      style: {
        "border-color": selected,
        "border-width": 3,
        "background-color": selected
      }
    },
    {
      selector: "edge:selected",
      style: {
        "line-color": selected,
        "target-arrow-color": selected,
        color: selected,
        opacity: 1,
        width: 3
      }
    },
    {
      selector: ".neighbor",
      style: {
        opacity: 1
      }
    },
    {
      selector: ".faded",
      style: {
        opacity: 0.18
      }
    }
  ] as unknown as StylesheetJson;
}

function layoutFor(kind: GraphCanvasKind): LayoutOptions {
  if (kind === "code") {
    return { name: "breadthfirst", directed: true, padding: 48, spacingFactor: 1.25 };
  }
  if (kind === "mixed") {
    return { name: "cose", animate: false, padding: 48, nodeRepulsion: 7600, idealEdgeLength: 92 };
  }

  return { name: "cose", animate: false, padding: 48, nodeRepulsion: 6800, idealEdgeLength: 86 };
}

function focusElement(cy: Core, target: SingularElementReturnValue, panel: HTMLElement) {
  cy.elements().removeClass("faded neighbor");
  const related = target.closedNeighborhood();
  cy.elements().difference(related).addClass("faded");
  related.addClass("neighbor");
  renderDetails(panel, target.data() as GraphElementData);
}

function clearSelection(cy: Core, panel: HTMLElement) {
  cy.elements().unselect();
  cy.elements().removeClass("faded neighbor");
  panel.className = "graph-details hidden";
  panel.replaceChildren();
}

function renderDetails(panel: HTMLElement, data: GraphElementData) {
  panel.className = "graph-details";
  const rows = element("dl", "graph-detail-list");
  rows.append(
    detailItem("Kind", data.kind),
    detailItem("Id", data.id),
    detailItem("Version", String(data.graph_version))
  );
  if (data.source_scope) {
    rows.append(detailItem("Scope", data.source_scope));
  }
  if (data.status) {
    rows.append(detailItem("Status", data.status));
  }
  if (data.confidence_basis_points !== undefined) {
    rows.append(detailItem("Confidence", String(data.confidence_basis_points)));
  }
  if (data.evidence_count !== undefined) {
    rows.append(detailItem("Evidence", String(data.evidence_count)));
  }
  for (const [key, value] of Object.entries(data.details ?? {})) {
    rows.append(detailItem(formatKey(key), value));
  }
  panel.replaceChildren(textElement("div", "graph-detail-title", data.label), rows);
}

function detailItem(label: string, value: string): HTMLElement {
  const item = element("div", "graph-detail-item");
  item.append(textElement("dt", undefined, label), textElement("dd", undefined, value));

  return item;
}

function zoomCurrentGraph(factor: number) {
  if (!currentCy) {
    return;
  }
  const box = currentCy.container()?.getBoundingClientRect();
  if (!box) {
    return;
  }
  currentCy.zoom({
    level: currentCy.zoom() * factor,
    renderedPosition: {
      x: box.width / 2,
      y: box.height / 2
    }
  });
}

function destroyCurrentGraph() {
  if (currentCy) {
    currentCy.destroy();
    currentCy = null;
  }
}

function rerenderGraphPage() {
  window.dispatchEvent(new CustomEvent("relay-knowledge:graph-rerender"));
}

function boundedLimit(value: number): number {
  if (!Number.isFinite(value)) {
    return DEFAULT_LIMIT;
  }

  return Math.min(MAX_LIMIT, Math.max(1, Math.round(value)));
}

function trimmedValue(value: string): string | undefined {
  const trimmed = value.trim();

  return trimmed.length > 0 ? trimmed : undefined;
}

function nodeSize(node: GraphCanvasNode): number {
  return 28 + Math.min(16, Math.max(0, node.weight) * 4);
}

function shapeForKind(kind: string): string {
  if (kind === "code_file") {
    return "round-rectangle";
  }
  if (kind === "claim" || kind === "event") {
    return "diamond";
  }

  return "ellipse";
}

function colorForKind(kind: string): string {
  const css = getComputedStyle(document.documentElement);
  const colors: Record<string, string> = {
    entity: css.getPropertyValue("--cyan").trim(),
    evidence: css.getPropertyValue("--green").trim(),
    relation: css.getPropertyValue("--amber").trim(),
    claim: css.getPropertyValue("--red").trim(),
    event: css.getPropertyValue("--amber").trim(),
    source_scope: css.getPropertyValue("--soft").trim(),
    code_file: css.getPropertyValue("--muted").trim(),
    code_symbol: css.getPropertyValue("--cyan").trim(),
    contains: css.getPropertyValue("--line-strong").trim(),
    defines: css.getPropertyValue("--cyan").trim(),
    call: css.getPropertyValue("--green").trim(),
    import: css.getPropertyValue("--amber").trim(),
    type: css.getPropertyValue("--cyan").trim(),
    implementation: css.getPropertyValue("--red").trim(),
    evidence_link: css.getPropertyValue("--green").trim(),
    source_path: css.getPropertyValue("--red").trim()
  };

  return colors[kind] ?? css.getPropertyValue("--line-strong").trim();
}

function formatKey(key: string): string {
  return key.replaceAll("_", " ");
}
