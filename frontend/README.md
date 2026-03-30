# Eden Frontend

Scaffold for the new Eden operator frontend.

Stack:

- React
- TypeScript
- Vite
- TanStack Router
- TanStack Query
- Zustand
- Blueprint

Commands:

```bash
npm install
npm run dev
npm run build
```

Environment:

- `VITE_EDEN_API_BASE_URL` defaults to `http://127.0.0.1:8787`

Current scope:

- app shell
- left thread rail
- center conversation surface
- right work-surface modules
- public backend health query

Not wired yet:

- authenticated `/api/agent/*` calls
- SSE streams
- real object/action/run/evidence modules
