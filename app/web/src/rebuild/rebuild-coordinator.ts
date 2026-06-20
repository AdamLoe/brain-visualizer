export interface NetworkRebuildRequest {
  n: number;
  k: number;
  seed: number;
}

export interface RebuildSimulationSettings {
  iExt: number;
  synapticScale: number;
}

export interface RebuildBackendOps {
  reinitialize(
    n: number,
    k: number,
    seed: number,
    iExt: number,
    synapticScale: number,
  ): void;
  updateSettings(settings: Float32Array): void;
  applyMorphConfig(json: string): void;
}

export interface RebuildFrameInputs {
  visualSettings(): Float32Array;
  simulationSettings(): RebuildSimulationSettings;
  morphConfigJson(): string;
}

export type RebuildStep =
  | { kind: "idle" }
  | { kind: "network"; request: NetworkRebuildRequest; sequence: number }
  | { kind: "settings"; sequence: number }
  | { kind: "morphology"; sequence: number; json: string };

export class RebuildCoordinator {
  private pendingNetwork: (NetworkRebuildRequest & { sequence: number }) | null = null;
  private pendingSettingsPush = false;
  private pendingMorphConfig: string | null = null;
  private needsMorphRefresh = false;
  private nextSequence = 1;

  requestNetwork(request: NetworkRebuildRequest): number {
    const sequence = this.nextSequence++;
    this.pendingNetwork = { ...request, sequence };
    this.pendingSettingsPush = true;
    this.needsMorphRefresh = true;
    return sequence;
  }

  requestSettingsPush(): void {
    this.pendingSettingsPush = true;
  }

  requestMorphConfig(json: string): void {
    this.pendingMorphConfig = json;
  }

  hasPendingWork(): boolean {
    return this.pendingNetwork !== null
      || this.pendingSettingsPush
      || this.pendingMorphConfig !== null
      || this.needsMorphRefresh;
  }

  applyNext(backend: RebuildBackendOps, inputs: RebuildFrameInputs): RebuildStep {
    if (this.pendingNetwork !== null) {
      const request = this.pendingNetwork;
      this.pendingNetwork = null;
      const sim = inputs.simulationSettings();
      backend.reinitialize(
        request.n,
        request.k,
        request.seed >>> 0,
        sim.iExt,
        sim.synapticScale,
      );
      this.pendingSettingsPush = true;
      this.needsMorphRefresh = true;
      return {
        kind: "network",
        request: { n: request.n, k: request.k, seed: request.seed >>> 0 },
        sequence: request.sequence,
      };
    }

    if (this.pendingSettingsPush) {
      this.pendingSettingsPush = false;
      backend.updateSettings(inputs.visualSettings());
      return { kind: "settings", sequence: this.nextSequence - 1 };
    }

    if (this.pendingMorphConfig !== null || this.needsMorphRefresh) {
      const json = this.pendingMorphConfig ?? inputs.morphConfigJson();
      this.pendingMorphConfig = null;
      this.needsMorphRefresh = false;
      backend.applyMorphConfig(json);
      return { kind: "morphology", sequence: this.nextSequence - 1, json };
    }

    return { kind: "idle" };
  }
}
