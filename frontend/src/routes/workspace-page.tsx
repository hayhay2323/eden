import { Card } from "@blueprintjs/core";

const workspaceModules = [
  {
    title: "Queue board",
    note: "Pinned cases, queue ownership, stage transitions, and execution posture.",
  },
  {
    title: "Inspector",
    note: "Case detail, narrative deltas, invalidation state, and supporting evidence.",
  },
  {
    title: "Paired application",
    note: "Reserved for maps, tables, or workflow apps that the agent can steer.",
  },
];

export function WorkspacePage() {
  return (
    <div className="stage-grid stage-grid--single">
      <Card className="stage-card" elevation={0}>
        <div className="section-eyebrow">Workspace</div>
        <h1>Paired application surface</h1>
        <p className="page-intro">
          This route is reserved for dense operational modules, not generic empty states.
        </p>
        <div className="module-grid">
          {workspaceModules.map((module) => (
            <article key={module.title} className="module-card">
              <div className="module-card__eyebrow">Reserved module</div>
              <h3>{module.title}</h3>
              <p>{module.note}</p>
            </article>
          ))}
        </div>
      </Card>
    </div>
  );
}
