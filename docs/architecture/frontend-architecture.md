# Eden Frontend Architecture

This document fixes the current frontend boundary after the ontology/workbench refactor.

It is not a wishlist. It describes the shape that now exists in the repo and the rules for extending it without collapsing back into page-sized components or agent-shaped ad hoc UI.

## 1. Product Shape

The frontend is now organized as an ontology-driven workbench:

- `Market Desk`
- `Case Board`
- `Signals`

Each page keeps its own working surface visible.
Object detail, navigation, history, and graph traversal live in a shared right-side inspector instead of replacing the whole page.

This is intentional.

Do not regress back to:

- full-page object takeovers
- page-specific detail implementations
- agent-style payload rendering without object traversal

## 2. Stack

Current stack:

- `React`
- `TypeScript`
- `Vite`
- `TanStack Router`
- `TanStack Query`
- `Zustand`
- `Blueprint`

Current build scripts:

- `npm run dev`
- `npm run build`
- `npm run typecheck`

Note:

- `build` currently uses `vite build`
- `typecheck` remains separate

## 3. Directory Ownership

### `frontend/src/routes/*`

Routes are page composition only.

Allowed here:

- snapshot loading
- selected object lookup
- page-level composition
- page-specific filter hook wiring

Not allowed here:

- large board rendering logic
- object detail implementations
- graph/history rendering
- duplicated row/card renderers

### `frontend/src/features/desk/*`

Shared desk/workbench components.

This includes:

- market session presentation
- focus / actions / evidence panels
- object inspector shell

### `frontend/src/features/desk/object-inspector/*`

Shared object drill-down system.

This layer owns:

- shared stat/chip/card primitives
- detail cards
- history workbench
- graph workbench

This is the only place that should know how object navigation, history refs, and graph refs are rendered together.

### `frontend/src/features/workspace/*`

Case/workflow-specific board and view-model logic.

This layer also owns the operator-facing workflow/runtime panels that sit next to the case board, as long as they remain workspace-specific composition rather than turning into global shell chrome.

### `frontend/src/features/signals/*`

Symbol/sector/macro-specific board, controls, and view-model logic.

### `frontend/src/features/workbench/*`

Low-level page/workbench UI helpers.

Only keep small reusable building blocks here:

- `SurfaceKpi`
- `SelectionHint`
- small UI primitives such as tone badges

Do not turn this into a large generic design system unless the frontend actually starts needing one.

### `frontend/src/lib/query/*`

Query ownership only.

This layer owns:

- snapshot loading
- navigation loading
- neighborhood loading
- history loading
- graph node loading
- live refresh policy
- runtime task loading

### `frontend/src/state/*`

Global app state only.

Current ownership:

- active market
- live refresh enabled/disabled
- selected object
- object trail
- inspector open state

## 4. State Model

The frontend is object-first.

Primary selection state:

- `selectedObject`
- `selectedObjectTrail`

Do not reintroduce page-specific primary selection state such as:

- `selectedSymbol`
- `selectedCase`
- `selectedWorkflow`

Those can exist as local derived state if needed, but they must not become the global navigation model.

## 5. Data Model

The main application surface should continue to prefer:

1. `operational-snapshot`
2. `navigation`
3. `neighborhood`
4. `history_refs`
5. `graph_ref`

The frontend should remain object-oriented, not agent-payload-oriented.

That means:

- page boards consume object collections from `OperationalSnapshot`
- the inspector consumes `navigation`, `history`, and `graph`
- route/page code should not rebuild object relations manually

## 6. Page Pattern

All three pages should follow the same rule:

- left/main side = domain work surface
- right side = shared object inspector

Page-specific meaning:

### `Desk`

- market session
- focus
- actions
- evidence

### `Workspace`

- cases
- workflow stage distribution
- workflow queue
- operator workflows
- runtime task state
- notices / transitions

### `Signals`

- sector flows
- macro events
- symbol board
- sort/filter controls

## 7. Inspector Pattern

The inspector is a workbench, not a detail modal.

It owns:

- object overview
- relationship traversal
- history feed selection and loading
- graph node drill-down

The inspector should remain shared across pages.

If a new page needs object detail, it should use the shared inspector instead of creating its own detail implementation.

## 8. Live Refresh

Live refresh is now query-driven.

Current policy:

- snapshot refetches on interval when live mode is enabled
- navigation / neighborhood / history / graph also refetch on interval
- live mode can be paused from the topbar
- manual refresh invalidates all object-related query layers

This is intentionally simpler than SSE.

Do not add streaming complexity unless polling becomes a real limitation.

## 9. Styling Rules

The frontend now has a usable set of workbench primitives in CSS and small React helpers.

Preferred style order:

1. existing class
2. small reusable primitive
3. limited inline style only for dynamic CSS variables or one-off runtime values

Inline style is still acceptable for:

- CSS variable tone injection
- very small runtime-only values

Inline style is not acceptable for:

- repeated layout patterns
- repeated chip/button/card styling
- repeated page spacing conventions

## 10. Known Accepted Debt

The following debt is acceptable for now:

- `ObjectInspectorPanel` is still the largest single feature entry point, even after decomposition
- `app.css` is still large
- `vite build` has shown occasional runner weirdness in this environment
- full stream/SSE integration is not done

These are not the next priority unless they block product work.

## 11. Stop Lines

For the next phase, do not spend time on:

- more component splitting for its own sake
- abstracting every inline style into a design system
- adding separate detail implementations per page
- bringing back agent-shaped UI as the main surface

The current structure is good enough to build product features.

## 12. Next Product Work

If continuing from here, prefer feature work over architecture work:

- live market freshness indicators on boards
- better object summaries for selected case/symbol/workflow
- signal board linking to sector and macro object traversal
- workflow actions and operator controls
- better history summarization

The architecture should now be treated as stable enough to build on.
