const baseUrl = process.env.EDEN_API_BASE_URL || 'http://127.0.0.1:8787';

async function main() {
  const response = await fetch(`${baseUrl.replace(/\/$/, '')}/health/report`);
  if (!response.ok) {
    throw new Error(`health/report failed: ${response.status} ${response.statusText}`);
  }

  const report = await response.json();
  console.log(`overall=${report.status} service=${report.service} version=${report.version}`);
  console.log(`api bind=${report.api.bind_addr} persistence=${report.api.persistence_enabled} db=${report.api.db_path}`);

  for (const runtime of report.runtimes || []) {
    console.log(
      `runtime ${runtime.market}: status=${runtime.status} debounce=${runtime.debounce_ms}ms rest=${runtime.rest_refresh_secs}s metrics_every=${runtime.metrics_every_ticks}`
    );
    const stale = (runtime.artifacts || []).filter(item => item.status !== 'fresh');
    if (stale.length) {
      console.log(`  artifacts: ${stale.map(item => `${item.kind}:${item.status}`).join(', ')}`);
    }
    if (runtime.issue_summary) {
      console.log(
        `  issues: warnings=${runtime.issue_summary.warning_count} errors=${runtime.issue_summary.error_count} codes=${(runtime.issue_summary.last_issue_codes || []).join(',')}`
      );
    }
  }
}

main().catch(error => {
  console.error(error.stack || String(error));
  process.exit(1);
});
