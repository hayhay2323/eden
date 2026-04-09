import { create } from "zustand";

export type EdenMarket = "hk" | "us";
export type EdenObjectKind =
  | "market_session"
  | "symbol_state"
  | "case"
  | "recommendation"
  | "macro_event"
  | "thread"
  | "workflow";

export interface EdenSelectedObject {
  kind: EdenObjectKind;
  id: string;
  label?: string | null;
}

interface ShellState {
  market: EdenMarket;
  liveRefreshEnabled: boolean;
  selectedObject: EdenSelectedObject | null;
  selectedObjectTrail: EdenSelectedObject[];
  inspectorOpen: boolean;

  setMarket: (market: EdenMarket) => void;
  setLiveRefreshEnabled: (value: boolean) => void;
  setSelectedObject: (value: EdenSelectedObject | null) => void;
  jumpToObject: (index: number) => void;
  openObject: (value: EdenSelectedObject) => void;
  closeInspector: () => void;
}

export const useShellStore = create<ShellState>((set) => ({
  market: "hk",
  liveRefreshEnabled: true,
  selectedObject: null,
  selectedObjectTrail: [],
  inspectorOpen: false,

  setMarket: (market) =>
    set({
      market,
      selectedObject: null,
      selectedObjectTrail: [],
      inspectorOpen: false,
    }),
  setLiveRefreshEnabled: (liveRefreshEnabled) => set({ liveRefreshEnabled }),
  setSelectedObject: (selectedObject) =>
    set({
      selectedObject,
      selectedObjectTrail: selectedObject ? [selectedObject] : [],
      inspectorOpen: Boolean(selectedObject),
    }),
  jumpToObject: (index) =>
    set((state) => {
      if (index < 0 || index >= state.selectedObjectTrail.length) {
        return state;
      }
      const selectedObjectTrail = state.selectedObjectTrail.slice(0, index + 1);
      return {
        selectedObjectTrail,
        selectedObject: selectedObjectTrail[selectedObjectTrail.length - 1] ?? null,
        inspectorOpen: selectedObjectTrail.length > 0,
      };
    }),
  openObject: (selectedObject) =>
    set((state) => {
      const existingIndex = state.selectedObjectTrail.findIndex(
        (item) => item.kind === selectedObject.kind && item.id === selectedObject.id,
      );
      const selectedObjectTrail =
        existingIndex >= 0
          ? state.selectedObjectTrail.slice(0, existingIndex + 1)
          : [...state.selectedObjectTrail, selectedObject];
      return {
        selectedObject,
        selectedObjectTrail,
        inspectorOpen: true,
      };
    }),
  closeInspector: () =>
    set((state) => {
      if (state.selectedObjectTrail.length > 1) {
        const selectedObjectTrail = state.selectedObjectTrail.slice(0, -1);
        return {
          selectedObjectTrail,
          selectedObject: selectedObjectTrail[selectedObjectTrail.length - 1] ?? null,
          inspectorOpen: true,
        };
      }
      return {
        selectedObject: null,
        selectedObjectTrail: [],
        inspectorOpen: false,
      };
    }),
}));
