import { invoke } from "@tauri-apps/api/core";
import type {
  AdapterStatus,
  DrossConfig,
  Report,
  RepositoryInfo,
  RiskEntry,
} from "./types";

export const api = {
  openRepository: (path: string) =>
    invoke<RepositoryInfo>("open_repository", { path }),

  currentRepository: () => invoke<RepositoryInfo>("current_repository"),

  analyze: (target: "staged" | "worktree") =>
    invoke<Report>("analyze", { args: { target } }),

  buildIndex: () => invoke<RepositoryInfo>("build_index"),

  listConnections: () => invoke<AdapterStatus[]>("list_connections"),

  installConnection: (id: string) =>
    invoke<AdapterStatus[]>("install_connection", { id }),

  uninstallConnection: (id: string) =>
    invoke<AdapterStatus[]>("uninstall_connection", { id }),

  riskHistory: (limit = 50) => invoke<RiskEntry[]>("risk_history", { limit }),

  fileSource: (path: string) => invoke<string>("file_source", { path }),

  getConfig: () => invoke<DrossConfig>("get_config"),

  setConfig: (config: DrossConfig) =>
    invoke<DrossConfig>("set_config", { config }),

  overrideAuthorship: (
    path: string,
    startLine: number,
    endLine: number,
    isAi: boolean,
  ) =>
    invoke<void>("override_authorship", {
      args: { path, startLine, endLine, isAi },
    }),
};
