export interface SessionInfo {
  id: string;
  agent: string;
  state: "Idle" | "Busy" | "WaitingPermission";
  workingDir: string | null;
}

export interface StoredMessage {
  id: number;
  sessionId: string;
  source: string;
  text: string;
  timestamp: number;
}
