import { useMutation, useQueryClient } from "@tanstack/react-query";

import {
  postCaseAssign,
  postCaseQueuePin,
  postCaseTransition,
} from "@/lib/api/client";
import { invalidateOperationalQueries } from "@/lib/query/operational";
import { useShellStore } from "@/state/shell-store";

const DEFAULT_ACTOR = "frontend-ui";

export function useCaseWorkflowActions(setupId: string | null) {
  const queryClient = useQueryClient();
  const market = useShellStore((s) => s.market);

  const invalidate = () => invalidateOperationalQueries(queryClient, market);

  const transition = useMutation({
    mutationFn: async (targetStage: string) => {
      if (!setupId) throw new Error("No case selected");
      return postCaseTransition(market, setupId, {
        target_stage: targetStage,
        actor: DEFAULT_ACTOR,
      });
    },
    onSuccess: invalidate,
  });

  const assign = useMutation({
    mutationFn: async (payload: {
      owner?: string | null;
      reviewer?: string | null;
      queue_pin?: string | null;
      note?: string;
    }) => {
      if (!setupId) throw new Error("No case selected");
      return postCaseAssign(market, setupId, {
        ...payload,
        actor: DEFAULT_ACTOR,
      });
    },
    onSuccess: invalidate,
  });

  const queuePin = useMutation({
    mutationFn: async (payload: {
      pinned: boolean;
      label?: string;
      note?: string;
    }) => {
      if (!setupId) throw new Error("No case selected");
      return postCaseQueuePin(market, setupId, {
        ...payload,
        actor: DEFAULT_ACTOR,
      });
    },
    onSuccess: invalidate,
  });

  const latestSuccess =
    transition.data ?? assign.data ?? queuePin.data ?? null;
  const latestAction =
    transition.data
      ? "transition"
      : assign.data
        ? "assignment"
        : queuePin.data
          ? "queue-pin"
          : null;

  return {
    transition,
    assign,
    queuePin,
    latestSuccess,
    latestAction,
    isPending:
      transition.isPending || assign.isPending || queuePin.isPending,
  };
}
