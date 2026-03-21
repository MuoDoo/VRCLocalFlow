import { useState, useCallback, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SubtitleOverlay } from "./components/SubtitleOverlay";
import {
  SettingsPanel,
  WhisperModel,
  TranslationModelInfo,
} from "./components/SettingsPanel";
import { StatusBar } from "./components/StatusBar";
import { useTranslation } from "./hooks/useTranslation";
import { useAudioDevices } from "./hooks/useAudioDevices";
import { useSettings } from "./hooks/useSettings";

const NLLB_MODEL_ID = "nllb-200-distilled-600M";

interface BackendInfo {
  id: string;
  name: string;
  available: boolean;
}

// All language pairs use the single NLLB model
function resolveRequiredModelIds(source: string, target: string): string[] {
  if (source === target) return [];
  return [NLLB_MODEL_ID];
}

function App() {
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Persisted settings
  const {
    loaded,
    selectedDevice,
    setSelectedDevice,
    sourceLang,
    setSourceLang,
    targetLang,
    setTargetLang,
    ttsEnabled,
    setTtsEnabled,
    modelPath,
    setModelPath,
    ttsOutputDevice,
    setTtsOutputDevice,
    vrchatOscEnabled,
    setVrchatOscEnabled,
    vrchatOscPort,
    setVrchatOscPort,
    backend,
    setBackend,
  } = useSettings();

  // Whisper model management state
  const [models, setModels] = useState<WhisperModel[]>([]);
  const [downloadingModel, setDownloadingModel] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);

  // Translation model status
  const [translationModels, setTranslationModels] = useState<
    TranslationModelInfo[]
  >([]);
  const [downloadingTranslationModel, setDownloadingTranslationModel] = useState<string | null>(null);
  const [translationDownloadProgress, setTranslationDownloadProgress] = useState(0);

  const { entries, clear } = useTranslation();
  const {
    devices,
    loading: devicesLoading,
    refresh: refreshDevices,
  } = useAudioDevices();

  // Pipeline status from backend (errors, warnings)
  const [pipelineMessage, setPipelineMessage] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<{ status: string; message: string }>(
      "pipeline-status",
      (event) => {
        const { status, message } = event.payload;
        if (status === "warning" || status === "error") {
          setError(message);
        }
        setPipelineMessage(message);
        if (status === "stopped") {
          setRunning(false);
        }
      },
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Engine backends
  const [backends, setBackends] = useState<BackendInfo[]>([]);

  const refreshBackends = useCallback(async () => {
    try {
      const list = await invoke<BackendInfo[]>("list_backends");
      setBackends(list);
    } catch (e) {
      console.error("Failed to list backends:", e);
    }
  }, []);

  useEffect(() => {
    refreshBackends();
  }, [refreshBackends]);

  // Output devices for TTS
  const [outputDevices, setOutputDevices] = useState<{ name: string }[]>([]);
  const refreshOutputDevices = useCallback(async () => {
    try {
      const list = await invoke<{ name: string }[]>("list_output_devices");
      setOutputDevices(list);
    } catch (e) {
      console.error("Failed to list output devices:", e);
    }
  }, []);

  useEffect(() => {
    refreshOutputDevices();
  }, [refreshOutputDevices]);

  // Fetch whisper model list on mount
  const refreshModels = useCallback(async () => {
    try {
      const list = await invoke<WhisperModel[]>("list_whisper_models");
      setModels(list);
    } catch (e) {
      console.error("Failed to list models:", e);
    }
  }, []);

  useEffect(() => {
    refreshModels();
  }, [refreshModels]);

  // Fetch translation model status
  const refreshTranslationModels = useCallback(async () => {
    try {
      const list = await invoke<TranslationModelInfo[]>(
        "list_translation_models"
      );
      setTranslationModels(list);
    } catch (e) {
      console.error("Failed to list translation models:", e);
    }
  }, []);

  useEffect(() => {
    refreshTranslationModels();
  }, [refreshTranslationModels]);

  // Compute required translation models for current language pair
  const requiredModelIds = useMemo(
    () => resolveRequiredModelIds(sourceLang, targetLang),
    [sourceLang, targetLang]
  );

  const requiredTranslationModels = useMemo(
    () =>
      requiredModelIds
        .map((id) => translationModels.find((m) => m.id === id))
        .filter((m): m is TranslationModelInfo => m !== undefined),
    [requiredModelIds, translationModels]
  );

  const allTranslationModelsReady = useMemo(
    () =>
      requiredModelIds.length === requiredTranslationModels.length &&
      requiredTranslationModels.every((m) => m.downloaded),
    [requiredModelIds, requiredTranslationModels]
  );

  // Listen for whisper download progress events
  useEffect(() => {
    const unlisten = listen<{ model_id: string; progress: number }>(
      "model-download-progress",
      (event) => {
        setDownloadProgress(event.payload.progress);
        if (event.payload.progress >= 100) {
          setDownloadingModel(null);
          setDownloadProgress(0);
          refreshModels();
        }
      }
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [refreshModels]);

  const handleDownloadModel = useCallback(async (modelId: string) => {
    setDownloadingModel(modelId);
    setDownloadProgress(0);
    try {
      await invoke("download_whisper_model", { modelId });
    } catch (e) {
      setError(String(e));
      setDownloadingModel(null);
      setDownloadProgress(0);
    }
  }, []);

  // Listen for translation model download progress events
  useEffect(() => {
    const unlisten = listen<{ model_id: string; progress: number }>(
      "translation-download-progress",
      (event) => {
        setTranslationDownloadProgress(event.payload.progress);
        if (event.payload.progress >= 100) {
          setDownloadingTranslationModel(null);
          setTranslationDownloadProgress(0);
          refreshTranslationModels();
        }
      }
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [refreshTranslationModels]);

  const handleDownloadTranslationModel = useCallback(async (modelId: string) => {
    setDownloadingTranslationModel(modelId);
    setTranslationDownloadProgress(0);
    try {
      await invoke("download_translation_model", { modelId });
    } catch (e) {
      setError(String(e));
      setDownloadingTranslationModel(null);
      setTranslationDownloadProgress(0);
    }
  }, []);

  // Language handlers — auto-swap when user picks the same language
  const handleSourceLangChange = useCallback(
    (lang: string) => {
      if (lang === targetLang) {
        setTargetLang(sourceLang);
      }
      setSourceLang(lang);
    },
    [sourceLang, targetLang, setSourceLang, setTargetLang]
  );

  const handleTargetLangChange = useCallback(
    (lang: string) => {
      if (lang === sourceLang) {
        setSourceLang(targetLang);
      }
      setTargetLang(lang);
    },
    [sourceLang, targetLang, setSourceLang, setTargetLang]
  );

  const handleSwapLanguages = useCallback(() => {
    const oldSource = sourceLang;
    const oldTarget = targetLang;
    setSourceLang(oldTarget);
    setTargetLang(oldSource);
  }, [sourceLang, targetLang, setSourceLang, setTargetLang]);

  const togglePipeline = useCallback(async () => {
    setError(null);
    try {
      if (running) {
        await invoke("stop_pipeline");
        setRunning(false);
      } else {
        await invoke("start_pipeline", {
          config: {
            device_id: selectedDevice,
            source_lang: sourceLang,
            target_lang: targetLang,
            tts_enabled: ttsEnabled,
            model_path: modelPath,
            tts_output_device: ttsOutputDevice,
            vrchat_osc_enabled: vrchatOscEnabled,
            vrchat_osc_port: vrchatOscPort,
            backend: backend,
          },
        });
        clear();
        setRunning(true);
      }
    } catch (e) {
      setError(String(e));
    }
  }, [
    running,
    selectedDevice,
    sourceLang,
    targetLang,
    ttsEnabled,
    modelPath,
    ttsOutputDevice,
    vrchatOscEnabled,
    vrchatOscPort,
    backend,
    clear,
  ]);

  if (!loaded) {
    return (
      <div className="h-screen flex items-center justify-center bg-gray-900 text-gray-400">
        Loading...
      </div>
    );
  }

  const canStart = allTranslationModelsReady;

  return (
    <div className="h-screen flex flex-col bg-gray-900 text-white select-none">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 bg-gray-800 border-b border-gray-700">
        <h1 className="text-sm font-semibold tracking-wide text-gray-200">
          RTVT
        </h1>
        <div className="flex items-center gap-3">
          <button
            onClick={togglePipeline}
            disabled={!running && !canStart}
            className={`px-5 py-1.5 rounded-full text-sm font-medium transition-colors ${
              running
                ? "bg-red-600 hover:bg-red-500 text-white"
                : canStart
                ? "bg-green-600 hover:bg-green-500 text-white"
                : "bg-gray-600 text-gray-400 cursor-not-allowed"
            }`}
            title={
              !running && !canStart
                ? "Required translation models not found"
                : undefined
            }
          >
            {running ? "Stop" : "Start"}
          </button>
          <button
            onClick={() => setSettingsOpen(true)}
            className="text-gray-400 hover:text-white text-lg"
            title="Settings"
          >
            &#9881;
          </button>
        </div>
      </div>

      {/* Subtitle area */}
      <SubtitleOverlay entries={entries} />

      {/* Status bar */}
      <StatusBar running={running} error={error} message={pipelineMessage} />

      {/* Settings drawer */}
      <SettingsPanel
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        devices={devices}
        devicesLoading={devicesLoading}
        onRefreshDevices={refreshDevices}
        selectedDevice={selectedDevice}
        onSelectDevice={setSelectedDevice}
        sourceLang={sourceLang}
        targetLang={targetLang}
        onSourceLangChange={handleSourceLangChange}
        onTargetLangChange={handleTargetLangChange}
        onSwapLanguages={handleSwapLanguages}
        ttsEnabled={ttsEnabled}
        onTtsToggle={setTtsEnabled}
        outputDevices={outputDevices}
        selectedOutputDevice={ttsOutputDevice}
        onSelectOutputDevice={setTtsOutputDevice}
        modelPath={modelPath}
        onModelPathChange={setModelPath}
        models={models}
        downloadingModel={downloadingModel}
        downloadProgress={downloadProgress}
        onDownloadModel={handleDownloadModel}
        requiredTranslationModels={requiredTranslationModels}
        downloadingTranslationModel={downloadingTranslationModel}
        translationDownloadProgress={translationDownloadProgress}
        onDownloadTranslationModel={handleDownloadTranslationModel}
        vrchatOscEnabled={vrchatOscEnabled}
        onVrchatOscToggle={setVrchatOscEnabled}
        vrchatOscPort={vrchatOscPort}
        onVrchatOscPortChange={setVrchatOscPort}
        backends={backends}
        selectedBackend={backend}
        onBackendChange={setBackend}
      />
    </div>
  );
}

export default App;
