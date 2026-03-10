export type AppDom = {
  workspaceLabel: HTMLElement;
  activeRuns: HTMLElement;
  totalRuns: HTMLElement;
  totalEvents: HTMLElement;
  totalWarnings: HTMLElement;
  promptInput: HTMLTextAreaElement;
  modelInput: HTMLSelectElement;
  sandboxInput: HTMLSelectElement;
  approvalInput: HTMLSelectElement;
  bypassInput: HTMLInputElement;
  settingsNote: HTMLElement;
  chooseFolderButton: HTMLButtonElement;
  addRunButton: HTMLButtonElement;
  newSessionButton: HTMLButtonElement;
  composerMessage: HTMLElement;
  errorBanner: HTMLElement;
  navSearchInput: HTMLInputElement;
  sessionNav: HTMLDivElement;
  detailTitle: HTMLElement;
  detailSubtitle: HTMLElement;
  detailStatus: HTMLElement;
  detailEvents: HTMLElement;
  detailCommands: HTMLElement;
  detailWarnings: HTMLElement;
  detailWorkspace: HTMLElement;
  detailCodexSession: HTMLElement;
  detailPrompt: HTMLElement;
  detailLatest: HTMLElement;
  detailMessage: HTMLElement;
  cancelRunButton: HTMLButtonElement;
  retryRunButton: HTMLButtonElement;
  deleteSessionButton: HTMLButtonElement;
  eventSearchInput: HTMLInputElement;
  kindFilter: HTMLSelectElement;
  detailEventsList: HTMLUListElement;
};

function queryRequired<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`required element not found: ${selector}`);
  }
  return element;
}

export function getAppDom(): AppDom {
  return {
    workspaceLabel: queryRequired("#workspaceLabel"),
    activeRuns: queryRequired("#activeRuns"),
    totalRuns: queryRequired("#totalRuns"),
    totalEvents: queryRequired("#totalEvents"),
    totalWarnings: queryRequired("#totalWarnings"),
    promptInput: queryRequired("#promptInput"),
    modelInput: queryRequired("#modelInput"),
    sandboxInput: queryRequired("#sandboxInput"),
    approvalInput: queryRequired("#approvalInput"),
    bypassInput: queryRequired("#bypassInput"),
    settingsNote: queryRequired("#settingsNote"),
    chooseFolderButton: queryRequired("#chooseFolderButton"),
    addRunButton: queryRequired("#addRunButton"),
    newSessionButton: queryRequired("#newSessionButton"),
    composerMessage: queryRequired("#composerMessage"),
    errorBanner: queryRequired("#errorBanner"),
    navSearchInput: queryRequired("#navSearchInput"),
    sessionNav: queryRequired("#sessionNav"),
    detailTitle: queryRequired("#detailTitle"),
    detailSubtitle: queryRequired("#detailSubtitle"),
    detailStatus: queryRequired("#detailStatus"),
    detailEvents: queryRequired("#detailEvents"),
    detailCommands: queryRequired("#detailCommands"),
    detailWarnings: queryRequired("#detailWarnings"),
    detailWorkspace: queryRequired("#detailWorkspace"),
    detailCodexSession: queryRequired("#detailCodexSession"),
    detailPrompt: queryRequired("#detailPrompt"),
    detailLatest: queryRequired("#detailLatest"),
    detailMessage: queryRequired("#detailMessage"),
    cancelRunButton: queryRequired("#cancelRunButton"),
    retryRunButton: queryRequired("#retryRunButton"),
    deleteSessionButton: queryRequired("#deleteSessionButton"),
    eventSearchInput: queryRequired("#eventSearchInput"),
    kindFilter: queryRequired("#kindFilter"),
    detailEventsList: queryRequired("#detailEventsList"),
  };
}
