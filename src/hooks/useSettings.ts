import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppSettings {
  selected_device: string;
  source_lang: string;
  target_lang: string;
  tts_enabled: boolean;
  model_path: string;
  tts_output_device: string;
  vrchat_osc_enabled: boolean;
  vrchat_osc_port: number;
  backend: string;
}

export function useSettings() {
  const [loaded, setLoaded] = useState(false);
  const [selectedDevice, setSelectedDevice] = useState("");
  const [sourceLang, setSourceLang] = useState("en");
  const [targetLang, setTargetLang] = useState("zh");
  const [ttsEnabled, setTtsEnabled] = useState(false);
  const [modelPath, setModelPath] = useState("base");
  const [ttsOutputDevice, setTtsOutputDevice] = useState("");
  const [vrchatOscEnabled, setVrchatOscEnabled] = useState(false);
  const [vrchatOscPort, setVrchatOscPort] = useState(9000);
  const [backend, setBackend] = useState("cpu");
  const isInitialLoad = useRef(true);

  // Load settings on mount
  useEffect(() => {
    invoke<AppSettings>("load_settings")
      .then((s) => {
        setSelectedDevice(s.selected_device);
        setSourceLang(s.source_lang);
        setTargetLang(s.target_lang);
        setTtsEnabled(s.tts_enabled);
        setModelPath(s.model_path);
        setTtsOutputDevice(s.tts_output_device || "");
        setVrchatOscEnabled(s.vrchat_osc_enabled ?? false);
        setVrchatOscPort(s.vrchat_osc_port ?? 9000);
        setBackend(s.backend || "cpu");
        setLoaded(true);
      })
      .catch((e) => {
        console.error("Failed to load settings:", e);
        setLoaded(true); // use defaults
      });
  }, []);

  // Save settings whenever they change (skip the initial load)
  useEffect(() => {
    if (!loaded) return;
    if (isInitialLoad.current) {
      isInitialLoad.current = false;
      return;
    }
    const settings: AppSettings = {
      selected_device: selectedDevice,
      source_lang: sourceLang,
      target_lang: targetLang,
      tts_enabled: ttsEnabled,
      model_path: modelPath,
      tts_output_device: ttsOutputDevice,
      vrchat_osc_enabled: vrchatOscEnabled,
      vrchat_osc_port: vrchatOscPort,
      backend: backend,
    };
    invoke("save_settings", { settings }).catch((e) =>
      console.error("Failed to save settings:", e)
    );
  }, [loaded, selectedDevice, sourceLang, targetLang, ttsEnabled, modelPath, ttsOutputDevice, vrchatOscEnabled, vrchatOscPort, backend]);

  return {
    loaded,
    selectedDevice,
    setSelectedDevice: useCallback((v: string) => setSelectedDevice(v), []),
    sourceLang,
    setSourceLang: useCallback((v: string) => setSourceLang(v), []),
    targetLang,
    setTargetLang: useCallback((v: string) => setTargetLang(v), []),
    ttsEnabled,
    setTtsEnabled: useCallback((v: boolean) => setTtsEnabled(v), []),
    modelPath,
    setModelPath: useCallback((v: string) => setModelPath(v), []),
    ttsOutputDevice,
    setTtsOutputDevice: useCallback((v: string) => setTtsOutputDevice(v), []),
    vrchatOscEnabled,
    setVrchatOscEnabled: useCallback((v: boolean) => setVrchatOscEnabled(v), []),
    vrchatOscPort,
    setVrchatOscPort: useCallback((v: number) => setVrchatOscPort(v), []),
    backend,
    setBackend: useCallback((v: string) => setBackend(v), []),
  };
}
