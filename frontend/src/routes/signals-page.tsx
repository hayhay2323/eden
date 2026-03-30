import { Card } from "@blueprintjs/core";

const signalModules = [
  {
    title: "Depth ladder",
    note: "L2 imbalance, spread changes, top-level concentration, and liquidity walls.",
  },
  {
    title: "Flow tape",
    note: "Trade bursts, directional flow, acceleration, and pressure persistence.",
  },
  {
    title: "Sector rotation",
    note: "Cross-symbol flow clusters, regime drift, and propagation anomalies.",
  },
];

export function SignalsPage() {
  return (
    <div className="stage-grid stage-grid--single">
      <Card className="stage-card" elevation={0}>
        <div className="section-eyebrow">Signals</div>
        <h1>Liquidity and signal modules</h1>
        <p className="page-intro">
          This route is reserved for the high-density market surfaces that actually justify the
          shell.
        </p>
        <div className="module-grid">
          {signalModules.map((module) => (
            <article key={module.title} className="module-card module-card--signal">
              <div className="module-card__eyebrow">Signal surface</div>
              <h3>{module.title}</h3>
              <p>{module.note}</p>
            </article>
          ))}
        </div>
      </Card>
    </div>
  );
}
