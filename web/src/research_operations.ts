export const researchActions: Array<[string, string]> = [
  ["research.status", "Research delivery status"],
  ["sources.audit", "Audit source captures"],
  ["evidence.validate", "Validate evidence bundle"],
  ["evidence.import", "Import proposed evidence"],
  ["evidence.export", "Export evidence bundle"],
  ["evidence.view", "View evidence graph"],
  ["evidence.impact", "Propose concept clarification"],
  ["knowledge.map.plan", "Plan map transaction"],
  ["knowledge.map.apply", "Apply map transaction"]
];

export const researchState = {
  action: "research.status",
  targetKind: "repository",
  reference: "core",
  scope: "research",
  input: "research/bundle.json",
  delivery: "authored_graph",
  catalog: "",
  bundle: "research/bundle.json",
  requirements: "",
  focus: "",
  node: "",
  label: "",
  id: "",
  revision: "",
  transaction: '{"schema_version":1,"transaction_id":"research-change","operations":[]}'
};

export function researchSnapshot() {
  const state = researchState;
  const target = state.targetKind === "repository"
    ? { kind: "repository", alias: state.reference }
    : { kind: "configured", path: state.reference, source_scope: state.scope };
  const payload: Record<string, unknown> = { operation: state.action };
  const command = ["relay-knowledge", ...state.action.replace("knowledge.map.", "map.").split(".")];
  const option = (flag: string, value: string) => {
    if (value) command.push(flag, `'${value.replaceAll("'", "'\\''")}'`);
  };
  if (state.action === "evidence.export") {
    Object.assign(payload, { id: state.id, source_scope: state.scope, revision: state.revision });
    option("--id", state.id); option("--scope", state.scope); option("--revision", state.revision);
  } else {
    payload.target = target;
    if (state.action.startsWith("knowledge.map.")) {
      try { payload.transaction = JSON.parse(state.transaction); }
      catch { payload.transaction = state.transaction; }
      command.push("--type", "knowledge", "--input", "transaction.json");
    } else {
      option("--root", state.targetKind === "configured" ? state.reference : ".");
      if (state.action === "research.status") {
        Object.assign(payload, { delivery: state.delivery, catalog: state.catalog || null,
          bundle: state.bundle || null, source_scope: state.scope || null, requirements: state.requirements || null });
        option("--delivery", state.delivery); option("--catalog", state.catalog); option("--bundle", state.bundle);
        option("--scope", state.scope); option("--requirements", state.requirements);
      } else {
        payload.input = state.input; option("--input", state.input);
        if (state.action !== "sources.audit") { payload.source_scope = state.scope; option("--scope", state.scope); }
        if (state.action === "evidence.view") { payload.focus = state.focus || null; option("--focus", state.focus); }
        if (state.action === "evidence.impact") {
          Object.assign(payload, { node: state.node, label: state.label });
          option("--node", state.node); option("--label", state.label);
        }
      }
    }
  }
  command.push("--format", "json");
  return { name: researchActions.find(([action]) => action === state.action)?.[1] ?? "Research workflow",
    command: command.join(" "), payload };
}
