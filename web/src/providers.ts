import type { HealthResponse, IndexCursor, IndexStatus, ProjectStatusResponse } from "./api/contracts";

type Tone = "good" | "warn" | "bad";

export function providersSection(
  status: ProjectStatusResponse,
  health: HealthResponse
): HTMLElement {
  const section = element("section", "section");
  section.id = "providers";
  section.append(textElement("h2", "section-title", "Providers"));

  const grid = element("div", "provider-grid");
  const semanticCursor = primaryCursor(health.index_cursors, "semantic");
  const vectorCursor = primaryCursor(health.index_cursors, "vector");
  grid.append(
    providerItem(
      "Semantic backend",
      status.runtime.semantic_backend_mode,
      backendTone(status.runtime.semantic_backend_mode, semanticCursor),
      providerDetail(
        status.runtime.text_embedding_model,
        status.runtime.embedding_dimension,
        semanticCursor
      )
    ),
    providerItem(
      "Vector backend",
      status.runtime.vector_backend_mode,
      backendTone(status.runtime.vector_backend_mode, vectorCursor),
      providerDetail(
        status.runtime.text_embedding_model,
        status.runtime.embedding_dimension,
        vectorCursor
      )
    ),
    providerItem(
      "Remote endpoint",
      status.runtime.embedding_provider ?? "not configured",
      status.runtime.embedding_provider ? "good" : "warn",
      endpointDetail(status)
    ),
    providerItem(
      "Rerank",
      status.runtime.rerank_backend_mode,
      status.runtime.rerank_backend_mode === "disabled" ? "warn" : "good",
      rerankDetail(status)
    ),
    providerItem(
      "Budgets",
      `${status.runtime.embedding_max_concurrency ?? 0} concurrent`,
      status.runtime.embedding_provider ? "good" : "warn",
      `batch ${status.runtime.embedding_batch_size ?? 0} / timeout ${
        status.runtime.embedding_timeout_ms ?? 0
      }ms`
    )
  );
  section.append(grid, providerCursorTable(health.index_cursors));

  return section;
}

function rerankDetail(status: ProjectStatusResponse): string {
  const model = status.runtime.rerank_model ?? "no model";
  return [
    model,
    `${status.runtime.rerank_candidate_multiplier}x candidates`,
    `cap ${status.runtime.rerank_max_candidates}`,
    `${status.runtime.rerank_timeout_ms}ms`
  ].join(" / ");
}

function primaryCursor(
  cursors: IndexCursor[],
  kind: IndexStatus["kind"]
): IndexCursor | undefined {
  return cursors
    .filter((cursor) => cursor.kind === kind)
    .sort((left, right) => right.indexed_graph_version - left.indexed_graph_version)[0];
}

function backendTone(mode: string, cursor: IndexCursor | undefined): Tone {
  if (mode === "disabled") {
    return "warn";
  }
  if (!cursor || cursor.state === "failed") {
    return "bad";
  }
  return cursor.state === "fresh" ? "good" : "warn";
}

function providerDetail(
  model: string,
  dimension: number,
  cursor: IndexCursor | undefined
): string {
  const indexed = cursor ? `indexed ${cursor.indexed_graph_version}` : "cursor unavailable";
  const cursorModel = cursor?.model_name ? ` / cursor ${cursor.model_name}` : "";

  return `${model} / ${dimension}d / ${indexed}${cursorModel}`;
}

function endpointDetail(status: ProjectStatusResponse): string {
  if (!status.runtime.embedding_provider) {
    return "external backend is not active";
  }
  const auth = status.runtime.embedding_api_key_configured ? "key configured" : "key missing";
  const baseUrl = status.runtime.embedding_base_url ?? "endpoint unavailable";

  return `${baseUrl} / ${auth}`;
}

function providerItem(label: string, value: string, tone: Tone, detail: string): HTMLElement {
  const item = element("div", "provider-item");
  const heading = element("div", "readiness-heading");
  heading.append(textElement("span", "readiness-label", label), statusPill(value, tone));
  item.append(heading, textElement("div", "readiness-detail", detail));

  return item;
}

function providerCursorTable(cursors: IndexCursor[]): HTMLElement {
  const table = document.createElement("table");
  table.className = "provider-cursors";
  table.append(
    tableHead(["Kind", "Scope", "State", "Model", "Dimension", "Cursor"]),
    cursorTableBody(cursors)
  );

  return table;
}

function cursorTableBody(cursors: IndexCursor[]): HTMLTableSectionElement {
  const body = document.createElement("tbody");
  const providerCursors = cursors.filter(
    (item) => item.kind === "semantic" || item.kind === "vector"
  );
  for (const cursor of providerCursors) {
    const row = document.createElement("tr");
    row.append(
      textElement("td", undefined, cursor.kind),
      textElement("td", undefined, cursor.source_scope),
      tableState(cursor.state),
      textElement("td", undefined, cursor.model_name ?? "unknown"),
      textElement("td", undefined, String(cursor.model_dimension ?? 0)),
      textElement("td", undefined, cursor.backend_cursor ?? "pending")
    );
    body.append(row);
  }
  if (body.children.length === 0) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.className = "muted-line";
    cell.textContent = "No semantic/vector cursor metadata";
    cell.colSpan = 6;
    row.append(cell);
    body.append(row);
  }

  return body;
}

function tableHead(labels: string[]): HTMLTableSectionElement {
  const head = document.createElement("thead");
  const row = document.createElement("tr");
  for (const label of labels) {
    row.append(textElement("th", undefined, label));
  }
  head.append(row);

  return head;
}

function tableState(state: IndexStatus["state"]): HTMLTableCellElement {
  const cell = document.createElement("td");
  const tone = state === "fresh" ? "good" : state === "stale" || state === "paused" ? "warn" : "bad";
  cell.append(statusPill(state, tone));

  return cell;
}

function statusPill(text: string, tone: Tone): HTMLElement {
  return textElement("span", `status-pill ${tone}`, text);
}

function element(tag: string, className?: string): HTMLElement {
  const node = document.createElement(tag);
  if (className) {
    node.className = className;
  }

  return node;
}

function textElement(tag: string, className: string | undefined, text: string): HTMLElement {
  const node = element(tag, className);
  node.textContent = text;

  return node;
}
