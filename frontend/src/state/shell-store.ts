import { create } from "zustand";

export type EdenMarket = "hk" | "us";
export type SidePanelTab = "objects" | "actions" | "runs" | "evidence";

interface ShellState {
  market: EdenMarket;
  sidePanelTab: SidePanelTab;
  selectedThreadId: string | null;
  setMarket: (market: EdenMarket) => void;
  setSidePanelTab: (tab: SidePanelTab) => void;
  setSelectedThreadId: (threadId: string | null) => void;
}

export const useShellStore = create<ShellState>((set) => ({
  market: "hk",
  sidePanelTab: "objects",
  selectedThreadId: "macro-open",
  setMarket: (market) => set({ market }),
  setSidePanelTab: (sidePanelTab) => set({ sidePanelTab }),
  setSelectedThreadId: (selectedThreadId) => set({ selectedThreadId }),
}));
