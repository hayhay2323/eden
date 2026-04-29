import { useEffect, useState } from "react";
import { useLocation } from "@tanstack/react-router";

import type { WorkspaceFilter } from "./use-workspace-view";

export function useWorkspaceFilterSearch() {
  const location = useLocation({
    select: (value) => ({
      pathname: value.pathname,
      searchStr: value.searchStr,
      hash: value.hash,
    }),
  });

  const [activeFilter, setActiveFilter] = useState<WorkspaceFilter>(() =>
    parseWorkspaceFilter(location.searchStr),
  );

  useEffect(() => {
    const next = parseWorkspaceFilter(location.searchStr);
    setActiveFilter((current) => (sameWorkspaceFilter(current, next) ? current : next));
  }, [location.searchStr]);

  useEffect(() => {
    if (typeof window === "undefined") return;

    const nextSearch = buildWorkspaceSearch(activeFilter);
    const nextUrl = `${location.pathname}${nextSearch}${location.hash || ""}`;
    const currentUrl = `${window.location.pathname}${window.location.search}${window.location.hash}`;

    if (nextUrl !== currentUrl) {
      window.history.replaceState(window.history.state, "", nextUrl);
    }
  }, [activeFilter, location.hash, location.pathname]);

  return {
    activeFilter,
    setActiveFilter,
  };
}

function parseWorkspaceFilter(searchStr: string): WorkspaceFilter {
  const params = new URLSearchParams(searchStr);
  const queue = params.get("queue");
  const owner = params.get("owner");
  const reviewer = params.get("reviewer");

  if (queue === "review") return { kind: "review" };
  if (queue === "pinned") return { kind: "pinned" };
  if (owner && owner.trim().length > 0) return { kind: "owner", label: owner.trim() };
  if (reviewer && reviewer.trim().length > 0) {
    return { kind: "reviewer", label: reviewer.trim() };
  }

  return { kind: "all" };
}

function buildWorkspaceSearch(filter: WorkspaceFilter) {
  const params = new URLSearchParams();

  switch (filter.kind) {
    case "review":
      params.set("queue", "review");
      break;
    case "pinned":
      params.set("queue", "pinned");
      break;
    case "owner":
      params.set("owner", filter.label);
      break;
    case "reviewer":
      params.set("reviewer", filter.label);
      break;
    case "all":
    default:
      break;
  }

  const search = params.toString();
  return search.length > 0 ? `?${search}` : "";
}

function sameWorkspaceFilter(left: WorkspaceFilter, right: WorkspaceFilter) {
  if (left.kind !== right.kind) return false;
  if ("label" in left || "label" in right) {
    return ("label" in left ? left.label : undefined) === ("label" in right ? right.label : undefined);
  }
  return true;
}
