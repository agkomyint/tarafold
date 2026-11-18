import { createContext, useContext } from "react";
import type { Api, DataSample } from "./report/load";
import type { Report, TargetReport } from "./report/schema";

export interface AppState {
  report: Report;
  data: DataSample | null;
  api: Api | null;
  target: TargetReport;
  targetIndex: number;
  /** Equation shown in the Equations view, per target. */
  shownEquation: number;
  setShownEquation: (id: number) => void;
  /** Replace the report after a server-side re-run. */
  setReport: (r: Report) => void;
}

export const Ctx = createContext<AppState | null>(null);

export function useApp(): AppState {
  const c = useContext(Ctx);
  if (!c) throw new Error("no app state");
  return c;
}
